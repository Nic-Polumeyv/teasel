//! What the Svelte walker and the stylesheets share: the cursor over the source, the host's
//! nodes as rows, HTML's own rules, and the JavaScript read into the same tree.

use std::borrow::Cow;

use super::entities;
use crate::ast::{Ast, CommentKind, HostGroup, HostRow, List, NodeId, NodeKind, Opens, Run, Value};
use crate::error::{Code, SyntaxError};
use crate::interner::StrId;
use crate::lexer::scan::{find, find_pair, is_space};
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry as JsEntry, Options, Parser, Result};

/// A name a node type or a field has, by its number in `svelte::NAMES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Name(pub(crate) u32);

impl Name {
	/// No name: a node of the host's without a type, a block whose body is not filed.
	pub(crate) const NONE: Name = Name(u32::MAX);
}

/// The names `core` and the stylesheet write, spelled as they are written: the first of `svelte::NAMES`.
#[allow(non_upper_case_globals)]
pub(crate) mod own {
	macro_rules! own {
		($($name:ident)*) => {
			#[allow(non_camel_case_types)]
			#[repr(u32)]
			enum Number {
				$($name),*
			}
			$(pub(crate) const $name: super::Name = super::Name(Number::$name as u32);)*
			pub(crate) const NAMES: &[&str] = &[$(stringify!($name)),*];
		};
	}
	own! {
		Fragment nodes Text raw data Comment Attribute name value attributes fragment expression modifiers
		Script context content StyleSheet children comments styles comment CSSComment position Atrule
		prelude block Rule SelectorList ComplexSelector RelativeSelector combinator selectors Combinator
		TypeSelector namespace IdSelector ClassSelector NestingSelector PseudoClassSelector
		PseudoElementSelector args AttributeSelector matcher flags Nth Percentage Block Declaration property
		branches
	}
}

/// The shapes of the host's nodes, numbered for good as the walker meets them: a node's
/// type and each field's name and kind of value. The decoder builds the
/// nodes of one shape by one literal.
#[derive(Clone, Debug, Default)]
pub(crate) struct Shapes {
	/// Each type's shapes, by the type's number after the one of nodes without a type.
	// tried a table by hash with each type's last shape held in place: 0.3 ms slower a pass over Svelte's tests
	of: Vec<Vec<Shape>>,
	count: u32,
}

#[derive(Clone, Debug)]
struct Shape {
	id: u32,
	fields: Vec<u64>,
}

impl Shapes {
	pub(crate) fn of(&mut self, ty: Name, keys: &[u32], values: &[Value]) -> u32 {
		let at = ty.0.wrapping_add(1) as usize;
		if self.of.len() <= at {
			self.of.resize_with(at + 1, Vec::new);
		}
		let fields = keys
			.iter()
			.zip(values)
			.map(|(&key, value)| (key as u64) << 8 | crate::layout::tag(std::ptr::from_ref(value).cast()) as u64);
		let shapes = &mut self.of[at];
		if let Some(shape) = shapes
			.iter()
			.find(|shape| shape.fields.iter().copied().eq(fields.clone()))
		{
			return shape.id;
		}
		let id = self.count;
		self.count += 1;
		shapes.push(Shape {
			id,
			fields: fields.collect(),
		});
		id
	}
}

/// Whether the browser closes `current` when `next` opens inside it.
pub(crate) fn closes(current: &str, next: &str) -> bool {
	match current {
		"li" => next == "li",
		"dt" | "dd" => matches!(next, "dt" | "dd"),
		"p" => {
			matches!(
				next,
				"address"
					| "article" | "aside"
					| "blockquote" | "div"
					| "dl" | "fieldset"
					| "footer" | "form"
					| "h1" | "h2" | "h3"
					| "h4" | "h5" | "h6"
					| "header" | "hgroup"
					| "hr" | "main" | "menu"
					| "nav" | "ol" | "p"
					| "pre" | "section"
					| "table" | "ul"
			)
		}
		"rt" | "rp" => matches!(next, "rt" | "rp"),
		"optgroup" => next == "optgroup",
		"option" => matches!(next, "option" | "optgroup"),
		"thead" | "tbody" => matches!(next, "tbody" | "tfoot"),
		"tfoot" => next == "tbody",
		"tr" => matches!(next, "tr" | "tbody"),
		"td" | "th" => matches!(next, "td" | "th" | "tr"),
		_ => false,
	}
}

