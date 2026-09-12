//! The host layer: a template language read by its grammar, the JavaScript inside it read by
//! the parser, one tree for both. The walker knows what every such language shares, tags and
//! text and where JavaScript begins; the grammar says the rest.

mod css;
pub mod entities;
pub mod grammar;

use std::borrow::Cow;

use crate::ast::{Ast, Comment, CommentKind, Host, HostGroup, List, NodeId, NodeKind, Opens, Value, VariableKind};
use crate::error::{Code, SyntaxError};
use crate::interner::StrId;
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry as JsEntry, Extension, Options, Parser, Result};
pub use grammar::Grammar;
use grammar::{
	Alternative, BlockRule, Body, DirectiveRule, DirectiveValue, DocField, Entry, Form, Item, Match, RootField,
	TagRule, Unique, component_name,
};

/// Whether the browser closes `current` when `next` opens inside it.
fn closes(current: &str, next: &str) -> bool {
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

fn is_space(c: char) -> bool {
	matches!(
		c,
		' ' | '\t'..='\r'
			| '\u{a0}' | '\u{1680}'
			| '\u{2000}'..='\u{200a}'
			| '\u{2028}' | '\u{2029}'
			| '\u{202f}' | '\u{205f}'
			| '\u{3000}' | '\u{feff}'
	)
}

/// A valid element name: a doctype, a namespaced name, or a tag name as HTML spells one.
fn valid_name(name: &str) -> bool {
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
fn decode(raw: &str, attribute: bool) -> Cow<'_, str> {
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
			let len = rest.bytes().take_while(|b| b.is_ascii_alphanumeric()).count().min(32);
			let mut try_len = len;
			while try_len > 0 {
				let with = rest.as_bytes().get(try_len) == Some(&b';');
				let candidate = &rest[..try_len + usize::from(with)];
				if let Ok(found) = entities::ENTITIES.binary_search_by(|(name, _)| name.cmp(&candidate)) {
					let after = rest.as_bytes().get(try_len);
					if attribute
						&& !with && after.is_some_and(|b| *b == b'=' || b.is_ascii_alphanumeric() || *b == b'_')
					{
						try_len -= 1;
						continue;
					}
					code = Some(entities::ENTITIES[found].1);
					consumed = try_len + usize::from(with);
					break;
				}
				try_len -= 1;
			}
		}
		match code.filter(|&c| c != 0) {
			Some(code) => {
				out.push(char::from_u32(valid_code(code, attribute)).unwrap_or('\0'));
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

fn error(pos: u32, end: u32, code: Code, arg: Option<&str>) -> Box<SyntaxError> {
	let message: Cow<'static, str> = match arg {
		Some(arg) => code.with(arg).into(),
		None => code.message().into(),
	};
	Box::new(SyntaxError::with(pos, code, message).to(end))
}

fn fail<T>(pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<T> {
	Err(error(pos, end, code, arg))
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

/// Whether the document is TypeScript by its script tags, the way the grammar tells.
pub fn typescript(src: &str, grammar: &Grammar) -> bool {
	let Some(script) = &grammar.script else { return false };
	if script.typescript.is_empty() {
		return false;
	}
	let mut rest = src;
	while let Some(i) = rest.find('<') {
		rest = &rest[i + 1..];
		if let Some(after) = rest.strip_prefix("!--") {
			rest = after.find("-->").map_or("", |j| &after[j + 3..]);
			continue;
		}
		let Some(after) = rest.strip_prefix(script.name) else {
			continue;
		};
		if !after.starts_with(is_space) {
			continue;
		}
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
				attributes = &attributes[1..];
			}
			if script
				.typescript
				.iter()
				.any(|&(attribute, wanted)| name == attribute && wanted.is_none_or(|w| value == Some(w)))
			{
				return true;
			}
		}
		rest = &after[tag_end..];
	}
	false
}

/// What the walker is inside: the document, an element, or a block.
enum Frame<'a> {
	Root {
		nodes: Vec<NodeId>,
		instance: Option<NodeId>,
		module: Option<NodeId>,
		css: Option<NodeId>,
	},
	Element {
		start: u32,
		name: (u32, u32),
		ty: &'static str,
		attributes: Vec<NodeId>,
		fields: Vec<(&'static str, Value)>,
		nodes: Vec<NodeId>,
		shadowroot: bool,
		/// The element made its subtree verbatim.
		verbatim: bool,
		/// The patterns its directives declare in its scope.
		declared: Vec<NodeId>,
	},
	Block {
		start: u32,
		rule: &'a BlockRule,
		fields: Vec<(&'static str, Value)>,
		/// The open body: its field and whether it is left out of blocks that never opened it.
		body: (&'static str, bool),
		nodes: Vec<NodeId>,
		/// Bodies closed so far, each as its children.
		done: Vec<(&'static str, Value)>,
		/// The scope of each body read so far.
		groups: Vec<BodyGroup>,
		outside: Vec<NodeId>,
		/// The parent's field this block fills as a chained branch, `{:else if}`.
		chain: Option<&'static str>,
	},
}

/// What a form read: its fields, and the body its alternatives chose.
#[derive(Default)]
struct Read {
	fields: Vec<(&'static str, Value)>,
	body: Option<Body>,
}

/// A body's scope as its form read it: the body's field, the entry fields the scope holds, and
/// the patterns declared in it.
struct BodyGroup {
	body: &'static str,
	fields: Vec<&'static str>,
	inside: Vec<NodeId>,
}

/// What reading an attribute gives: its node, its type, and the kind and name it must not
/// repeat on the element, when it has such a name.
type Attribute = (NodeId, &'static str, Option<(&'static str, String)>);

/// An attribute name read as a directive: its rule, name, argument and modifiers.
struct Directive<'a> {
	rule: &'a DirectiveRule,
	name: &'a str,
	/// The argument's text and span, and whether it is an expression in brackets.
	arg: Option<(&'a str, u32, u32, bool)>,
	modifiers: Vec<&'a str>,
}

struct Walker<'a, E: Extension> {
	src: &'a str,
	full: u32,
	grammar: &'a Grammar,
	options: Options,
	ast: Option<Ast<E::Data>>,
	at: u32,
	/// Where the JavaScript read at the cursor must end: the source, or an attribute value.
	limit: u32,
	frames: Vec<Frame<'a>>,
	once: Vec<&'static str>,
	/// Where the current tag's word starts, for a declaration spelled without its keyword.
	keyword: u32,
	/// The element the browser closed last, what closed it and how deep the stack was then.
	autoclosed: Option<(&'a str, &'a str, usize)>,
	/// How many open elements made their subtree verbatim.
	verbatim: u32,
	/// The patterns the directives of the element being read declare.
	declared: Vec<NodeId>,
	/// Buffers earlier nodes gave back.
	spare_fields: Vec<Vec<(&'static str, Value)>>,
	spare_nodes: Vec<Vec<NodeId>>,
}

/// Parses a document by its grammar: the host's tree with the JavaScript inside it, positions
/// of the whole source. Returns the tree and its root.
pub(crate) fn parse_document<E: Extension>(
	src: &str,
	grammar: &Grammar,
	options: Options,
	reused: Option<Ast<E::Data>>,
) -> (Ast<E::Data>, Result<NodeId>) {
	let full = src.len() as u32;
	let cut = if grammar.trim {
		src.trim_end_matches(is_space)
	} else {
		src
	};
	let mut walker = Walker::<E> {
		src: cut,
		full,
		grammar,
		options,
		ast: Some(reused.unwrap_or_else(|| Ast::sized(src.len()))),
		at: 0,
		limit: cut.len() as u32,
		frames: vec![Frame::Root {
			nodes: Vec::new(),
			instance: None,
			module: None,
			css: None,
		}],
		once: Vec::new(),
		keyword: 0,
		autoclosed: None,
		verbatim: 0,
		declared: Vec::new(),
		spare_fields: Vec::new(),
		spare_nodes: Vec::new(),
	};
	let root = walker.run();
	(walker.ast.take().unwrap(), root)
}

impl<'a, E: Extension> Walker<'a, E> {
	fn ast(&mut self) -> &mut Ast<E::Data> {
		self.ast.as_mut().unwrap()
	}

	fn tree(&self) -> &Ast<E::Data> {
		self.ast.as_ref().unwrap()
	}

	fn len(&self) -> u32 {
		self.src.len() as u32
	}

	fn rest(&self) -> &'a str {
		&self.src[self.at as usize..]
	}

	fn byte(&self) -> Option<u8> {
		self.src.as_bytes().get(self.at as usize).copied()
	}

	fn char(&self) -> Option<char> {
		self.rest().chars().next()
	}

	fn matches(&self, s: &str) -> bool {
		self.rest().starts_with(s)
	}

	/// A word of the host's, followed by nothing that continues an identifier.
	fn word(&self, s: &str) -> bool {
		self.matches(s) && !self.rest()[s.len()..].starts_with(is_id_continue)
	}

	fn eat(&mut self, s: &str) -> bool {
		if self.matches(s) {
			self.at += s.len() as u32;
			true
		} else {
			false
		}
	}

	fn expect(&mut self, s: &str) -> Result<()> {
		if self.eat(s) {
			Ok(())
		} else {
			self.report(error(self.at, self.at, Code::Expected, Some(s)))
		}
	}

	fn recovering(&self) -> bool {
		self.options.error_recovery
	}

	/// Under recovery the error is recorded on the tree and reading goes on; otherwise it ends
	/// the parse.
	fn report(&mut self, error: Box<SyntaxError>) -> Result<()> {
		if self.recovering() {
			self.ast().errors.push(*error);
			Ok(())
		} else {
			Err(error)
		}
	}

	/// The delimiter that closes the tag being read, skipped to under recovery.
	fn skip_tag(&mut self) {
		let close = self.grammar.delimiters.1;
		if let Some(i) = self.rest().find(close) {
			self.at += (i + close.len()) as u32;
		} else {
			self.at = self.len();
		}
	}

	fn space(&mut self) {
		while let Some(c) = self.char() {
			if !is_space(c) {
				break;
			}
			self.at += c.len_utf8() as u32;
		}
	}

	fn require_space(&mut self) -> Result<()> {
		match self.char() {
			Some(c) if is_space(c) => {
				self.space();
				Ok(())
			}
			_ => fail(self.at, self.at, Code::Expected, Some("whitespace")),
		}
	}

	fn intern(&mut self, s: &str) -> StrId {
		self.ast().strings.intern(s)
	}

	/// A string field: a slice of the source where the text is the source's, interned otherwise.
	fn text(&mut self, start: u32, end: u32, value: Cow<str>) -> Value {
		match value {
			Cow::Borrowed(_) => Value::Slice(start, end),
			Cow::Owned(owned) => Value::Str(self.intern(&owned)),
		}
	}

	fn host(
		&mut self,
		ty: &'static str,
		start: u32,
		end: u32,
		fields: &[(&'static str, Value)],
		scope: Option<Opens>,
		span: bool,
	) -> NodeId {
		let ast = self.ast();
		let from = ast.host_fields.len() as u32;
		let len = fields.len() as u32;
		ast.host_fields.extend_from_slice(fields);
		let index = ast.hosts.len() as u32;
		ast.hosts.push(Host {
			ty,
			fields: (from, len),
			span,
			scope,
		});
		ast.add(NodeKind::Host(index), start, end)
	}

	fn text_chunk(&mut self, start: u32, end: u32, attribute: bool) -> NodeId {
		let raw = &self.src[start as usize..end as usize];
		let data = decode(raw, attribute);
		let data = self.text(start, end, data);
		let rule = &self.grammar.text;
		match rule.raw {
			Some(raw) => self.host(rule.ty, start, end, &[(raw, Value::Slice(start, end)), (rule.data, data)], None, true),
			None => self.host(rule.ty, start, end, &[(rule.data, data)], None, true),
		}
	}

	/// A list of nodes as the grammar holds one: wrapped in its fragment node, or bare.
	fn children(&mut self, nodes: Vec<NodeId>) -> Value {
		let list = self.list(&nodes);
		let value = match self.grammar.fragment {
			Some((ty, field)) => {
				// positions for the walks that order nodes, though none are written
				let (start, end) = match (nodes.first(), nodes.last()) {
					(Some(&first), Some(&last)) => (self.tree().node(first).start, self.tree().node(last).end),
					_ => (self.at, self.at),
				};
				Value::Node(self.host(ty, start, end, &[(field, Value::Nodes(list))], None, false))
			}
			None => Value::Nodes(list),
		};
		self.recycle_nodes(nodes);
		value
	}

	/// The scopes an element opens, over the fields about to make its node: one for what its
	/// directives declare, over its attributes and children, and one for its children when the
	/// grammar says every fragment is a scope.
	fn element_scope(&mut self, fields: &[(&'static str, Value)], declared: &[NodeId]) -> Option<Opens> {
		let names = &self.grammar.element_fields;
		let attributes = fields.iter().position(|(f, _)| *f == names.attributes);
		let children = fields.iter().position(|(f, _)| *f == names.children);
		let base = self.ast().host_fields.len() as u32;
		let at = self.ast().host_groups.len() as u32;
		let mut count = 0;
		if let (false, Some(attributes), Some(children)) = (declared.is_empty(), attributes, children) {
			let inside = self.list(declared);
			self.ast().host_groups.push(HostGroup {
				inside,
				from: base + attributes as u32,
				until: base + children as u32 + 1,
				node: None,
			});
			count += 1;
		}
		if let (true, Some(children)) = (self.grammar.fragment_scope, children) {
			let node = match fields[children].1 {
				Value::Node(fragment) => Some(fragment),
				_ => None,
			};
			self.ast().host_groups.push(HostGroup {
				inside: List::EMPTY,
				from: base + children as u32,
				until: base + children as u32 + 1,
				node,
			});
			count += 1;
		}
		(count > 0).then_some(Opens {
			outside: List::EMPTY,
			groups: (at, count),
		})
	}

	fn list(&mut self, nodes: &[NodeId]) -> List {
		self.ast().add_list_from(nodes.iter().map(|&id| Some(id)))
	}

	fn fields(&mut self) -> Vec<(&'static str, Value)> {
		self.spare_fields.pop().unwrap_or_default()
	}

	fn recycle_fields(&mut self, mut fields: Vec<(&'static str, Value)>) {
		fields.clear();
		self.spare_fields.push(fields);
	}

	fn nodes(&mut self) -> Vec<NodeId> {
		self.spare_nodes.pop().unwrap_or_default()
	}

	fn recycle_nodes(&mut self, mut nodes: Vec<NodeId>) {
		nodes.clear();
		self.spare_nodes.push(nodes);
	}

	fn append(&mut self, node: NodeId) {
		match self.frames.last_mut().unwrap() {
			Frame::Root { nodes, .. } | Frame::Element { nodes, .. } | Frame::Block { nodes, .. } => nodes.push(node),
		}
	}

	fn host_type(&self, id: NodeId) -> &'static str {
		let ast = self.tree();
		match ast.node(id).kind {
			NodeKind::Host(index) => ast.hosts[index as usize].ty,
			_ => "",
		}
	}

	fn field_of(&self, id: NodeId, key: &str) -> Option<Value> {
		let ast = self.tree();
		let NodeKind::Host(index) = ast.node(id).kind else {
			return None;
		};
		let host = ast.hosts[index as usize];
		ast.host_fields[host.fields.0 as usize..(host.fields.0 + host.fields.1) as usize]
			.iter()
			.find(|(k, _)| *k == key)
			.map(|&(_, v)| v)
	}

	fn string_of(&self, value: Value) -> Option<&str> {
		match value {
			Value::Str(s) => Some(self.tree().str(s)),
			Value::Slice(a, b) => Some(&self.src[a as usize..b as usize]),
			_ => None,
		}
	}

	fn attribute_named(&self, id: NodeId, name: &str) -> bool {
		self.host_type(id) == "Attribute" && self.field_of(id, "name").and_then(|v| self.string_of(v)) == Some(name)
	}

	/// The one chunk of an attribute's value, when it has one.
	fn attribute_chunk(&self, id: NodeId) -> Option<NodeId> {
		match self.field_of(id, "value")? {
			Value::Node(tag) => Some(tag),
			Value::Nodes(list) => match self.tree().list(list) {
				[Some(chunk)] => Some(*chunk),
				_ => None,
			},
			_ => None,
		}
	}

	fn expression_type(&self) -> &'static str {
		self.grammar.expression.as_ref().map_or("Expression", |rule| rule.ty)
	}

	/// The expression node of a chunk written as one, `{expression}`.
	fn chunk_expression(&self, chunk: NodeId) -> Option<NodeId> {
		if self.host_type(chunk) != self.expression_type() {
			return None;
		}
		let rule = self.grammar.expression.as_ref()?;
		let Some(Item::Entry { field, .. }) = rule.form.items.first() else {
			return None;
		};
		match self.field_of(chunk, field)? {
			Value::Node(expression) => Some(expression),
			_ => None,
		}
	}

	/// The expression of `name={expression}`, quoted or not.
	fn attribute_expression(&self, id: NodeId) -> Option<NodeId> {
		self.chunk_expression(self.attribute_chunk(id)?)
	}

	/// The text of `name="text"`.
	fn attribute_text(&self, id: NodeId) -> Option<&str> {
		let Value::Nodes(_) = self.field_of(id, "value")? else {
			return None;
		};
		let chunk = self.attribute_chunk(id)?;
		if self.host_type(chunk) != self.grammar.text.ty {
			return None;
		}
		self.string_of(self.field_of(chunk, self.grammar.text.data)?)
	}

	/// An expression chunk, `{expression}`, as the grammar's expression node.
	fn expression_tag(&mut self, start: u32, end: u32, expression: NodeId) -> Result<NodeId> {
		let Some(rule) = &self.grammar.expression else {
			return fail(start, end, Code::UnexpectedToken, None);
		};
		let field = match rule.form.items.first() {
			Some(Item::Entry { field, .. }) => field,
			_ => "expression",
		};
		Ok(self.host(rule.ty, start, end, &[(field, Value::Node(expression))], None, true))
	}

	/// Whether a `<` at `i` starts a tag: a name, a closing tag or a comment follows.
	fn tag_start(&self, i: usize) -> bool {
		let bytes = self.src.as_bytes();
		bytes[i] == b'<' && !matches!(bytes.get(i + 1), Some(b) if !b.is_ascii_alphabetic() && *b != b'/' && *b != b'!')
	}

	/// Skips whitespace up to `limit`.
	fn space_to(&mut self, limit: u32) {
		while self.at < limit {
			match self.char() {
				Some(c) if is_space(c) => self.at += c.len_utf8() as u32,
				_ => break,
			}
		}
	}

	fn run(&mut self) -> Result<NodeId> {
		let open = self.grammar.delimiters.0;
		while self.at < self.len() {
			let before = self.at;
			let result = if self.byte() == Some(b'<') && self.tag_start(self.at as usize) {
				self.element()
			} else if self.verbatim == 0 && self.matches(open) {
				self.tag()
			} else {
				self.text_node();
				Ok(())
			};
			if let Err(error) = result {
				self.report(error)?;
				// whatever could not be read, something is
				if self.at == before {
					self.at += self.char().map_or(1, |c| c.len_utf8() as u32);
				}
			}
		}
		while self.frames.len() > 1 {
			let (start, what) = match self.frames.last().unwrap() {
				Frame::Element { start, name, .. } => (*start, &self.src[name.0 as usize..name.1 as usize]),
				Frame::Block { start, rule, .. } => (*start, rule.name),
				Frame::Root { .. } => unreachable!(),
			};
			self.report(error(start, start + 1, Code::Unclosed, Some(what)))?;
			// under recovery, what is open ends with the source
			let end = self.len();
			match self.frames.last().unwrap() {
				Frame::Element { .. } => self.close_top(end),
				_ => {
					self.pop_block(end);
				}
			}
		}
		let errors = &mut self.ast().errors;
		errors.sort_by_key(|error| error.pos);
		errors.dedup_by(|a, b| a.pos == b.pos && a.code == b.code);
		let Some(Frame::Root {
			nodes,
			instance,
			module,
			css,
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		let children = self.children(nodes);
		let mut fields = self.fields();
		// (from, until, node) of each scope the document line opens, over the fields it holds
		let mut scopes: Vec<(usize, usize, Option<NodeId>)> = Vec::new();
		fn place<E: Extension>(
			w: &mut Walker<E>,
			items: &[DocField],
			fields: &mut Vec<(&'static str, Value)>,
			scopes: &mut Vec<(usize, usize, Option<NodeId>)>,
			held: (Value, Option<NodeId>, Option<NodeId>, Option<NodeId>),
		) {
			let (children, instance, module, css) = held;
			for item in items {
				match item {
					DocField::Scope(inner) => {
						let from = fields.len();
						place(w, inner, fields, scopes, held);
						let until = fields.len();
						if until == from {
							continue;
						}
						// the scope belongs to the fragment it starts with, or to a script's program
						let node = match fields[from].1 {
							Value::Node(id) if matches!(children, Value::Node(fragment) if fragment == id) => Some(id),
							Value::Node(id) if w.host_type(id) == "Script" => match w.field_of(id, "content") {
								Some(Value::Node(program)) => Some(program),
								_ => None,
							},
							_ => None,
						};
						scopes.push((from, until, node));
					}
					&DocField::Field(field, holds, omit) => {
						let value = match holds {
							RootField::Fragment => {
								fields.push((field, children));
								continue;
							}
							RootField::Script { module: false } => instance,
							RootField::Script { module: true } => module,
							RootField::Style => css,
							RootField::Comments => {
								fields.push((field, Value::Comments));
								continue;
							}
							RootField::EmptyList => {
								fields.push((field, Value::Nodes(List::EMPTY)));
								continue;
							}
							RootField::Null => {
								fields.push((field, Value::Null));
								continue;
							}
						};
						match value {
							Some(node) => fields.push((field, Value::Node(node))),
							None if !omit => fields.push((field, Value::Null)),
							None => {}
						}
					}
				}
			}
		}
		let document = self.grammar.document.fields.clone();
		place(
			self,
			&document,
			&mut fields,
			&mut scopes,
			(children, instance, module, css),
		);
		let full = self.full;
		let base = self.ast().host_fields.len() as u32;
		let at = self.ast().host_groups.len() as u32;
		for &(from, until, node) in &scopes {
			self.ast().host_groups.push(HostGroup {
				inside: List::EMPTY,
				from: base + from as u32,
				until: base + until as u32,
				node,
			});
		}
		let scope = (!scopes.is_empty()).then_some(Opens {
			outside: List::EMPTY,
			groups: (at, scopes.len() as u32),
		});
		let node = self.host(self.grammar.document.ty, 0, full, &fields, scope, true);
		self.recycle_fields(fields);
		Ok(node)
	}

	fn text_node(&mut self) {
		let start = self.at;
		let bytes = self.src.as_bytes();
		let open = self.grammar.delimiters.0.as_bytes();
		// the first character is text whatever it is
		let mut i = self.at as usize + self.char().map_or(1, char::len_utf8);
		while i < bytes.len() {
			if (bytes[i] == b'<' && self.tag_start(i)) || (self.verbatim == 0 && bytes[i..].starts_with(open)) {
				break;
			}
			i += 1;
		}
		self.at = i as u32;
		let node = self.text_chunk(start, self.at, false);
		self.append(node);
	}

	/// Whether the nearest element around the cursor, past blocks and meta elements, is `name`.
	fn nearest_element_is(&self, name: &str) -> bool {
		let plain = self.grammar.element("*").map(|any| any.ty);
		let component = self
			.grammar
			.elements
			.iter()
			.find(|rule| rule.name == Match::Component)
			.map(|rule| rule.ty);
		for frame in self.frames.iter().rev() {
			if let Frame::Element { name: span, ty, .. } = frame {
				if &self.src[span.0 as usize..span.1 as usize] == name {
					return true;
				}
				if Some(*ty) == plain || Some(*ty) == component {
					return false;
				}
			}
		}
		false
	}

	fn element(&mut self) -> Result<()> {
		let start = self.at;
		self.at += 1;
		if self.eat("!--") {
			let Some(len) = self.rest().find("-->") else {
				return fail(self.len(), self.len(), Code::Expected, Some("-->"));
			};
			let data_start = self.at;
			self.at += len as u32 + 3;
			let rule = &self.grammar.comment;
			let node = self.host(
				rule.ty,
				start,
				self.at,
				&[(rule.data, Value::Slice(data_start, self.at - 3))],
				None,
				true,
			);
			self.append(node);
			return Ok(());
		}
		if self.eat("/") {
			let name = self.tag_name(false)?;
			self.space();
			self.expect(">")?;
			if self.grammar.is_void(name) {
				return self.report(error(
					start,
					start + 1,
					Code::Placement,
					Some("A closing tag of a void element"),
				));
			}
			return self.close_element(start, name);
		}
		let name = self.tag_name(false)?;
		let name_span = (start + 1, self.at);
		let rule = self.grammar.element(name);
		let namespaced = name
			.split_once(':')
			.is_some_and(|(prefix, _)| prefix == self.grammar.name);
		let rule = rule.filter(|rule| rule.name != Match::Any || (!namespaced && valid_name(name)));
		let typing = self.recovering() && name.ends_with('.') && component_name(&format!("{name}_"));
		let Some(rule) = rule.or_else(|| typing.then(|| self.grammar.component()).flatten()) else {
			return fail(name_span.0, name_span.1, Code::InvalidName, Some(name));
		};
		if rule.root && self.frames.len() > 1 {
			self.report(error(start, start + 1, Code::Placement, Some(name)))?;
		}
		if rule.once {
			if self.once.contains(&rule.ty) {
				self.report(error(start, start + 1, Code::Duplicate, Some(name)))?;
			} else {
				self.once.push(rule.ty);
			}
		}
		let plain = self.grammar.element("*").map_or(rule.ty, |any| any.ty);
		let mut ty = rule.ty;
		if let Some(inside) = rule.inside
			&& !self.nearest_element_is(inside)
		{
			ty = plain;
		}
		if rule.outside.is_some()
			&& self
				.frames
				.iter()
				.any(|frame| matches!(frame, Frame::Element { shadowroot: true, .. }))
		{
			ty = plain;
		}
		self.space();
		// the browser closes the open element when this one cannot sit inside it
		if self.grammar.autoclose
			&& let Some(Frame::Element {
				name: parent,
				ty: parent_ty,
				..
			}) = self.frames.last()
		{
			let src = self.src;
			let parent_name = &src[parent.0 as usize..parent.1 as usize];
			if *parent_ty == plain && closes(parent_name, name) {
				self.close_top(start);
				self.autoclosed = Some((parent_name, name, self.frames.len()));
			}
		}
		let at_root = self.frames.len() == 1;
		let script = self.grammar.script.as_ref().filter(|s| s.name == name && at_root);
		let style = self.grammar.style.filter(|s| *s == name && at_root);
		let attributes_at = self.at;
		let verbatim_before = self.verbatim;
		let mut attributes = self.nodes();
		let mut seen: Vec<(&'static str, String)> = Vec::new();
		let mut shadowroot = false;
		self.declared.clear();
		loop {
			let attribute = if script.is_some() || style.is_some() || self.verbatim > 0 {
				self.static_attribute()?
			} else {
				self.attribute()?
			};
			let Some((node, kind, key)) = attribute else { break };
			if let Some((kind, key)) = key {
				if self.verbatim == verbatim_before && Some(key.as_str()) == self.grammar.verbatim {
					// what came before is read again as plain attributes
					self.verbatim += 1;
					self.at = attributes_at;
					attributes.clear();
					seen.clear();
					shadowroot = false;
					self.declared.clear();
					continue;
				}
				if kind == "Attribute" && key == "shadowrootmode" && ty == plain {
					shadowroot = true;
				}
				if seen.iter().any(|(k, n)| *k == kind && *n == key) {
					let (at, end) = (self.tree().node(node).start, self.tree().node(node).end);
					self.report(error(at, end, Code::Duplicate, Some(&key)))?;
				}
				if key != "this" {
					seen.push((kind, key));
				}
			}
			let _ = kind;
			attributes.push(node);
			self.space();
		}
		let verbatim_here = self.verbatim > verbatim_before;
		let declared = std::mem::take(&mut self.declared);
		let mut fields = self.fields();
		if let Some((field, text)) = rule.this {
			let position = attributes.iter().position(|&id| self.attribute_named(id, "this"));
			let value = match position {
				None => {
					self.report(error(start, start + 1, Code::Expected, Some("a this attribute")))?;
					self.placeholder(name_span.1, name_span.1)
				}
				Some(position) => {
					let this = attributes.remove(position);
					match self.attribute_expression(this) {
						Some(expression) => expression,
						None => {
							let chunk = self
								.attribute_chunk(this)
								.filter(|&chunk| text && self.host_type(chunk) == self.grammar.text.ty);
							match chunk {
								Some(chunk) => {
									let node = *self.tree().node(chunk);
									let value = match self.field_of(chunk, self.grammar.text.data) {
										Some(Value::Str(s)) => s,
										Some(Value::Slice(a, b)) => self.intern(&self.src[a as usize..b as usize]),
										_ => unreachable!(),
									};
									self.ast().add(NodeKind::StringLiteral { value }, node.start, node.end)
								}
								None => {
									let node = *self.tree().node(this);
									self.report(error(
										node.start,
										node.end,
										Code::Expected,
										Some("an expression as this"),
									))?;
									self.placeholder(node.start, node.end)
								}
							}
						}
					}
				}
			};
			fields.push((field, Value::Node(value)));
		}
		if let Some(script) = script {
			self.expect(">")?;
			let content_start = self.at;
			let close = match self.find_closing(name) {
				Some(close) => close,
				None => {
					self.report(error(self.len(), self.len(), Code::Unclosed, Some(name)))?;
					self.len()
				}
			};
			let program = self.program(content_start, close)?;
			self.at = close;
			if close < self.len() {
				self.close_tag(name)?;
			}
			let mut module = false;
			for &id in &attributes {
				for &(attribute, value) in &script.module {
					if !self.attribute_named(id, attribute) {
						continue;
					}
					match value {
						Some(value) if self.attribute_text(id) != Some(value) => {
							let node = *self.tree().node(id);
							self.report(error(
								node.start,
								node.end,
								Code::Expected,
								Some(&format!("{attribute} to be \"{value}\"")),
							))?;
						}
						None if !matches!(self.field_of(id, "value"), Some(Value::Bool(true))) => {
							let node = *self.tree().node(id);
							self.report(error(
								node.start,
								node.end,
								Code::Expected,
								Some(&format!("{attribute} without a value")),
							))?;
						}
						_ => module = true,
					}
				}
			}
			let context = self.intern(if module { "module" } else { "default" });
			let attributes = self.list(&attributes);
			let node = self.host(
				"Script",
				start,
				self.at,
				&[
					("context", Value::Str(context)),
					("content", Value::Node(program)),
					("attributes", Value::Nodes(attributes)),
				],
				None,
				true,
			);
			let Some(Frame::Root {
				instance,
				module: module_slot,
				..
			}) = self.frames.first_mut()
			else {
				unreachable!()
			};
			let slot = if module { module_slot } else { instance };
			if slot.is_none() {
				*slot = Some(node);
				return Ok(());
			}
			return self.report(error(start, start + 1, Code::Duplicate, Some(name)));
		}
		if style.is_some() {
			self.expect(">")?;
			let node = self.style_sheet(start, name, attributes)?;
			let Some(Frame::Root { css, .. }) = self.frames.first_mut() else {
				unreachable!()
			};
			if css.is_none() {
				*css = Some(node);
				return Ok(());
			}
			return self.report(error(start, start + 1, Code::Duplicate, Some(name)));
		}
		let self_closing = self.eat("/") || self.grammar.is_void(name);
		let unclosed = !self.eat(">");
		if unclosed {
			self.report(error(self.at, self.at, Code::Expected, Some(">")))?;
		}
		let name_id = self.intern(name);
		let names = &self.grammar.element_fields;
		fields.insert(0, (names.name, Value::Str(name_id)));
		let finish = |w: &mut Self, attributes: Vec<NodeId>, mut fields: Vec<(&'static str, Value)>, nodes: Vec<NodeId>, end: u32| {
			let list = w.list(&attributes);
			w.recycle_nodes(attributes);
			fields.push((names.attributes, Value::Nodes(list)));
			let children = w.children(nodes);
			fields.push((names.children, children));
			let scope = w.element_scope(&fields, &declared);
			let node = w.host(ty, start, end, &fields, scope, true);
			w.recycle_fields(fields);
			w.append(node);
			if verbatim_here {
				w.verbatim -= 1;
			}
		};
		if self_closing || unclosed {
			let end = self.at;
			let nodes = self.nodes();
			finish(self, attributes, fields, nodes, end);
			return Ok(());
		}
		if rule.rcdata {
			let nodes = self.sequence(
				|w| closing_tag(w.rest(), name).is_some(),
				&format!("<{name}>"),
				JsEntry::Expression,
			)?;
			if let Some(len) = closing_tag(self.rest(), name) {
				self.at += len as u32;
			}
			let end = self.at;
			finish(self, attributes, fields, nodes, end);
			return Ok(());
		}
		if rule.raw {
			let content_start = self.at;
			let mut close = self.at;
			loop {
				let Some(i) = self.src[close as usize..].find("</") else {
					close = self.len();
					break;
				};
				close += i as u32;
				if closing_tag(&self.src[close as usize..], name).is_some() {
					break;
				}
				close += 2;
			}
			self.at = close;
			let rule = &self.grammar.text;
			let mut text_fields = self.fields();
			if let Some(raw) = rule.raw {
				text_fields.push((raw, Value::Slice(content_start, close)));
			}
			text_fields.push((rule.data, Value::Slice(content_start, close)));
			let node = self.host(rule.ty, content_start, close, &text_fields, None, true);
			self.recycle_fields(text_fields);
			match closing_tag(self.rest(), name) {
				Some(len) => self.at += len as u32,
				None => self.report(error(self.len(), self.len(), Code::Unclosed, Some(name)))?,
			}
			let end = self.at;
			let mut nodes = self.nodes();
			nodes.push(node);
			finish(self, attributes, fields, nodes, end);
			return Ok(());
		}
		let nodes = self.nodes();
		self.frames.push(Frame::Element {
			start,
			name: name_span,
			ty,
			attributes,
			fields,
			nodes,
			shadowroot,
			verbatim: verbatim_here,
			declared,
		});
		Ok(())
	}

	/// `</name`, optional space and `>`, from the cursor.
	fn find_closing(&self, name: &str) -> Option<u32> {
		let rest = self.rest();
		let mut from = 0;
		while let Some(i) = rest[from..].find("</") {
			let at = from + i + 2;
			if let Some(after) = rest[at..].strip_prefix(name)
				&& after.trim_start_matches(is_space).starts_with('>')
			{
				return Some(self.at + (from + i) as u32);
			}
			from = at;
		}
		None
	}

	fn close_tag(&mut self, name: &str) -> Result<()> {
		self.expect("</")?;
		self.expect(name)?;
		self.space();
		self.expect(">")
	}

	fn tag_name(&mut self, attribute: bool) -> Result<&'a str> {
		let start = self.at;
		if start >= self.len() {
			if attribute && self.recovering() {
				return Ok("");
			}
			return fail(self.len(), self.len(), Code::UnexpectedEof, None);
		}
		while let Some(c) = self.char() {
			if is_space(c) || c == '/' || c == '>' || (attribute && matches!(c, '"' | '\'' | '=')) {
				break;
			}
			self.at += c.len_utf8() as u32;
		}
		Ok(&self.src[start as usize..self.at as usize])
	}

	/// Closes the element on top of the stack at `end`.
	fn close_top(&mut self, end: u32) {
		let Some(Frame::Element {
			start,
			ty,
			attributes,
			mut fields,
			nodes,
			verbatim,
			declared,
			..
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		let names = &self.grammar.element_fields;
		let list = self.list(&attributes);
		self.recycle_nodes(attributes);
		fields.push((names.attributes, Value::Nodes(list)));
		let children = self.children(nodes);
		fields.push((names.children, children));
		let scope = self.element_scope(&fields, &declared);
		let node = self.host(ty, start, end, &fields, scope, true);
		self.recycle_fields(fields);
		self.append(node);
		if verbatim {
			self.verbatim -= 1;
		}
	}

	fn close_element(&mut self, start: u32, name: &str) -> Result<()> {
		let plain = self.grammar.element("*").map(|any| any.ty);
		if let Some((_, _, depth)) = self.autoclosed
			&& self.frames.len() < depth
		{
			self.autoclosed = None;
		}
		let autoclosed = self.autoclosed;
		let closed = move || match autoclosed {
			Some((closed, by, _)) if closed == name => format!("{name}, closed by {by}"),
			_ => name.to_string(),
		};
		// under recovery a closing tag that closes nothing is skipped, and one that closes an
		// element further out closes what is open inside it
		let opens = |w: &Self| {
			w.frames.iter().any(
				|frame| matches!(frame, Frame::Element { name: span, .. } if &w.src[span.0 as usize..span.1 as usize] == name),
			)
		};
		if self.recovering() && !opens(self) {
			return self.report(error(start, start + 1, Code::UnexpectedClose, Some(&closed())));
		}
		loop {
			match self.frames.last() {
				Some(Frame::Element { name: span, ty, .. }) => {
					let open = &self.src[span.0 as usize..span.1 as usize];
					if open == name {
						let end = self.at;
						self.close_top(end);
						return Ok(());
					}
					if Some(*ty) != plain {
						self.report(error(start, start + 1, Code::UnexpectedClose, Some(name)))?;
					}
					// the browser closes it here
					self.close_top(start);
				}
				Some(Frame::Block {
					start: block_start,
					rule,
					chain,
					..
				}) if self.recovering() => {
					let (block_start, what, chained) = (*block_start, rule.name, chain.is_some());
					if !chained {
						self.report(error(block_start, block_start + 1, Code::Unclosed, Some(what)))?;
					}
					self.pop_block(start);
				}
				_ => return fail(start, start + 1, Code::UnexpectedClose, Some(&closed())),
			}
		}
	}

	/// A quoted or bare text value after `=`, as one text chunk.
	fn text_value(&mut self) -> Result<Value> {
		let value_start = self.at;
		let quote = self.char().filter(|c| matches!(c, '"' | '\''));
		let (raw_start, raw_end) = match quote {
			Some(q) => {
				self.at += 1;
				let Some(len) = self.rest().find(q) else {
					return fail(value_start, value_start, Code::Expected, Some("an attribute value"));
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
					return fail(value_start, value_start, Code::Expected, Some("an attribute value"));
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
	fn static_attribute(&mut self) -> Result<Option<Attribute>> {
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
			return fail(self.at, self.at, Code::Expected, Some("="));
		}
		let name_id = self.intern(name);
		let node = self.host(
			"Attribute",
			start,
			self.at,
			&[("name", Value::Str(name_id)), ("value", value)],
			None,
			true,
		);
		Ok(Some((node, "Attribute", Some(("Attribute", name.to_string())))))
	}

	fn comment_between_attributes(&mut self) -> bool {
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
		self.ast().comments.push(Comment { kind, start, end });
		true
	}

	/// The attribute name at `start` read as a directive, when the grammar's syntax says it is one.
	fn directive_of(&self, name: &'a str, start: u32) -> Result<Option<Directive<'a>>> {
		let Some(syntax) = &self.grammar.directive_syntax else {
			return Ok(None);
		};
		let mut base_modifiers: &[&'static str] = &[];
		let (directive, rest, rest_at): (&'a str, &'a str, usize) = if let Some(shorthand) = self
			.grammar
			.shorthands
			.iter()
			.find(|s| name.starts_with(s.token) && syntax.prefix.is_none_or(|p| !name.starts_with(p)))
		{
			base_modifiers = &shorthand.modifiers;
			(shorthand.name, &name[shorthand.token.len()..], shorthand.token.len())
		} else if let Some(prefix) = syntax.prefix {
			let Some(after) = name.strip_prefix(prefix) else {
				return Ok(None);
			};
			let len = after
				.find(|c: char| syntax.arg.starts_with(c) || syntax.modifier.starts_with(c))
				.unwrap_or(after.len());
			let rest = after[len..].strip_prefix(syntax.arg).unwrap_or(&after[len..]);
			(&after[..len], rest, name.len() - rest.len())
		} else {
			let Some((head, tail)) = name.split_once(syntax.arg) else {
				return Ok(None);
			};
			if self.grammar.directive(head).is_none() {
				return Ok(None);
			}
			(head, tail, head.len() + syntax.arg.len())
		};
		if directive.is_empty() {
			return fail(
				start,
				start + name.len() as u32,
				Code::Expected,
				Some("a directive name"),
			);
		}
		let Some(rule) = self.grammar.directive(directive) else {
			return Ok(None);
		};
		// the argument, in brackets when it is an expression, then the modifiers
		let mut modifiers = Vec::new();
		let mut arg = None;
		let after = if let Some((open, close)) = syntax.dynamic
			&& let Some(inner) = rest.strip_prefix(open)
		{
			let Some(len) = inner.find(close) else {
				return fail(start, start + name.len() as u32, Code::Expected, Some(close));
			};
			let inner_at = rest_at + open.len();
			arg = Some((&inner[..len], inner_at, inner_at + len, true));
			&inner[len + close.len()..]
		} else {
			let len = rest.find(syntax.modifier).unwrap_or(rest.len());
			if len > 0 {
				arg = Some((&rest[..len], rest_at, rest_at + len, false));
			} else if syntax.prefix.is_none() {
				return fail(
					start,
					start + name.len() as u32,
					Code::Expected,
					Some("a directive name"),
				);
			}
			&rest[len..]
		};
		for modifier in after.split(syntax.modifier) {
			if !modifier.is_empty() {
				modifiers.push(modifier);
			}
		}
		let mut all: Vec<&'a str> = base_modifiers.to_vec();
		all.extend(modifiers);
		Ok(Some(Directive {
			rule,
			name: directive,
			arg: arg.map(|(text, s, e, dynamic)| (text, start + s as u32, start + e as u32, dynamic)),
			modifiers: all,
		}))
	}

	/// One attribute: a plain one, a shorthand, a spread, an attachment or a directive; the node,
	/// its type, and the key it must not repeat.
	fn attribute(&mut self) -> Result<Option<Attribute>> {
		let expressions = self.grammar.attribute_expressions;
		if expressions {
			while self.comment_between_attributes() {
				self.space();
			}
		}
		let start = self.at;
		if expressions && self.eat("{") {
			self.space();
			if self.matches("/>") || self.matches(">") {
				return fail(self.at, self.at, Code::Expected, Some("}"));
			}
			if let Some(sigils) = &self.grammar.sigils
				&& self.eat(sigils.tag)
			{
				let (node, rule) = self.tag_node(start)?;
				if !rule.attribute {
					return fail(
						start,
						self.at,
						Code::Placement,
						Some(&format!("A {} tag among attributes", rule.name)),
					);
				}
				return Ok(Some((node, rule.ty, None)));
			}
			if self.eat("...") {
				let Some(ty) = self.grammar.spread else {
					return fail(start, start + 1, Code::UnexpectedToken, None);
				};
				let expression = self.expression("")?;
				self.space();
				self.expect("}")?;
				let node = self.host(
					ty,
					start,
					self.at,
					&[("expression", Value::Node(expression))],
					None,
					true,
				);
				return Ok(Some((node, ty, None)));
			}
			if self.recovering()
				&& let Some(sigils) = &self.grammar.sigils
				&& [sigils.open, sigils.branch, sigils.close, sigils.tag]
					.iter()
					.any(|s| self.matches(s))
			{
				self.at = start;
				return Ok(None);
			}
			if !self.grammar.attribute_shorthand {
				return fail(start, start + 1, Code::UnexpectedToken, None);
			}
			let id_start = self.at;
			let id = self.identifier()?;
			let id_end = self.at;
			let name = self.src[id_start as usize..id_end as usize].to_string();
			if reserved(&name) {
				return fail(id_start, id_end, Code::ReservedWord, Some(&name));
			}
			self.space();
			self.expect("}")?;
			let tag = self.expression_tag(id_start, id_end, id)?;
			let name_id = self.intern(&name);
			let node = self.host(
				"Attribute",
				start,
				self.at,
				&[("name", Value::Str(name_id)), ("value", Value::Node(tag))],
				None,
				true,
			);
			return Ok(Some((node, "Attribute", Some(("Attribute", name)))));
		}
		let name = self.tag_name(true)?;
		if name.is_empty() || (self.recovering() && name.starts_with('<')) {
			self.at = start;
			return Ok(None);
		}
		let name_end = self.at;
		let directive = self.directive_of(name, start)?;
		let mut end = name_end;
		self.space();
		let has_value = self.eat("=");
		if !has_value && self.char().is_some_and(|c| c == '"' || c == '\'') {
			return fail(self.at, self.at, Code::Expected, Some("="));
		}
		if has_value {
			self.space();
		}
		let Some(directive) = directive else {
			let value = if has_value {
				self.plain_value()?
			} else {
				Value::Bool(true)
			};
			if has_value {
				end = self.at;
			}
			let name_id = self.intern(name);
			let node = self.host(
				"Attribute",
				start,
				end,
				&[("name", Value::Str(name_id)), ("value", value)],
				None,
				true,
			);
			return Ok(Some((node, "Attribute", Some(("Attribute", name.to_string())))));
		};
		let syntax = self.grammar.directive_syntax.as_ref().unwrap();
		let rule = directive.rule;
		let mut fields = self.fields();
		if let Some(field) = syntax.name_field {
			let id = self.intern(directive.name);
			fields.push((field, Value::Str(id)));
		}
		if let Some(field) = syntax.raw_field {
			fields.push((field, Value::Slice(start, name_end)));
		}
		if let Some(field) = syntax.arg_field {
			let value = match directive.arg {
				Some((_, arg_start, arg_end, true)) => {
					let (at, limit) = (self.at, self.limit);
					self.at = arg_start;
					self.limit = arg_end;
					let expression = self.expression("");
					self.at = at;
					self.limit = limit;
					Value::Node(expression?)
				}
				Some((text, _, _, false)) => Value::Str(self.intern(text)),
				None => Value::Null,
			};
			fields.push((field, value));
		}
		let declares = rule.declares.as_deref();
		match &rule.value {
			DirectiveValue::Value => {
				let value = if has_value {
					self.plain_value()?
				} else {
					Value::Bool(true)
				};
				if has_value {
					end = self.at;
				}
				fields.push(("value", value));
			}
			DirectiveValue::Expression {
				optional,
				name: own_name,
			}
			| DirectiveValue::Pattern {
				optional,
				name: own_name,
			} => {
				let entry = if matches!(rule.value, DirectiveValue::Pattern { .. }) {
					JsEntry::Pattern
				} else {
					JsEntry::Expression
				};
				let value = if has_value {
					self.plain_value_as(entry)?
				} else {
					Value::Bool(true)
				};
				if has_value {
					end = self.at;
				}
				let expression = match value {
					Value::Bool(true) => None,
					Value::Node(tag) => self.chunk_expression(tag),
					Value::Nodes(list) => {
						let items = self.tree().list(list).to_vec();
						match items[..] {
							[Some(chunk)] if self.chunk_expression(chunk).is_some() => self.chunk_expression(chunk),
							[Some(chunk), ..] => {
								let node = self.tree().node(chunk);
								return fail(node.start, node.end, Code::Expected, Some("an expression, not text"));
							}
							_ => None,
						}
					}
					_ => None,
				};
				let expression = match expression {
					Some(e) => Value::Node(e),
					None if *own_name => match directive.arg {
						Some((text, arg_start, _, _)) => {
							let id = self.intern(text);
							Value::Node(self.ast().add(NodeKind::Identifier { name: id }, arg_start, end))
						}
						None => return fail(start, end, Code::Expected, Some("a value")),
					},
					None if *optional => Value::Null,
					None => return fail(start, end, Code::Expected, Some("a value")),
				};
				if let (Some([]), Value::Node(pattern)) = (declares, expression) {
					self.declared.push(pattern);
				}
				fields.push(("expression", expression));
			}
			DirectiveValue::Form(form) => {
				let mut read = Read {
			fields: self.fields(),
			body: None,
		};
				if has_value {
					let (value_start, value_end, after) = self.value_range()?;
					if value_end > value_start {
						let limit = self.limit;
						self.at = value_start;
						self.limit = value_end;
						let result = self.form(form, &mut read);
						self.limit = limit;
						result?;
						self.space_to(value_end);
						if self.at != value_end {
							return fail(
								self.at,
								value_end,
								Code::Expected,
								Some("the end of the attribute value"),
							);
						}
					}
					self.at = after;
					end = after;
				}
				let mut entries = Vec::new();
				collect_entries(&form.items, &mut entries);
				for (field, omit) in entries {
					if !omit && !read.fields.iter().any(|(k, _)| *k == field) {
						read.fields.push((field, Value::Null));
					}
				}
				if let Some(names) = declares {
					for &(field, value) in &read.fields {
						if !names.contains(&field) {
							continue;
						}
						match value {
							Value::Node(pattern) => self.declared.push(pattern),
							Value::Nodes(list) => {
								let patterns: Vec<NodeId> = self.tree().list(list).iter().flatten().copied().collect();
								self.declared.extend(patterns);
							}
							_ => {}
						}
					}
				}
				fields.extend(read.fields);
			}
		}
		if let Some(field) = syntax.modifiers_field {
			let from = self.ast().host_strings.len() as u32;
			let count = directive.modifiers.len() as u32;
			for modifier in &directive.modifiers {
				let id = self.intern(modifier);
				self.ast().host_strings.push(id);
			}
			fields.push((field, Value::Strs(from, count)));
		}
		for &(flag, on) in &rule.flags {
			fields.push((flag, Value::Bool(on)));
		}
		let node = self.host(rule.ty, start, end, &fields, None, true);
		self.recycle_fields(fields);
		let key = if syntax.unique {
			Some(("Attribute", name.to_string()))
		} else {
			match (rule.unique, directive.arg) {
				(Unique::Kind, Some((text, ..))) => Some((rule.ty, text.to_string())),
				(Unique::Attribute, Some((text, ..))) => Some(("Attribute", text.to_string())),
				_ => None,
			}
		};
		Ok(Some((node, rule.ty, key)))
	}

	/// An attribute value as the grammar reads one: text with expressions, or text.
	fn plain_value(&mut self) -> Result<Value> {
		self.plain_value_as(JsEntry::Expression)
	}

	/// The same, its expressions read as `entry`.
	fn plain_value_as(&mut self, entry: JsEntry) -> Result<Value> {
		if !self.grammar.attribute_expressions {
			return self.text_value();
		}
		if self.matches("/>") {
			// `<a href=/>`: the slash is the value
			let slash = self.at;
			self.at += 1;
			let text = self.text_chunk(slash, slash + 1, true);
			let list = self.list(&[text]);
			return Ok(Value::Nodes(list));
		}
		self.attribute_value(entry)
	}

	/// The span of the attribute value at the cursor, quoted or bare, and where the cursor goes
	/// after it.
	fn value_range(&mut self) -> Result<(u32, u32, u32)> {
		let at = self.at;
		match self.char() {
			Some(q @ ('"' | '\'')) => {
				let Some(len) = self.rest()[1..].find(q) else {
					return fail(at, at, Code::Expected, Some("an attribute value"));
				};
				Ok((at + 1, at + 1 + len as u32, at + 2 + len as u32))
			}
			_ => {
				let len = self
					.rest()
					.find(|c: char| c == '>' || is_space(c))
					.unwrap_or(self.rest().len());
				let len = self.rest()[..len].find("/>").unwrap_or(len);
				if len == 0 {
					return fail(at, at, Code::Expected, Some("an attribute value"));
				}
				Ok((at, at + len as u32, at + len as u32))
			}
		}
	}

	/// A quoted or bare attribute value: text with expressions, or one expression on its own.
	fn attribute_value(&mut self, entry: JsEntry) -> Result<Value> {
		let quote = self.char().filter(|c| matches!(c, '"' | '\''));
		if let Some(q) = quote {
			self.at += 1;
			if self.char() == Some(q) {
				let at = self.at;
				self.at += 1;
				let text = self.text_chunk(at, at, true);
				let list = self.list(&[text]);
				return Ok(Value::Nodes(list));
			}
		}
		let chunks = match quote {
			Some(q) => self.sequence(move |w| w.char() == Some(q), "an attribute value", entry)?,
			None => self.sequence(
				|w| {
					w.matches("/>")
						|| w.char()
							.is_none_or(|c| is_space(c) || matches!(c, '"' | '\'' | '=' | '<' | '>' | '`'))
				},
				"an attribute value",
				entry,
			)?,
		};
		if chunks.is_empty() && quote.is_none() {
			return fail(self.at, self.at, Code::Expected, Some("an attribute value"));
		}
		if quote.is_some() && self.char() == quote {
			self.at += 1;
		}
		if quote.is_some() || chunks.len() > 1 || self.host_type(chunks[0]) == self.grammar.text.ty {
			let list = self.list(&chunks);
			Ok(Value::Nodes(list))
		} else {
			Ok(Value::Node(chunks[0]))
		}
	}

	/// Text and expression chunks up to where `done` says.
	fn sequence(&mut self, done: impl Fn(&Self) -> bool, place: &str, entry: JsEntry) -> Result<Vec<NodeId>> {
		let (open, close) = self.grammar.delimiters;
		let mut chunks = self.nodes();
		let mut chunk_start = self.at;
		loop {
			if self.at >= self.len() {
				self.report(error(self.len(), self.len(), Code::UnexpectedEof, None))?;
				let at = self.at;
				self.flush_text(chunk_start, at, &mut chunks);
				return Ok(chunks);
			}
			if done(self) {
				let at = self.at;
				self.flush_text(chunk_start, at, &mut chunks);
				return Ok(chunks);
			}
			if self.verbatim == 0 && self.eat(open) {
				let start = self.at - open.len() as u32;
				if let Some(sigils) = &self.grammar.sigils
					&& (self.matches(sigils.open) || self.matches(sigils.tag))
				{
					return fail(
						start,
						start + 1,
						Code::Placement,
						Some(&format!("A block or tag in {place}")),
					);
				}
				self.flush_text(chunk_start, start, &mut chunks);
				self.space();
				if self.matches("/>") || self.matches(">") {
					return fail(self.at, self.at, Code::Expected, Some(close));
				}
				let expression = self.js(entry, "")?;
				let expression = self.first(expression);
				self.space();
				self.expect(close)?;
				let tag = self.expression_tag(start, self.at, expression)?;
				chunks.push(tag);
				chunk_start = self.at;
			} else {
				self.at += self.char().map_or(1, |c| c.len_utf8() as u32);
			}
		}
	}

	fn flush_text(&mut self, from: u32, to: u32, chunks: &mut Vec<NodeId>) {
		if to > from {
			let text = self.text_chunk(from, to, true);
			chunks.push(text);
		}
	}

	/// A tag between the delimiters: a block, a branch, a close, a special tag, a declaration or
	/// an expression.
	fn tag(&mut self) -> Result<()> {
		let (open, close) = self.grammar.delimiters;
		let start = self.at;
		self.at += open.len() as u32;
		self.space();
		if let Some(sigils) = &self.grammar.sigils {
			if self.eat(sigils.open) {
				return self.open_block(start);
			}
			if self.eat(sigils.branch) {
				return self.branch(start);
			}
			// a `/` that starts a comment is the expression's
			let comment = sigils.close == "/" && (self.matches("/*") || self.matches("//"));
			if !comment && self.eat(sigils.close) {
				return self.close_block(start);
			}
			if self.eat(sigils.tag) {
				return self.special(start);
			}
		}
		if let Some(rule) = &self.grammar.declaration {
			if let Some(word) = ["var", "interface", "enum"].into_iter().find(|w| self.word(w)) {
				let at = self.at;
				return fail(
					at,
					at + word.len() as u32,
					Code::Placement,
					Some("A declaration of that kind"),
				);
			}
			if self.word("let") || self.word("const") || self.word("type") {
				let at = self.at;
				let comments = self.tree().comments.len();
				let statement = self.js(JsEntry::Statement, "")?;
				let statement = self.first(statement);
				let kind = self.tree().node(statement).kind;
				match kind {
					NodeKind::VariableDeclaration {
						kind: VariableKind::Let | VariableKind::Const,
						..
					} => {
						self.space();
						self.expect(close)?;
						let node = self.host(
							rule.ty,
							start,
							self.at,
							&[("declaration", Value::Node(statement))],
							None,
							true,
						);
						self.append(node);
						return Ok(());
					}
					// `{type}` is an expression after all
					NodeKind::ExpressionStatement { .. } => {
						self.at = at;
						self.ast().comments.truncate(comments);
					}
					_ => {
						let node = self.tree().node(statement);
						return fail(
							node.start,
							node.end,
							Code::Placement,
							Some("A declaration of that kind"),
						);
					}
				}
			}
		}
		let Some(rule) = &self.grammar.expression else {
			return fail(start, start + 1, Code::UnexpectedToken, None);
		};
		let expression = self.expression("")?;
		self.space();
		self.expect(close)?;
		let field = match rule.form.items.first() {
			Some(Item::Entry { field, .. }) => field,
			_ => "expression",
		};
		let node = self.host(
			rule.ty,
			start,
			self.at,
			&[(field, Value::Node(expression))],
			None,
			true,
		);
		self.append(node);
		Ok(())
	}

	fn lowercase_word(&mut self) -> &'a str {
		let start = self.at;
		while self.byte().is_some_and(|b| b.is_ascii_lowercase()) {
			self.at += 1;
		}
		&self.src[start as usize..self.at as usize]
	}

	fn open_block(&mut self, start: u32) -> Result<()> {
		let close = self.grammar.delimiters.1;
		let name_at = self.at;
		let name = self.lowercase_word();
		let Some(rule) = self.grammar.block(name) else {
			if let Some(known) = self.grammar.blocks.iter().find(|rule| name.starts_with(rule.name)) {
				let at = name_at + known.name.len() as u32;
				return fail(at, at, Code::Expected, Some("whitespace"));
			}
			self.report(error(name_at, self.at, Code::Expected, Some("a block name")))?;
			self.skip_tag();
			return Ok(());
		};
		self.keyword = name_at;
		if !rule.open.items.is_empty() {
			self.require_space()?;
		}
		let mut read = Read {
			fields: self.fields(),
			body: None,
		};
		self.form(&rule.open, &mut read)?;
		self.space();
		self.expect(close)?;
		let Some(body) = read.body.take().or_else(|| rule.open.body.clone()) else {
			return fail(start, start + 1, Code::Placement, Some("A block without a body"));
		};
		let mut outside = self.nodes();
		let group = self.group_of(&read, &body, &mut outside);
		let (nodes, done) = (self.nodes(), self.fields());
		self.frames.push(Frame::Block {
			start,
			rule,
			fields: read.fields,
			body: (body.field, body.omit),
			nodes,
			done,
			groups: vec![group],
			outside,
			chain: None,
		});
		Ok(())
	}

	fn branch(&mut self, start: u32) -> Result<()> {
		let close = self.grammar.delimiters.1;
		let Some(Frame::Block { rule, .. }) = self.frames.last() else {
			self.report(error(
				start,
				start + 1,
				Code::Placement,
				Some("A branch outside its block"),
			))?;
			self.skip_tag();
			return Ok(());
		};
		let rule: &'a BlockRule = rule;
		// the longest run of words first: `else if` before `else`
		let mut branches: Vec<&'a grammar::BranchRule> = rule.branches.iter().collect();
		branches.sort_by_key(|b| std::cmp::Reverse(b.words.len()));
		let at = self.at;
		let mut found = None;
		for branch in branches {
			self.at = at;
			let mut ok = true;
			for (i, word) in branch.words.iter().enumerate() {
				if i > 0 {
					self.space();
				}
				if !self.word(word) {
					ok = false;
					break;
				}
				self.at += word.len() as u32;
			}
			if ok {
				found = Some(branch);
				break;
			}
		}
		let Some(branch) = found else {
			self.at = at;
			let names = rule
				.branches
				.iter()
				.map(|b| b.words.join(" "))
				.collect::<Vec<_>>()
				.join(" or ");
			return fail(start, start + 1, Code::Expected, Some(&names));
		};
		let body = branch.form.body.clone().unwrap();
		self.finish_body();
		if let Some(child_field) = body.chain {
			// the branch opens a block of its own inside the parent's field, which closes with it
			if let Some(Frame::Block { body: current, .. }) = self.frames.last_mut() {
				*current = ("", true);
			}
			self.keyword = at;
			if !branch.form.items.is_empty() {
				self.require_space()?;
			}
			let mut read = Read {
			fields: self.fields(),
			body: None,
		};
			self.form(&branch.form, &mut read)?;
			self.space();
			self.expect(close)?;
			let child = Body {
				field: child_field,
				omit: false,
				chain: None,
				declares: body.declares.clone(),
			};
			let mut outside = self.nodes();
			let group = self.group_of(&read, &child, &mut outside);
			let (nodes, done) = (self.nodes(), self.fields());
			self.frames.push(Frame::Block {
				start,
				rule,
				fields: read.fields,
				body: (child_field, false),
				nodes,
				done,
				groups: vec![group],
				outside,
				chain: Some(body.field),
			});
			return Ok(());
		}
		let mut read = Read {
			fields: self.fields(),
			body: None,
		};
		self.form(&branch.form, &mut read)?;
		self.space();
		self.expect(close)?;
		let mut outside = self.nodes();
		let group = self.group_of(&read, &body, &mut outside);
		let Some(Frame::Block {
			fields,
			body: current,
			done,
			groups,
			outside: all_outside,
			..
		}) = self.frames.last_mut()
		else {
			unreachable!()
		};
		if done.iter().any(|(field, _)| *field == body.field) {
			return fail(
				start,
				start + 1,
				Code::Duplicate,
				Some(&format!("{{:{}}}", branch.words.join(" "))),
			);
		}
		fields.extend(read.fields.iter().copied());
		*current = (body.field, body.omit);
		groups.push(group);
		all_outside.extend(outside);
		Ok(())
	}

	/// The patterns a body declares, by the fields that hold them.
	/// The scope a body opens, as its form read it: the patterns the body declares, those the
	/// block declares around itself, and the entries read after the first declared one, a key
	/// after the context of an each block, which the scope holds too.
	fn group_of(&self, read: &Read, body: &Body, outside: &mut Vec<NodeId>) -> BodyGroup {
		let mut group = BodyGroup {
			body: body.field,
			fields: Vec::new(),
			inside: Vec::new(),
		};
		let mut opened = false;
		for &(field, value) in &read.fields {
			match body.declares.iter().find(|declare| declare.field == field) {
				Some(declare) => {
					let target = if declare.outside {
						&mut *outside
					} else {
						&mut group.inside
					};
					match value {
						Value::Node(id) => target.push(id),
						Value::Nodes(list) => target.extend(self.tree().list(list).iter().flatten()),
						_ => {}
					}
					if !declare.outside {
						group.fields.push(field);
						opened = true;
					}
				}
				None if opened => group.fields.push(field),
				None => {}
			}
		}
		group
	}

	/// Closes the open body of the block on top: its nodes become the children under its field.
	fn finish_body(&mut self) {
		let Some(Frame::Block { nodes, body, .. }) = self.frames.last_mut() else {
			unreachable!()
		};
		let nodes = std::mem::take(nodes);
		let field = body.0;
		if field.is_empty() {
			return;
		}
		let children = self.children(nodes);
		let Some(Frame::Block { done, .. }) = self.frames.last_mut() else {
			unreachable!()
		};
		done.push((field, children));
	}

	fn close_block(&mut self, start: u32) -> Result<()> {
		let close = self.grammar.delimiters.1;
		let name_at = self.at;
		let name = self.lowercase_word();
		self.space();
		self.expect(close)?;
		let end = self.at;
		let open = self
			.frames
			.iter()
			.any(|frame| matches!(frame, Frame::Block { rule, .. } if rule.name == name));
		if !self.recovering() || !open {
			match self.frames.last() {
				Some(Frame::Block { rule, .. }) if rule.name == name => {}
				Some(Frame::Block { .. }) => {
					return self.report(error(
						name_at,
						name_at + name.len() as u32,
						Code::UnexpectedClose,
						Some(name),
					));
				}
				_ => return self.report(error(start, start + 1, Code::UnexpectedClose, Some(name))),
			}
		}
		loop {
			match self.frames.last() {
				Some(Frame::Block { rule, .. }) if rule.name == name => break,
				Some(Frame::Block {
					start: block_start,
					rule,
					chain,
					..
				}) => {
					let (block_start, what, chained) = (*block_start, rule.name, chain.is_some());
					if !chained {
						self.report(error(block_start, block_start + 1, Code::Unclosed, Some(what)))?;
					}
					self.pop_block(start);
				}
				Some(Frame::Element {
					start: element_start,
					name: span,
					..
				}) => {
					let (element_start, what) = (*element_start, &self.src[span.0 as usize..span.1 as usize]);
					self.report(error(element_start, element_start + 1, Code::Unclosed, Some(what)))?;
					self.close_top(start);
				}
				_ => unreachable!(),
			}
		}
		while !self.pop_block(end) {}
		Ok(())
	}

	/// Closes the block on top at `end`; whether it stood on its own rather than in a chain,
	/// whose parent is then still open.
	fn pop_block(&mut self, end: u32) -> bool {
		self.finish_body();
		let Some(Frame::Block {
			start: block_start,
			rule,
			fields,
			done,
			groups,
			outside,
			chain,
			..
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		let node = self.block_node(rule, block_start, end, fields, done, groups, outside, chain.is_some());
		match chain {
			Some(field) => {
				let mut one = self.nodes();
				one.push(node);
				let children = self.children(one);
				let Some(Frame::Block { done, .. }) = self.frames.last_mut() else {
					unreachable!()
				};
				done.push((field, children));
				false
			}
			None => {
				self.append(node);
				true
			}
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn block_node(
		&mut self,
		rule: &BlockRule,
		start: u32,
		end: u32,
		mut fields: Vec<(&'static str, Value)>,
		done: Vec<(&'static str, Value)>,
		groups: Vec<BodyGroup>,
		outside: Vec<NodeId>,
		chained: bool,
	) -> NodeId {
		// every entry the block could have read, null unless left out on purpose
		let mut entries = Vec::new();
		collect_entries(&rule.open.items, &mut entries);
		for branch in &rule.branches {
			collect_entries(&branch.form.items, &mut entries);
		}
		for (field, omit) in entries {
			if !omit && !fields.iter().any(|(k, _)| *k == field) {
				fields.push((field, Value::Null));
			}
		}
		if let Some(flag) = rule.chain_flag {
			fields.push((flag, Value::Bool(chained)));
		}
		// the fields outside every scope first, then each body's scope: its fields, then the body
		let mut ordered = self.fields();
		ordered.extend(
			fields
				.iter()
				.copied()
				.filter(|(f, _)| !groups.iter().any(|group| group.fields.contains(f))),
		);
		let mut bodies: Vec<(&'static str, bool)> = Vec::new();
		collect_bodies(&rule.open, &mut bodies);
		for branch in &rule.branches {
			collect_bodies(&branch.form, &mut bodies);
		}
		for (field, omit) in bodies {
			if !omit && !done.iter().any(|(k, _)| *k == field) {
				ordered.push((field, Value::Null));
			}
		}
		let base = self.ast().host_fields.len() as u32;
		let groups_at = self.ast().host_groups.len() as u32;
		let mut count = 0;
		for &(body, children) in &done {
			let from = ordered.len() as u32;
			let group = groups.iter().find(|group| group.body == body);
			if let Some(group) = group {
				for &field in &group.fields {
					if let Some(&entry) = fields.iter().find(|(f, _)| *f == field) {
						ordered.push(entry);
					}
				}
			}
			ordered.push((body, children));
			let inside: &[NodeId] = group.map_or(&[], |group| &group.inside);
			if self.grammar.fragment_scope || !inside.is_empty() {
				let inside = self.list(inside);
				let node = match children {
					Value::Node(fragment) if self.grammar.fragment.is_some() => Some(fragment),
					_ => None,
				};
				self.ast().host_groups.push(HostGroup {
					inside,
					from: base + from,
					until: base + ordered.len() as u32,
					node,
				});
				count += 1;
			}
		}
		let outside_list = self.list(&outside);
		self.recycle_nodes(outside);
		let scope = Some(Opens {
			outside: outside_list,
			groups: (groups_at, count),
		});
		let node = self.host(rule.ty, start, end, &ordered, scope, true);
		self.recycle_fields(ordered);
		self.recycle_fields(fields);
		self.recycle_fields(done);
		node
	}

	fn special(&mut self, start: u32) -> Result<()> {
		let (node, rule) = self.tag_node(start)?;
		if rule.attribute {
			return fail(
				start,
				self.at,
				Code::Placement,
				Some(&format!("A {} tag in content", rule.name)),
			);
		}
		self.append(node);
		Ok(())
	}

	/// A special tag after its sigil: its node, and its rule.
	fn tag_node(&mut self, start: u32) -> Result<(NodeId, &'a TagRule)> {
		let close = self.grammar.delimiters.1;
		let name_at = self.at;
		let name = self.lowercase_word();
		let Some(rule) = self.grammar.tag(name) else {
			if let Some(known) = self.grammar.tags.iter().find(|rule| name.starts_with(rule.name)) {
				let at = name_at + known.name.len() as u32;
				return fail(at, at, Code::Expected, Some("whitespace"));
			}
			return fail(name_at, self.at, Code::Expected, Some("a tag name"));
		};
		let rule: &'a TagRule = rule;
		self.keyword = name_at;
		let mut read = Read {
			fields: self.fields(),
			body: None,
		};
		let lists_names = matches!(
			rule.form.items.first(),
			Some(Item::Entry {
				entry: Entry::Identifiers,
				..
			})
		);
		if !rule.form.items.is_empty() && !lists_names {
			self.require_space()?;
		}
		self.form(&rule.form, &mut read)?;
		self.space();
		self.expect(close)?;
		let node = self.host(rule.ty, start, self.at, &read.fields, None, true);
		self.recycle_fields(read.fields);
		Ok((node, rule))
	}

	/// Runs a form at the cursor: every literal in place, every entry read, the first fitting
	/// alternative of a group taken.
	fn form(&mut self, form: &Form, read: &mut Read) -> Result<()> {
		self.items(&form.items, &[], read)
	}

	fn items(&mut self, items: &[Item], follow: &[&'static str], read: &mut Read) -> Result<()> {
		for (i, item) in items.iter().enumerate() {
			match item {
				Item::Literal(literal) => {
					self.space();
					if !self.literal_here(literal) {
						return fail(self.at, self.at, Code::Expected, Some(literal));
					}
					self.at += literal.len() as u32;
				}
				Item::Entry { field, entry, .. } => {
					self.space();
					let stops = first_literals(&items[i + 1..], follow);
					let value = self.entry(*entry, &stops)?;
					read.fields.push((field, value));
				}
				Item::Group { alternatives, required } => {
					self.space();
					let after = first_literals(&items[i + 1..], follow);
					let mut taken = false;
					for alternative in alternatives {
						if self.alternative_here(alternative, &after) {
							self.items(&alternative.items, &after, read)?;
							if let Some(body) = &alternative.body {
								read.body = Some(body.clone());
							}
							taken = true;
							break;
						}
					}
					if !taken && *required {
						let mut names = Vec::new();
						for alternative in alternatives {
							names.extend(first_literals(&alternative.items, &[]));
						}
						return fail(self.at, self.at, Code::Expected, Some(&names.join(" or ")));
					}
				}
			}
		}
		Ok(())
	}

	fn literal_here(&self, literal: &str) -> bool {
		if literal.starts_with(is_id_start) {
			self.word(literal)
		} else {
			self.matches(literal)
		}
	}

	/// Whether an alternative starts here: by its first literal, or, for one that starts with an
	/// entry, by what the entry starts with: anything that is not the end of the tag nor a token
	/// that follows the group, unless the entry has an opener of its own. An optional group that
	/// is not here is skipped for what follows it.
	fn alternative_here(&self, alternative: &Alternative, after: &[&'static str]) -> bool {
		let mut items = alternative.items.iter();
		loop {
			let item = items.next();
			if let Some(Item::Group { alternatives, required }) = item {
				if alternatives.iter().any(|a| self.alternative_here(a, after)) {
					return true;
				}
				if *required {
					return false;
				}
				continue;
			}
			return match item {
				Some(Item::Literal(literal)) => self.literal_here(literal),
				Some(Item::Entry { entry, .. }) => self.entry_here(*entry, after),
				_ => true,
			};
		}
	}

	fn entry_here(&self, entry: Entry, after: &[&'static str]) -> bool {
		let close = self.grammar.delimiters.1;
		match entry {
			Entry::TypeParameters => self.matches("<"),
			Entry::Params => self.matches("("),
			Entry::Identifier => self.char().is_some_and(is_id_start),
			Entry::Pattern => self.char().is_some_and(|c| is_id_start(c) || c == '[' || c == '{'),
			_ => {
				!self.matches(close)
					&& self.at < self.limit
					&& !self.char().is_some_and(|c| matches!(c, ')' | ']' | '}' | ',' | ';'))
					&& !after.iter().any(|stop| self.literal_here(stop))
			}
		}
	}

	fn entry(&mut self, entry: Entry, stops: &[&'static str]) -> Result<Value> {
		let stop = stops.join(" ");
		Ok(match entry {
			Entry::Expression => {
				// a token that continues an expression is JavaScript's before it is the host's
				let stop: Vec<&str> = stops
					.iter()
					.copied()
					.filter(|s| !matches!(*s, "(" | "[" | "." | "?." | "`"))
					.collect();
				let roots = self.js(JsEntry::Expression, &stop.join(" "))?;
				Value::Node(self.first(roots))
			}
			Entry::Pattern => {
				let roots = self.js(JsEntry::Pattern, &stop)?;
				Value::Node(self.first(roots))
			}
			Entry::Statement => {
				let roots = self.js(JsEntry::Statement, &stop)?;
				Value::Node(self.first(roots))
			}
			Entry::Code => {
				let (start, end) = (self.at, self.limit);
				let comments = self.tree().comments.len();
				let mark = self.tree().mark();
				// one expression, read strictly so that recovery cannot stand in for the statements
				let recovering = std::mem::replace(&mut self.options.error_recovery, false);
				let expression = self.js(JsEntry::Expression, "");
				self.options.error_recovery = recovering;
				if let Ok(roots) = expression {
					self.space_to(end);
					if self.at >= end {
						return Ok(Value::Node(self.first(roots)));
					}
				}
				self.at = start;
				self.ast().comments.truncate(comments);
				self.ast().truncate(mark);
				let program = self.program(start, end)?;
				self.at = end;
				Value::Node(program)
			}
			Entry::TypeParameters => {
				let node = self.js(JsEntry::TypeParameters, &stop)?;
				let node = self.first(node);
				let node = self.tree().node(node);
				Value::Slice(node.start + 1, node.end - 1)
			}
			Entry::Params => {
				Value::Nodes(self.js(JsEntry::Params, &stop)?)
			}
			Entry::Identifier => Value::Node(self.identifier()?),
			Entry::Text => {
				let close = self.grammar.delimiters.1;
				let rest = &self.src[self.at as usize..self.limit as usize];
				let len = rest.find(close).unwrap_or(rest.len());
				let text = &rest[..len];
				let start = self.at + (text.len() - text.trim_start_matches(is_space).len()) as u32;
				let end = self.at + text.trim_end_matches(is_space).len() as u32;
				self.at += len as u32;
				Value::Slice(start, end.max(start))
			}
			Entry::Identifiers => {
				let close = self.grammar.delimiters.1;
				let mut ids = self.nodes();
				loop {
					self.space();
					if self.matches(close) || self.at >= self.limit {
						break;
					}
					ids.push(self.identifier()?);
					self.space();
					if !self.eat(",") {
						break;
					}
				}
				let list = self.list(&ids);
				Value::Nodes(list)
			}
			Entry::Const => {
				let id = self.js(JsEntry::Pattern, "=")?;
				let id = self.first(id);
				let id_start = self.tree().node(id).start;
				self.space();
				self.expect("=")?;
				self.space();
				let init_at = self.at;
				let init = self.js(JsEntry::Expression, "")?;
				let init = self.first(init);
				let declarator_end = self.at;
				let init_node = *self.tree().node(init);
				if matches!(init_node.kind, NodeKind::SequenceExpression { .. })
					&& !self.src[init_at as usize..init_node.start as usize].contains('(')
				{
					return fail(
						init_node.start,
						init_node.end,
						Code::Expected,
						Some("a single declaration"),
					);
				}
				self.space();
				let end = self.at;
				let declarator = self.ast().add(
					NodeKind::VariableDeclarator { id, init: Some(init) },
					id_start,
					declarator_end,
				);
				let declarations = self.list(&[declarator]);
				let keyword = self.keyword;
				Value::Node(self.ast().add(
					NodeKind::VariableDeclaration {
						declarations,
						kind: VariableKind::Const,
					},
					keyword,
					end,
				))
			}
		})
	}

	fn expression(&mut self, stop: &str) -> Result<NodeId> {
		let roots = self.js(JsEntry::Expression, stop)?;
		Ok(self.first(roots))
	}

	/// The JavaScript at the cursor, read by the parser into the same tree up to the limit; the
	/// cursor moves past it.
	fn js(&mut self, entry: JsEntry, stop: &str) -> Result<List> {
		let ast = self.ast.take().unwrap();
		let src = &self.src[..self.limit as usize];
		let mut parser = Parser::<E>::new(src, self.at, self.options, 0, stop, ast);
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
		self.ast = Some(parser.finish());
		let roots = roots?;
		self.at = end;
		Ok(roots)
	}

	/// The one root a JavaScript entry read.
	fn first(&self, roots: List) -> NodeId {
		self.tree().nth(roots, 0).unwrap()
	}

	fn program(&mut self, start: u32, end: u32) -> Result<NodeId> {
		let ast = self.ast.take().unwrap();
		let src = &self.src[..end as usize];
		// the template may declare what the script exports
		let mut options = self.options;
		options.allow_undeclared_exports = true;
		let mut parser = Parser::<E>::new(src, start, options, (end - start) as usize, "", ast);
		let program = parser.start().and_then(|()| parser.parse_program());
		self.ast = Some(parser.finish());
		program
	}

	/// An empty identifier standing where one could not be read.
	fn placeholder(&mut self, start: u32, end: u32) -> NodeId {
		let name = self.intern("");
		self.ast().add(NodeKind::Identifier { name }, start, end)
	}

	/// An identifier the host reads itself, as a node.
	fn identifier(&mut self) -> Result<NodeId> {
		let start = self.at;
		match self.char() {
			Some(c) if is_id_start(c) => self.at += c.len_utf8() as u32,
			_ => {
				// under recovery an empty identifier stands where one was expected
				self.report(error(start, start, Code::Expected, Some("an identifier")))?;
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
		let name = self.intern(&self.src[start as usize..end as usize]);
		Ok(self.ast().add(NodeKind::Identifier { name }, start, end))
	}
}

/// A word that cannot name a binding.
fn reserved(word: &str) -> bool {
	use crate::lexer::token::word::{ENUM, KEYWORD, STRICT, flags};
	flags(word) & (KEYWORD | STRICT | ENUM) != 0 || matches!(word, "this" | "true" | "false" | "null")
}

/// The length of a `</name>` closer at the start of `rest`, in any case, attributes and all.
fn closing_tag(rest: &str, name: &str) -> Option<usize> {
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

/// The literals that can come first after a point in a form: what an entry before it stops at.
fn first_literals(items: &[Item], follow: &[&'static str]) -> Vec<&'static str> {
	let mut out = Vec::new();
	for item in items {
		match item {
			Item::Literal(literal) => {
				out.push(*literal);
				return out;
			}
			Item::Entry { .. } => return out,
			Item::Group { alternatives, required } => {
				for alternative in alternatives {
					out.extend(first_literals(&alternative.items, &[]));
				}
				if *required {
					return out;
				}
			}
		}
	}
	out.extend_from_slice(follow);
	out
}

fn collect_entries(items: &[Item], out: &mut Vec<(&'static str, bool)>) {
	for item in items {
		match item {
			Item::Entry { field, omit, .. } => out.push((field, *omit)),
			Item::Group { alternatives, .. } => {
				for alternative in alternatives {
					collect_entries(&alternative.items, out);
				}
			}
			Item::Literal(_) => {}
		}
	}
}

fn collect_bodies(form: &Form, out: &mut Vec<(&'static str, bool)>) {
	let mut push = |body: &Body| {
		if !out.iter().any(|(f, _)| *f == body.field) {
			out.push((body.field, body.omit));
		}
	};
	if let Some(body) = &form.body {
		push(body);
	}
	for item in &form.items {
		if let Item::Group { alternatives, .. } = item {
			for alternative in alternatives {
				if let Some(body) = &alternative.body {
					push(body);
				}
			}
		}
	}
}