/// A valid element name: a doctype, a namespaced name, or a tag name as HTML spells one.
pub(crate) fn valid_name(name: &str) -> bool {
	let mut chars = name.chars();
	let Some(first) = chars.next() else { return false };
	if first == '!' {
		return name.len() > 1 && chars.all(|c| c.is_ascii_alphabetic());
	}
	if !first.is_ascii_alphabetic() {
		return false;
	}
	if let Some((prefix, rest)) = name.split_once(':') {
		return prefix.chars().all(|c| c.is_ascii_alphanumeric())
			&& rest.starts_with(|c: char| c.is_ascii_alphabetic())
			&& rest.ends_with(|c: char| c.is_ascii_alphanumeric())
			&& rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
	}
	let mut seen_dash = false;
	for c in chars {
		if c == '-' {
			seen_dash = true;
		} else if !c.is_ascii_alphanumeric()
			&& !(seen_dash && (c == '.' || c == '_' || c == '\u{b7}' || c as u32 >= 0xc0))
		{
			return false;
		}
	}
	true
}

// code points a browser repairs when a reference names them
const WINDOWS_1252: [u32; 32] = [
	8364, 129, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 141, 381, 143, 144, 8216, 8217, 8220,
	8221, 8226, 8211, 8212, 732, 8482, 353, 8250, 339, 157, 382, 376,
];

fn valid_code(code: u32, attribute: bool) -> u32 {
	if code == 10 && !attribute {
		return 32;
	}
	if code < 128 {
		return code;
	}
	if code <= 159 {
		return WINDOWS_1252[code as usize - 128];
	}
	if (55296..=57343).contains(&code) {
		return 0;
	}
	if code <= 0x2ffff || (0xe0000..=0xe007f).contains(&code) || (0xe0100..=0xe01ef).contains(&code) {
		return code;
	}
	0
}

/// Character references decoded as the browser would: the longest named reference wins, and in
/// an attribute one without its `;` stays text before `=` or a word character.
pub(crate) fn decode(raw: &str, attribute: bool) -> Cow<'_, str> {
	if !raw.contains('&') {
		return Cow::Borrowed(raw);
	}
	let mut out = String::with_capacity(raw.len());
	let bytes = raw.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] != b'&' {
			let start = i;
			while i < bytes.len() && bytes[i] != b'&' {
				i += 1;
			}
			out.push_str(&raw[start..i]);
			continue;
		}
		let rest = &raw[i + 1..];
		let mut code = None;
		let mut mark = None;
		let mut consumed = 0;
		if let Some(number) = rest.strip_prefix('#') {
			let (digits, hex) = match number.strip_prefix(['x', 'X']) {
				Some(hex) => (hex, true),
				None => (number, false),
			};
			let len = digits
				.bytes()
				.take_while(|b| if hex { b.is_ascii_hexdigit() } else { b.is_ascii_digit() })
				.count();
			if len > 0 {
				code = u32::from_str_radix(&digits[..len], if hex { 16 } else { 10 }).ok();
				consumed = 1 + usize::from(hex) + len;
				if rest.as_bytes().get(consumed) == Some(&b';') {
					consumed += 1;
				}
			}
		} else {
			let len = rest
				.bytes()
				.take(entities::LONGEST)
				.take_while(|b| b.is_ascii_alphanumeric())
				.count();
			let with = rest.as_bytes().get(len) == Some(&b';');
			let bare = (1..=len.min(entities::LONGEST_BARE)).rev();
			for try_len in with.then_some(len + 1).into_iter().chain(bare) {
				let Some((found, found_mark)) = entities::lookup(&rest[..try_len]) else {
					continue;
				};
				let after = rest.as_bytes().get(try_len);
				if try_len <= len
					&& attribute && after.is_some_and(|b| *b == b'=' || b.is_ascii_alphanumeric() || *b == b'_')
				{
					continue;
				}
				code = Some(found);
				mark = found_mark;
				consumed = try_len;
				break;
			}
		}
		match code.filter(|&c| c != 0) {
			Some(code) => {
				out.push(char::from_u32(valid_code(code, attribute)).unwrap_or('\0'));
				out.extend(mark);
				i += 1 + consumed;
			}
			None => {
				out.push('&');
				i += 1;
			}
		}
	}
	Cow::Owned(out)
}

/// Where the tag ends: the first `>` outside quotes, or the end.
fn tag_end(after: &str) -> usize {
	let mut quote = None;
	for (i, c) in after.char_indices() {
		match quote {
			Some(q) if c == q => quote = None,
			Some(_) => {}
			None if c == '"' || c == '\'' => quote = Some(c),
			None if c == '>' => return i,
			None => {}
		}
	}
	after.len()
}

/// Whether a `<name` tag in `src`, outside HTML comments, carries one of `wanted`: an attribute,
/// with the text value it needs, if any.
pub(crate) fn tag_carries(src: &str, name: &str, wanted: &[(&str, Option<&str>)]) -> bool {
	if wanted.is_empty() {
		return false;
	}
	// stopping at every `<` took 1.22 ms a pass over Svelte's tests, the lexer's scan for `<` 0.64, this 0.52
	let (bytes, first) = (src.as_bytes(), name.as_bytes().first().copied());
	let mut at = 0;
	loop {
		at = match first {
			Some(first) => find_pair(bytes, at, b'<', [b'!', first]),
			None => find(bytes, at, *b"<", false),
		};
		if at >= src.len() {
			return false;
		}
		let rest = &src[at + 1..];
		if let Some(after) = rest.strip_prefix("!--") {
			let Some(j) = after.find("-->") else {
				return false;
			};
			at = src.len() - after.len() + j + 3;
			continue;
		}
		let Some(after) = rest.strip_prefix(name).filter(|after| after.starts_with(is_space)) else {
			at += 1;
			continue;
		};
		let tag_end = tag_end(after);
		let mut attributes = after[..tag_end].trim_start_matches(is_space);
		while !attributes.is_empty() {
			let name_len = attributes
				.find(|c: char| is_space(c) || c == '=' || c == '/')
				.unwrap_or(attributes.len());
			let name = &attributes[..name_len];
			attributes = attributes[name_len..].trim_start_matches(is_space);
			let mut value = None;
			if let Some(after) = attributes.strip_prefix('=') {
				let after = after.trim_start_matches(is_space);
				let (text, len) = match after.chars().next() {
					Some(q @ ('"' | '\'')) => {
						let close = after[1..].find(q).unwrap_or(after.len() - 1);
						(&after[1..1 + close], (1 + close + 1).min(after.len()))
					}
					_ => {
						let len = after.find(is_space).unwrap_or(after.len());
						(&after[..len], len)
					}
				};
				value = Some(text);
				attributes = after[len..].trim_start_matches(is_space);
			} else if name.is_empty() {
				let mut chars = attributes.chars();
				chars.next();
				attributes = chars.as_str();
			}
			if wanted
				.iter()
				.any(|&(attribute, wanted)| name == attribute && wanted.is_none_or(|w| value == Some(w)))
			{
				return true;
			}
		}
		at = src.len() - after.len() + tag_end;
	}
}

/// A word that cannot name a binding.
pub(crate) fn reserved(word: &str) -> bool {
	use crate::lexer::token::word::{ENUM, KEYWORD, STRICT, flags};
	flags(word) & (KEYWORD | STRICT | ENUM) != 0
}

/// The length of a `</name>` closer at the start of `rest`, in any case, attributes and all.
pub(crate) fn closing_tag(rest: &str, name: &str) -> Option<usize> {
	let head = 2 + name.len();
	let start = rest.get(..head)?;
	if !start.starts_with("</") || !start[2..].eq_ignore_ascii_case(name) {
		return None;
	}
	let tail = &rest[head..];
	if tail.starts_with('>') {
		return Some(head + 1);
	}
	if !tail.starts_with(is_space) {
		return None;
	}
	tail.find('>').map(|i| head + i + 1)
}

/// Buffers given back once used, for the next of their kind.
pub(crate) struct Pool<T>(Vec<Vec<T>>);

impl<T> Default for Pool<T> {
	fn default() -> Self {
		Pool(Vec::new())
	}
}

impl<T> Pool<T> {
	pub(crate) fn take(&mut self) -> Vec<T> {
		self.0.pop().unwrap_or_default()
	}

	pub(crate) fn give(&mut self, mut buffer: Vec<T>) {
		buffer.clear();
		self.0.push(buffer);
	}
}

/// What reading an attribute gives: its node, and the kind and name it must not repeat on the
/// element, when it has such a name.
pub(crate) type Attribute = (NodeId, Option<(Name, StrId)>);

/// The cursor over a template's source and the tree it writes: the host's rows, numbered by
/// shape, and the JavaScript read into it.
pub(crate) struct Core<'a> {
	pub(crate) src: &'a str,
	pub(crate) full: u32,
	pub(crate) shapes: std::cell::RefMut<'a, Shapes>,
	pub(crate) options: Options,
	pub(crate) ast: Option<Box<Ast>>,
	pub(crate) at: u32,
	/// Where the JavaScript read at the cursor must end: the source, or an attribute value.
	pub(crate) limit: u32,
	/// The JavaScript read last ended at one of its stops.
	pub(crate) stopped: bool,
	/// How many CSS blocks and pseudo-class argument lists are open.
	pub(crate) nesting: u32,
}

impl<'a> Core<'a> {
	pub(crate) fn new(
		src: &'a str,
		full: u32,
		shapes: std::cell::RefMut<'a, Shapes>,
		options: Options,
		reused: Option<Box<Ast>>,
	) -> Self {
		Core {
			src,
			full,
			shapes,
			options,
			ast: Some(reused.unwrap_or_else(|| Box::new(Ast::sized(full as usize)))),
			at: 0,
			limit: src.len() as u32,
			stopped: false,
			nesting: 0,
		}
	}

	pub(crate) fn ast(&mut self) -> &mut Ast {
		self.ast.as_deref_mut().unwrap()
	}

	pub(crate) fn tree(&self) -> &Ast {
		self.ast.as_deref().unwrap()
	}

	pub(crate) fn len(&self) -> u32 {
		self.src.len() as u32
	}

	pub(crate) fn rest(&self) -> &'a str {
		&self.src[self.at as usize..]
	}

	pub(crate) fn byte(&self) -> Option<u8> {
		self.src.as_bytes().get(self.at as usize).copied()
	}

	pub(crate) fn char(&self) -> Option<char> {
		self.rest().chars().next()
	}

	pub(crate) fn matches(&self, s: &str) -> bool {
		self.rest().starts_with(s)
	}

	/// A word of the host's, followed by nothing that continues an identifier.
	pub(crate) fn word(&self, s: &str) -> bool {
		self.matches(s) && !self.rest()[s.len()..].starts_with(is_id_continue)
	}

	pub(crate) fn eat(&mut self, s: &str) -> bool {
		if self.matches(s) {
			self.at += s.len() as u32;
			true
		} else {
			false
		}
	}

	pub(crate) fn expect(&mut self, s: &str) -> Result<()> {
		if self.eat(s) {
			Ok(())
		} else {
			self.report_at(self.at, self.at, Code::Expected, Some(s))
		}
	}

	pub(crate) fn recovering(&self) -> bool {
		self.options.has(Options::ERROR_RECOVERY)
	}

	/// Under recovery the error is recorded on the tree and reading goes on; otherwise it ends
	/// the parse.
	pub(crate) fn report(&mut self, error: Box<SyntaxError>) -> Result<()> {
		if self.recovering() {
			self.ast().errors.push(*error);
			Ok(())
		} else {
			Err(error)
		}
	}

	pub(crate) fn report_at(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<()> {
		let error = self.error(pos, end, code, arg);
		self.report(error)
	}

	pub(crate) fn fail<T>(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<T> {
		Err(self.error(pos, end, code, arg))
	}

	pub(crate) fn error(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Box<SyntaxError> {
		let error = SyntaxError::new(pos, code).to(end);
		Box::new(match arg {
			Some(arg) => error.arg(self.intern(arg)),
			None => error,
		})
	}

	/// The word that ends the tag being read, skipped to under recovery.
	pub(crate) fn skip_to(&mut self, end: &str) {
		if let Some(i) = self.rest().find(end) {
			self.at += (i + end.len()) as u32;
		} else {
			self.at = self.len();
		}
	}

	pub(crate) fn space(&mut self) {
		let bytes = self.src.as_bytes();
		let mut i = self.at as usize;
		// ASCII by its bytes, the rest by its characters
		while let Some(&b) = bytes.get(i) {
			if b < 0x80 {
				if !matches!(b, b' ' | b'\t'..=b'\r') {
					break;
				}
				i += 1;
			} else {
				let c = self.src[i..].chars().next().unwrap();
				if !is_space(c) {
					break;
				}
				i += c.len_utf8();
			}
		}
		self.at = i as u32;
	}

	pub(crate) fn require_space(&mut self) -> Result<()> {
		match self.char() {
			Some(c) if is_space(c) => {
				self.space();
				Ok(())
			}
			_ => self.fail(self.at, self.at, Code::Expected, Some("whitespace")),
		}
	}

	pub(crate) fn intern(&mut self, s: &str) -> StrId {
		self.ast().strings.intern(s)
	}

	/// A string field: a slice of the source where the text is the source's, interned otherwise.
	pub(crate) fn text(&mut self, start: u32, end: u32, value: Cow<str>) -> Value {
		match value {
			Cow::Borrowed(_) => Value::Slice(start, end),
			Cow::Owned(owned) => Value::Str(self.intern(&owned)),
		}
	}

	pub(crate) fn host(&mut self, ty: Name, start: u32, end: u32, fields: &[(Name, Value)]) -> NodeId {
		self.opening(ty, start, end, fields, None)
	}

	pub(crate) fn opening(
		&mut self,
		ty: Name,
		start: u32,
		end: u32,
		fields: &[(Name, Value)],
		scope: Option<Opens>,
	) -> NodeId {
		let ast = self.ast.as_deref_mut().unwrap();
		let from = ast.host_keys.len();
		for &(key, value) in fields {
			ast.host_keys.push(key.0);
			ast.host_vals.push(value);
		}
		let shape = self.shapes.of(ty, &ast.host_keys[from..], &ast.host_vals[from..]);
		let index = ast.host_rows.len() as u32;
		ast.host_rows.push(HostRow {
			ty: ty.0,
			fields: Run {
				start: from as u32,
				len: fields.len() as u32,
			},
			shape,
		});
		ast.host_scopes.push(scope);
		ast.add(NodeKind::Host(index), start, end)
	}

	pub(crate) fn text_chunk(&mut self, start: u32, end: u32, attribute: bool) -> NodeId {
		let raw = &self.src[start as usize..end as usize];
		let data = decode(raw, attribute);
		let data = self.text(start, end, data);
		self.host(
			own::Text,
			start,
			end,
			&[(own::raw, Value::Slice(start, end)), (own::data, data)],
		)
	}

	pub(crate) fn flush_text(&mut self, from: u32, to: u32, chunks: &mut Vec<NodeId>) {
		if to > from {
			let text = self.text_chunk(from, to, true);
			chunks.push(text);
		}
	}

	/// The nodes as a fragment: positioned for the walks that order nodes, though none are written,
	/// at the cursor when there are none.
	pub(crate) fn fragment(&mut self, nodes: &[NodeId]) -> NodeId {
		let list = self.list(nodes);
		let (start, end) = match (nodes.first(), nodes.last()) {
			(Some(&first), Some(&last)) => (self.tree().node(first).start, self.tree().node(last).end),
			_ => (self.at, self.at),
		};
		self.host(own::Fragment, start, end, &[(own::nodes, Value::Nodes(list))])
	}

	/// The scope of an element whose `attributes` field is at that index of its fields, `fragment`
	/// after it: what its directives declare over both, then its fragment's own.
	pub(crate) fn element_scope(&mut self, attributes: usize, fragment: NodeId, declared: &[NodeId]) -> Opens {
		let base = self.ast().host_keys.len() as u32 + attributes as u32;
		let at = self.ast().host_groups.len() as u32;
		if !declared.is_empty() {
			let inside = self.list(declared);
			self.ast().host_groups.push(HostGroup {
				inside,
				from: base,
				until: base + 2,
				node: None,
			});
		}
		self.ast().host_groups.push(HostGroup {
			inside: List::EMPTY,
			from: base + 1,
			until: base + 2,
			node: Some(fragment),
		});
		Opens {
			outside: List::EMPTY,
			groups: (at, self.ast().host_groups.len() as u32 - at),
		}
	}

	pub(crate) fn list(&mut self, nodes: &[NodeId]) -> List {
		self.ast().add_list_from(nodes.iter().map(|&id| Some(id)))
	}

	pub(crate) fn host_type(&self, id: NodeId) -> Option<Name> {
		let ast = self.tree();
		match ast.node(id).kind {
			NodeKind::Host(index) => Some(Name(ast.host_rows[index as usize].ty)),
			_ => None,
		}
	}

	pub(crate) fn field_of(&self, id: NodeId, key: Name) -> Option<Value> {
		let ast = self.tree();
		let NodeKind::Host(index) = ast.node(id).kind else {
			return None;
		};
		ast.host_rows[index as usize]
			.fields
			.indices()
			.find(|&i| ast.host_keys[i as usize] == key.0)
			.map(|i| ast.host_vals[i as usize])
	}

	pub(crate) fn string_of(&self, value: Value) -> Option<&str> {
		match value {
			Value::Str(s) => Some(self.tree().str(s)),
			Value::Slice(a, b) => Some(&self.src[a as usize..b as usize]),
			_ => None,
		}
	}

	pub(crate) fn attribute_named(&self, id: NodeId, name: &str) -> bool {
		self.host_type(id) == Some(own::Attribute)
			&& self.field_of(id, own::name).and_then(|v| self.string_of(v)) == Some(name)
	}

	/// The one chunk of an attribute's value, when it has one.
	pub(crate) fn attribute_chunk(&self, id: NodeId) -> Option<NodeId> {
		match self.field_of(id, own::value)? {
			Value::Node(tag) => Some(tag),
			Value::Nodes(list) => match self.tree().list(list) {
				[Some(chunk)] => Some(*chunk),
				_ => None,
			},
			_ => None,
		}
	}

	/// The text of `name="text"`.
	pub(crate) fn attribute_text(&self, id: NodeId) -> Option<&str> {
		let Value::Nodes(_) = self.field_of(id, own::value)? else {
			return None;
		};
		let chunk = self.attribute_chunk(id)?;
		if self.host_type(chunk) != Some(own::Text) {
			return None;
		}
		self.string_of(self.field_of(chunk, own::data)?)
	}

	/// Whether a `<` at `i` starts a tag: a name, a closing tag or a comment follows.
	pub(crate) fn tag_start(&self, i: usize) -> bool {
		let bytes = self.src.as_bytes();
		bytes[i] == b'<' && !matches!(bytes.get(i + 1), Some(b) if !b.is_ascii_alphabetic() && *b != b'/' && *b != b'!')
	}

	/// Where the first closing tag of `name` starts and where it ends, from the cursor; the length
	/// twice when there is none.
	pub(crate) fn closing(&self, name: &str) -> (u32, u32) {
		let bytes = self.src.as_bytes();
		let mut at = self.at as usize;
		// found by `str::find`, a pass over Svelte's tests took 1% longer
		loop {
			at = find_pair(bytes, at, b'<', *b"/");
			if at >= bytes.len() {
				return (self.len(), self.len());
			}
			if let Some(len) = closing_tag(&self.src[at..], name) {
				return (at as u32, (at + len) as u32);
			}
			at += 1;
		}
	}

	pub(crate) fn tag_name(&mut self, attribute: bool) -> Result<&'a str> {
		let start = self.at;
		if start >= self.len() {
			if attribute && self.recovering() {
				return Ok("");
			}
			return self.fail(self.len(), self.len(), Code::UnexpectedEof, None);
		}
		let bytes = self.src.as_bytes();
		let mut i = start as usize;
		// ASCII by its bytes, the rest by its characters
		while let Some(&b) = bytes.get(i) {
			if b < 0x80 {
				if matches!(b, b' ' | b'\t'..=b'\r' | b'/' | b'>') || (attribute && matches!(b, b'"' | b'\'' | b'=')) {
					break;
				}
				i += 1;
			} else {
				let c = self.src[i..].chars().next().unwrap();
				if is_space(c) {
					break;
				}
				i += c.len_utf8();
			}
		}
		self.at = i as u32;
		Ok(&self.src[start as usize..i])
	}

	/// A quoted or bare text value after `=`, as one text chunk.
	pub(crate) fn text_value(&mut self) -> Result<Value> {
		let value_start = self.at;
		let quote = self.char().filter(|c| matches!(c, '"' | '\''));
		let (raw_start, raw_end) = match quote {
			Some(q) => {
				self.at += 1;
				let Some(len) = self.rest().find(q) else {
					return self.fail(value_start, value_start, Code::Expected, Some("an attribute value"));
				};
				let raw = (self.at, self.at + len as u32);
				self.at += len as u32 + 1;
				raw
			}
			None => {
				let len = self
					.rest()
					.find(|c: char| c == '>' || is_space(c))
					.unwrap_or(self.rest().len());
				if len == 0 {
					return self.fail(value_start, value_start, Code::Expected, Some("an attribute value"));
				}
				self.at += len as u32;
				(value_start, self.at)
			}
		};
		let text = self.text_chunk(raw_start, raw_end, true);
		let list = self.list(&[text]);
		Ok(Value::Nodes(list))
	}

	/// `name`, `name=value` or `name="text"`: a plain attribute, as a script tag or a verbatim
	/// element takes them.
	pub(crate) fn static_attribute(&mut self) -> Result<Option<Attribute>> {
		let start = self.at;
		let name = self.tag_name(true)?;
		if name.is_empty() {
			return Ok(None);
		}
		let mut value = Value::Bool(true);
		if self.eat("=") {
			self.space();
			value = self.text_value()?;
		}
		if self.char().is_some_and(|c| c == '"' || c == '\'') {
			return self.fail(self.at, self.at, Code::Expected, Some("="));
		}
		let name_id = self.intern(name);
		let node = self.host(
			own::Attribute,
			start,
			self.at,
			&[(own::name, Value::Str(name_id)), (own::value, value)],
		);
		Ok(Some((node, Some((own::Attribute, name_id)))))
	}

	pub(crate) fn comment_between_attributes(&mut self) -> bool {
		let start = self.at;
		let (kind, len) = if self.matches("//") {
			(CommentKind::Line, self.rest().find('\n').unwrap_or(self.rest().len()))
		} else if self.matches("/*") {
			match self.rest()[2..].find("*/") {
				Some(i) => (CommentKind::Block, i + 4),
				None => (CommentKind::Unclosed, self.rest().len()),
			}
		} else {
			return false;
		};
		self.at += len as u32;
		let end = self.at;
		self.ast().comments.push(crate::ast::Comment { kind, start, end });
		true
	}

	/// One expression read strictly, so that recovery cannot stand in for what is not one, and
	/// taken back unless `keep` says it ended where it should.
	pub(crate) fn trial(&mut self, stop: &str, keep: impl FnOnce(&mut Self) -> bool) -> Option<List> {
		let (start, comments, mark) = (self.at, self.tree().comments.len(), self.tree().mark());
		let options = self.options;
		self.options = options.without(Options::ERROR_RECOVERY);
		let read = self.js(JsEntry::Expression, stop);
		self.options = options;
		if let Ok(roots) = read
			&& keep(self)
		{
			return Some(roots);
		}
		self.at = start;
		self.ast().comments.truncate(comments);
		self.ast().truncate(mark);
		None
	}

	/// The JavaScript at the cursor, read by the parser into the same tree up to the limit; the
	/// cursor moves past it.
	pub(crate) fn js(&mut self, entry: JsEntry, stop: &str) -> Result<List> {
		if self.options.typescript() {
			self.js_in::<true>(entry, stop)
		} else {
			self.js_in::<false>(entry, stop)
		}
	}

	fn js_in<const TS: bool>(&mut self, entry: JsEntry, stop: &str) -> Result<List> {
		let ast = self.ast.take().unwrap();
		let src = &self.src[..self.limit as usize];
		let mut parser = Parser::<TS>::new(src, self.at, self.options, stop, ast);
		let first = parser.tok.start;
		let roots = match parser.start().and_then(|()| parser.read_entry(entry)) {
			// the placeholder spans what was read: a host copies an expression's text by its range
			Err(error) if parser.recovering() => {
				let at = error.pos;
				parser.record(Err(error)).unwrap();
				parser.skip_to_end();
				parser.prev_end = parser.prev_end.max(at);
				if entry == JsEntry::Params {
					Ok(List::EMPTY)
				} else {
					let name = parser.intern("");
					let end = parser.consumed_end().max(first);
					let placeholder = parser.add_with_end(NodeKind::Identifier { name }, first, end);
					Ok(parser.list_of(&[placeholder]))
				}
			}
			result => result,
		};
		let end = parser.consumed_end();
		self.stopped = roots.is_ok() && parser.stopped();
		self.ast = Some(parser.finish());
		let roots = roots?;
		self.at = end;
		Ok(roots)
	}

	/// The one root a JavaScript entry read.
	pub(crate) fn first(&self, roots: List) -> NodeId {
		self.tree().nth(roots, 0).unwrap()
	}

	pub(crate) fn program(&mut self, start: u32, end: u32) -> Result<NodeId> {
		if self.options.typescript() {
			self.program_in::<true>(start, end)
		} else {
			self.program_in::<false>(start, end)
		}
	}

	fn program_in<const TS: bool>(&mut self, start: u32, end: u32) -> Result<NodeId> {
		let ast = self.ast.take().unwrap();
		let src = &self.src[..end as usize];
		// the template may declare what the script exports
		let options = self.options.with(Options::ALLOW_UNDECLARED_EXPORTS);
		let mut parser = Parser::<TS>::new(src, start, options, "", ast);
		let program = parser.start().and_then(|()| parser.parse_program());
		self.ast = Some(parser.finish());
		program
	}

	/// An empty identifier standing where one could not be read.
	pub(crate) fn placeholder(&mut self, start: u32, end: u32) -> NodeId {
		let name = self.intern("");
		self.ast().add(NodeKind::Identifier { name }, start, end)
	}

	/// An identifier the host reads itself, as a node.
	pub(crate) fn identifier(&mut self) -> Result<NodeId> {
		let start = self.at;
		match self.char() {
			Some(c) if is_id_start(c) => self.at += c.len_utf8() as u32,
			_ => {
				// under recovery an empty identifier stands where one was expected
				self.report_at(start, start, Code::Expected, Some("an identifier"))?;
				return Ok(self.placeholder(start, start));
			}
		}
		while let Some(c) = self.char() {
			if !is_id_continue(c) {
				break;
			}
			self.at += c.len_utf8() as u32;
		}
		let end = self.at;
		let word = &self.src[start as usize..end as usize];
		if reserved(word) || (self.options.has(Options::MODULE) && word == "await") {
			self.report_at(start, end, Code::ReservedWord, Some(word))?;
		}
		let name = self.intern(word);
		Ok(self.ast().add(NodeKind::Identifier { name }, start, end))
	}
}
