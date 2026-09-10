//! The host layer: a template language read by its grammar, the JavaScript inside it read by
//! the parser, one tree for both. The walker knows what every such language shares, tags and
//! text and where JavaScript begins; the grammar says the rest.

pub mod entities;
pub mod grammar;

use std::borrow::Cow;

use crate::ast::{Ast, Comment, CommentKind, Host, List, NodeId, NodeKind, Opens, Value, VariableKind};
use crate::error::{Code, SyntaxError};
use crate::interner::StrId;
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry as JsEntry, Extension, Options, Parser, Result};
pub use grammar::Grammar;
use grammar::{Alternative, BlockRule, Body, DirectiveValue, Entry, Form, Item, Match, RootField, TagRule};

const VOID: [&str; 16] = [
	"area", "base", "br", "col", "command", "embed", "hr", "img", "input", "keygen", "link", "meta", "param", "source",
	"track", "wbr",
];

fn is_void(name: &str) -> bool {
	VOID.contains(&name) || name.eq_ignore_ascii_case("!doctype")
}

/// Whether the browser closes `current` when `next` opens inside it.
fn closes(current: &str, next: &str) -> bool {
	match current {
		"li" => next == "li",
		"dt" | "dd" => matches!(next, "dt" | "dd"),
		"p" => matches!(
			next,
			"address"
				| "article" | "aside"
				| "blockquote" | "div"
				| "dl" | "fieldset"
				| "footer" | "form"
				| "h1" | "h2" | "h3"
				| "h4" | "h5" | "h6"
				| "header" | "hgroup"
				| "hr" | "main"
				| "menu" | "nav"
				| "ol" | "p" | "pre"
				| "section" | "table"
				| "ul"
		),
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
		} else if c.is_ascii_alphanumeric() {
		} else if seen_dash && (c == '.' || c == '_' || c == '\u{b7}' || c as u32 >= 0xc0) {
		} else {
			return false;
		}
	}
	true
}

// code points a browser repairs when a reference names them
const WINDOWS_1252: [u32; 32] = [
	8364, 129, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 141, 381, 143, 144, 8216, 8217, 8220, 8221,
	8226, 8211, 8212, 732, 8482, 353, 8250, 339, 157, 382, 376,
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
					if attribute && !with && after.is_some_and(|b| *b == b'=' || b.is_ascii_alphanumeric() || *b == b'_') {
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

fn fail<T>(pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<T> {
	let message: Cow<'static, str> = match arg {
		Some(arg) => code.with(arg).into(),
		None => code.message().into(),
	};
	Err(Box::new(SyntaxError::with(pos, code, message).to(end)))
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
		let Some(after) = rest.strip_prefix(script.name) else { continue };
		if !after.starts_with(is_space) {
			continue;
		}
		let tag_end = after.find('>').unwrap_or(after.len());
		let mut attributes = after[..tag_end].trim_start_matches(is_space);
		while !attributes.is_empty() {
			let name_len = attributes.find(|c: char| is_space(c) || c == '=' || c == '/').unwrap_or(attributes.len());
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
			if script.typescript.iter().any(|&(attribute, wanted)| name == attribute && wanted.is_none_or(|w| value == Some(w))) {
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
	},
	Block {
		start: u32,
		rule: &'a BlockRule,
		fields: Vec<(&'static str, Value)>,
		/// The open body: its field and whether it is left out of blocks that never opened it.
		body: (&'static str, bool),
		nodes: Vec<NodeId>,
		/// Bodies closed so far, each as a fragment.
		done: Vec<(&'static str, NodeId)>,
		inside: Vec<NodeId>,
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

struct Walker<'a, E: Extension> {
	src: &'a str,
	full: u32,
	grammar: &'a Grammar,
	options: Options,
	ast: Option<Ast<E::Data>>,
	at: u32,
	frames: Vec<Frame<'a>>,
	once: Vec<&'static str>,
	/// Where the current tag's word starts, for a declaration spelled without its keyword.
	keyword: u32,
}

/// Parses a document by its grammar: the host's tree with the JavaScript inside it, positions
/// of the whole source. Returns the tree and its root.
pub(crate) fn parse_document<E: Extension>(
	src: &str,
	grammar: &Grammar,
	options: Options,
	reused: Option<Ast<E::Data>>,
) -> Result<(Ast<E::Data>, NodeId)> {
	let full = src.len() as u32;
	let cut = src.trim_end_matches(is_space);
	let mut walker = Walker::<E> {
		src: cut,
		full,
		grammar,
		options,
		ast: Some(reused.unwrap_or_else(|| Ast::sized(src.len()))),
		at: 0,
		frames: vec![Frame::Root {
			nodes: Vec::new(),
			instance: None,
			module: None,
			css: None,
		}],
		once: Vec::new(),
		keyword: 0,
	};
	let root = walker.run()?;
	Ok((walker.ast.take().unwrap(), root))
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
			fail(self.at, self.at, Code::Expected, Some(s))
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
		fields: Vec<(&'static str, Value)>,
		scope: Option<Opens>,
		span: bool,
	) -> NodeId {
		let ast = self.ast();
		let from = ast.host_fields.len() as u32;
		let len = fields.len() as u32;
		ast.host_fields.extend(fields);
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
		self.host(
			"Text",
			start,
			end,
			vec![("raw", Value::Slice(start, end)), ("data", data)],
			None,
			true,
		)
	}

	fn fragment(&mut self, nodes: Vec<NodeId>) -> NodeId {
		let list = self.list(&nodes);
		self.host("Fragment", 0, 0, vec![("nodes", Value::Nodes(list))], None, false)
	}

	fn list(&mut self, nodes: &[NodeId]) -> List {
		let items: Vec<Option<NodeId>> = nodes.iter().map(|&id| Some(id)).collect();
		self.ast().add_list(&items)
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
		let NodeKind::Host(index) = ast.node(id).kind else { return None };
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

	/// The expression of `name={expression}`, quoted or not.
	fn attribute_expression(&self, id: NodeId) -> Option<NodeId> {
		let chunk = self.attribute_chunk(id)?;
		if self.host_type(chunk) != "ExpressionTag" {
			return None;
		}
		match self.field_of(chunk, "expression")? {
			Value::Node(expression) => Some(expression),
			_ => None,
		}
	}

	/// The text of `name="text"`.
	fn attribute_text(&self, id: NodeId) -> Option<&str> {
		let Value::Nodes(_) = self.field_of(id, "value")? else { return None };
		let chunk = self.attribute_chunk(id)?;
		if self.host_type(chunk) != "Text" {
			return None;
		}
		self.string_of(self.field_of(chunk, "data")?)
	}

	fn run(&mut self) -> Result<NodeId> {
		while self.at < self.len() {
			match self.byte() {
				Some(b'<') => self.element()?,
				Some(b'{') => self.tag()?,
				_ => self.text_node(),
			}
		}
		if self.frames.len() > 1 {
			let (start, what) = match self.frames.last().unwrap() {
				Frame::Element { start, name, .. } => (*start, &self.src[name.0 as usize..name.1 as usize]),
				Frame::Block { start, rule, .. } => (*start, rule.name),
				Frame::Root { .. } => unreachable!(),
			};
			return fail(start, start + 1, Code::Unclosed, Some(what));
		}
		let Some(Frame::Root {
			nodes,
			instance,
			module,
			css,
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		let fragment = self.fragment(nodes);
		let mut fields = Vec::new();
		for &(field, holds, omit) in &self.grammar.document.fields {
			let value = match holds {
				RootField::Fragment => Some(fragment),
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
		let full = self.full;
		Ok(self.host(self.grammar.document.ty, 0, full, fields, None, true))
	}

	fn text_node(&mut self) {
		let start = self.at;
		let rest = self.rest();
		let len = rest.find(['<', '{']).unwrap_or(rest.len());
		self.at += len as u32;
		let node = self.text_chunk(start, self.at, false);
		self.append(node);
	}

	/// Whether the nearest element around the cursor, past blocks and meta elements, is `name`.
	fn nearest_element_is(&self, name: &str) -> bool {
		for frame in self.frames.iter().rev() {
			if let Frame::Element { name: span, ty, .. } = frame {
				if &self.src[span.0 as usize..span.1 as usize] == name {
					return true;
				}
				if *ty == "RegularElement" || *ty == "Component" {
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
				return fail(self.len(), self.len(), Code::UnexpectedEof, None);
			};
			let data_start = self.at;
			self.at += len as u32 + 3;
			let node = self.host(
				"Comment",
				start,
				self.at,
				vec![("data", Value::Slice(data_start, self.at - 3))],
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
			if is_void(name) {
				return fail(start, start + 1, Code::Placement, Some("A closing tag of a void element"));
			}
			return self.close_element(start, name);
		}
		let name = self.tag_name(false)?;
		let name_span = (start + 1, self.at);
		let rule = self.grammar.element(name);
		let namespaced = name.split_once(':').is_some_and(|(prefix, _)| prefix == self.grammar.name);
		let Some(rule) = rule.filter(|rule| rule.name != Match::Any || (!namespaced && valid_name(name))) else {
			return fail(name_span.0, name_span.1, Code::InvalidName, Some(name));
		};
		if rule.root && self.frames.len() > 1 {
			return fail(start, start + 1, Code::Placement, Some(name));
		}
		if rule.once {
			if self.once.contains(&rule.ty) {
				return fail(start, start + 1, Code::Duplicate, Some(name));
			}
			self.once.push(rule.ty);
		}
		let plain = self.grammar.element("*").map_or(rule.ty, |any| any.ty);
		let mut ty = rule.ty;
		if let Some(inside) = rule.inside {
			if !self.nearest_element_is(inside) {
				ty = plain;
			}
		}
		if rule.outside.is_some() && self.frames.iter().any(|frame| matches!(frame, Frame::Element { shadowroot: true, .. })) {
			ty = plain;
		}
		self.space();
		// the browser closes the open element when this one cannot sit inside it
		if let Some(Frame::Element { name: parent, ty: parent_ty, .. }) = self.frames.last() {
			let parent_name = &self.src[parent.0 as usize..parent.1 as usize];
			if *parent_ty == plain && closes(parent_name, name) {
				self.close_top(start);
			}
		}
		let at_root = self.frames.len() == 1;
		let script = self.grammar.script.as_ref().filter(|s| s.name == name && at_root);
		let style = self.grammar.style.filter(|s| *s == name && at_root);
		let mut attributes = Vec::new();
		let mut seen: Vec<(&'static str, String)> = Vec::new();
		let mut shadowroot = false;
		loop {
			let attribute = if script.is_some() || style.is_some() {
				self.static_attribute()?
			} else {
				self.attribute()?
			};
			let Some((node, kind, attribute_name)) = attribute else { break };
			if let Some(attribute_name) = attribute_name {
				if kind == "Attribute" && attribute_name == "shadowrootmode" && ty == plain {
					shadowroot = true;
				}
				let key = if kind == "BindDirective" { "Attribute" } else { kind };
				if matches!(kind, "Attribute" | "BindDirective" | "StyleDirective" | "ClassDirective") {
					if seen.iter().any(|(k, n)| *k == key && *n == attribute_name) {
						let node = self.tree().node(node);
						return fail(node.start, node.end, Code::Duplicate, Some(&attribute_name));
					}
					if attribute_name != "this" {
						seen.push((key, attribute_name.to_string()));
					}
				}
			}
			attributes.push(node);
			self.space();
		}
		let mut fields = Vec::new();
		if let Some((field, text)) = rule.this {
			let Some(position) = attributes.iter().position(|&id| self.attribute_named(id, "this")) else {
				return fail(start, start + 1, Code::Expected, Some("a this attribute"));
			};
			let this = attributes.remove(position);
			let value = match self.attribute_expression(this) {
				Some(expression) => expression,
				None => {
					let chunk = self.attribute_chunk(this).filter(|&chunk| text && self.host_type(chunk) == "Text");
					let Some(chunk) = chunk else {
						let node = self.tree().node(this);
						return fail(node.start, node.end, Code::Expected, Some("an expression as this"));
					};
					let node = *self.tree().node(chunk);
					let value = match self.field_of(chunk, "data") {
						Some(Value::Str(s)) => s,
						Some(Value::Slice(a, b)) => self.intern(&self.src[a as usize..b as usize]),
						_ => unreachable!(),
					};
					self.ast().add(NodeKind::StringLiteral { value }, node.start, node.end)
				}
			};
			fields.push((field, Value::Node(value)));
		}
		if let Some(script) = script {
			self.expect(">")?;
			let content_start = self.at;
			let Some(close) = self.find_closing(name) else {
				return fail(self.len(), self.len(), Code::Unclosed, Some(name));
			};
			let program = self.program(content_start, close)?;
			self.at = close;
			self.close_tag(name)?;
			let mut module = false;
			for &id in &attributes {
				for &(attribute, value) in &script.module {
					if !self.attribute_named(id, attribute) {
						continue;
					}
					match value {
						Some(value) if self.attribute_text(id) != Some(value) => {
							let node = self.tree().node(id);
							return fail(node.start, node.end, Code::Expected, Some(&format!("{attribute} to be \"{value}\"")));
						}
						None if !matches!(self.field_of(id, "value"), Some(Value::Bool(true))) => {
							let node = self.tree().node(id);
							return fail(node.start, node.end, Code::Expected, Some(&format!("{attribute} without a value")));
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
				vec![
					("context", Value::Str(context)),
					("content", Value::Node(program)),
					("attributes", Value::Nodes(attributes)),
				],
				None,
				true,
			);
			let Some(Frame::Root { instance, module: module_slot, .. }) = self.frames.first_mut() else { unreachable!() };
			let slot = if module { module_slot } else { instance };
			if slot.is_some() {
				return fail(start, start + 1, Code::Duplicate, Some(name));
			}
			*slot = Some(node);
			return Ok(());
		}
		if style.is_some() {
			self.expect(">")?;
			let content_start = self.at;
			let Some(close) = self.find_closing(name) else {
				return fail(self.len(), self.len(), Code::Unclosed, Some(name));
			};
			self.at = close;
			self.close_tag(name)?;
			let attributes = self.list(&attributes);
			let content = self.host(
				"Content",
				content_start,
				close,
				vec![("styles", Value::Slice(content_start, close)), ("comment", Value::Null)],
				None,
				true,
			);
			let node = self.host(
				"StyleSheet",
				start,
				self.at,
				vec![
					("attributes", Value::Nodes(attributes)),
					("children", Value::Nodes(List::EMPTY)),
					("content", Value::Node(content)),
				],
				None,
				true,
			);
			let Some(Frame::Root { css, .. }) = self.frames.first_mut() else { unreachable!() };
			if css.is_some() {
				return fail(start, start + 1, Code::Duplicate, Some(name));
			}
			*css = Some(node);
			return Ok(());
		}
		let self_closing = self.eat("/") || is_void(name);
		self.expect(">")?;
		let name_id = self.intern(name);
		fields.insert(0, ("name", Value::Str(name_id)));
		let mut finish = |w: &mut Self, nodes: Vec<NodeId>, end: u32| {
			let attributes = w.list(&attributes);
			fields.push(("attributes", Value::Nodes(attributes)));
			let fragment = w.fragment(nodes);
			fields.push(("fragment", Value::Node(fragment)));
			let node = w.host(ty, start, end, std::mem::take(&mut fields), None, true);
			w.append(node);
		};
		if self_closing {
			let end = self.at;
			finish(self, Vec::new(), end);
			return Ok(());
		}
		if name == "textarea" {
			let nodes = self.sequence(|w| closing_textarea(w.rest()).is_some(), "a textarea")?;
			self.at += closing_textarea(self.rest()).unwrap() as u32;
			let end = self.at;
			finish(self, nodes, end);
			return Ok(());
		}
		if self.grammar.script.as_ref().is_some_and(|s| s.name == name) || self.grammar.style == Some(name) {
			// raw text, which the browser reads the same way
			let content_start = self.at;
			let closer = format!("</{name}>");
			let close = self.rest().find(&closer).map_or(self.len(), |i| self.at + i as u32);
			self.at = close;
			let node = self.host(
				"Text",
				content_start,
				close,
				vec![("raw", Value::Slice(content_start, close)), ("data", Value::Slice(content_start, close))],
				None,
				true,
			);
			self.expect(&closer)?;
			let end = self.at;
			finish(self, vec![node], end);
			return Ok(());
		}
		drop(finish);
		self.frames.push(Frame::Element {
			start,
			name: name_span,
			ty,
			attributes,
			fields,
			nodes: Vec::new(),
			shadowroot,
		});
		Ok(())
	}

	/// `</name`, optional space and `>`, from the cursor.
	fn find_closing(&self, name: &str) -> Option<u32> {
		let rest = self.rest();
		let mut from = 0;
		while let Some(i) = rest[from..].find("</") {
			let at = from + i + 2;
			if let Some(after) = rest[at..].strip_prefix(name) {
				if after.trim_start_matches(is_space).starts_with('>') {
					return Some(self.at + (from + i) as u32);
				}
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
			..
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		let attributes = self.list(&attributes);
		fields.push(("attributes", Value::Nodes(attributes)));
		let fragment = self.fragment(nodes);
		fields.push(("fragment", Value::Node(fragment)));
		let node = self.host(ty, start, end, fields, None, true);
		self.append(node);
	}

	fn close_element(&mut self, start: u32, name: &str) -> Result<()> {
		let plain = self.grammar.element("*").map(|any| any.ty);
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
						return fail(start, start + 1, Code::UnexpectedClose, Some(name));
					}
					// the browser closes it here
					self.close_top(start);
				}
				_ => return fail(start, start + 1, Code::UnexpectedClose, Some(name)),
			}
		}
	}

	/// `name`, `name=value` or `name="text"` of a script or style tag.
	fn static_attribute(&mut self) -> Result<Option<(NodeId, &'static str, Option<String>)>> {
		let start = self.at;
		let name = self.tag_name(true)?;
		if name.is_empty() {
			return Ok(None);
		}
		let mut value = Value::Bool(true);
		if self.eat("=") {
			self.space();
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
					let len = self.rest().find(|c: char| c == '>' || is_space(c)).unwrap_or(self.rest().len());
					if len == 0 {
						return fail(value_start, value_start, Code::Expected, Some("an attribute value"));
					}
					self.at += len as u32;
					(value_start, self.at)
				}
			};
			let text = self.text_chunk(raw_start, raw_end, true);
			let list = self.list(&[text]);
			value = Value::Nodes(list);
		}
		if self.char().is_some_and(|c| c == '"' || c == '\'') {
			return fail(self.at, self.at, Code::Expected, Some("="));
		}
		let name_id = self.intern(name);
		let node = self.host(
			"Attribute",
			start,
			self.at,
			vec![("name", Value::Str(name_id)), ("value", value)],
			None,
			true,
		);
		Ok(Some((node, "Attribute", Some(name.to_string()))))
	}

	fn comment_between_attributes(&mut self) -> bool {
		let start = self.at;
		let (kind, len) = if self.matches("//") {
			(CommentKind::Line, self.rest().find('\n').unwrap_or(self.rest().len()))
		} else if self.matches("/*") {
			(CommentKind::Block, self.rest()[2..].find("*/").map_or(self.rest().len(), |i| i + 4))
		} else {
			return false;
		};
		self.at += len as u32;
		let end = self.at;
		self.ast().comments.push(Comment { kind, start, end });
		true
	}

	/// One attribute: a plain one, a shorthand, a spread, an attachment or a directive; the node,
	/// its type and its name for the duplicate check.
	fn attribute(&mut self) -> Result<Option<(NodeId, &'static str, Option<String>)>> {
		while self.comment_between_attributes() {
			self.space();
		}
		let start = self.at;
		if self.eat("{") {
			self.space();
			if let Some(ty) = self.grammar.attach {
				if self.eat("@") {
					self.expect("attach")?;
					self.require_space()?;
					let expression = self.expression("")?;
					self.space();
					self.expect("}")?;
					let node = self.host(ty, start, self.at, vec![("expression", Value::Node(expression))], None, true);
					return Ok(Some((node, ty, None)));
				}
			}
			if self.eat("...") {
				let Some(ty) = self.grammar.spread else {
					return fail(start, start + 1, Code::UnexpectedToken, None);
				};
				let expression = self.expression("")?;
				self.space();
				self.expect("}")?;
				let node = self.host(ty, start, self.at, vec![("expression", Value::Node(expression))], None, true);
				return Ok(Some((node, ty, None)));
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
			let tag = self.host("ExpressionTag", id_start, id_end, vec![("expression", Value::Node(id))], None, true);
			let name_id = self.intern(&name);
			let node = self.host(
				"Attribute",
				start,
				self.at,
				vec![("name", Value::Str(name_id)), ("value", Value::Node(tag))],
				None,
				true,
			);
			return Ok(Some((node, "Attribute", Some(name))));
		}
		let name = self.tag_name(true)?;
		if name.is_empty() {
			return Ok(None);
		}
		let mut end = self.at;
		self.space();
		let directive = name
			.split_once(':')
			.and_then(|(prefix, rest)| self.grammar.directive(prefix).map(|rule| (rule, rest)));
		let mut value = Value::Bool(true);
		if self.eat("=") {
			self.space();
			if self.matches("/>") {
				// `<a href=/>`: the slash is the value
				let slash = self.at;
				self.at += 1;
				let text = self.text_chunk(slash, slash + 1, true);
				let list = self.list(&[text]);
				value = Value::Nodes(list);
			} else {
				value = self.attribute_value()?;
			}
			end = self.at;
		} else if self.char().is_some_and(|c| c == '"' || c == '\'') {
			return fail(self.at, self.at, Code::Expected, Some("="));
		}
		let Some((rule, rest)) = directive else {
			let name_id = self.intern(name);
			let node = self.host(
				"Attribute",
				start,
				end,
				vec![("name", Value::Str(name_id)), ("value", value)],
				None,
				true,
			);
			return Ok(Some((node, "Attribute", Some(name.to_string()))));
		};
		let mut modifiers = rest.split('|');
		let directive_name = modifiers.next().unwrap_or("");
		if directive_name.is_empty() {
			return fail(start, start + (name.len() - rest.len()) as u32, Code::Expected, Some("a directive name"));
		}
		let name_id = self.intern(directive_name);
		let mut fields = vec![("name", Value::Str(name_id))];
		let from = self.ast().host_strings.len() as u32;
		let mut count = 0;
		for modifier in modifiers {
			let id = self.intern(modifier);
			self.ast().host_strings.push(id);
			count += 1;
		}
		fields.push(("modifiers", Value::Strs(from, count)));
		match rule.value {
			DirectiveValue::Value => fields.push(("value", value)),
			DirectiveValue::Expression { optional, name: own_name } => {
				let expression = match value {
					Value::Bool(true) => None,
					Value::Node(tag) => match self.field_of(tag, "expression") {
						Some(Value::Node(e)) => Some(e),
						_ => None,
					},
					Value::Nodes(list) => {
						let items = self.tree().list(list).to_vec();
						match items[..] {
							[Some(chunk)] if self.host_type(chunk) == "ExpressionTag" => {
								match self.field_of(chunk, "expression") {
									Some(Value::Node(e)) => Some(e),
									_ => None,
								}
							}
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
					None if own_name => {
						let name_start = start + (name.len() - rest.len()) as u32;
						Value::Node(self.ast().add(NodeKind::Identifier { name: name_id }, name_start, end))
					}
					None if optional => Value::Null,
					None => return fail(start, end, Code::Expected, Some("a value")),
				};
				fields.push(("expression", expression));
			}
		}
		for &(flag, on) in &rule.flags {
			fields.push((flag, Value::Bool(on)));
		}
		let node = self.host(rule.ty, start, end, fields, None, true);
		Ok(Some((node, rule.ty, Some(directive_name.to_string()))))
	}

	/// A quoted or bare attribute value: text with expressions, or one expression on its own.
	fn attribute_value(&mut self) -> Result<Value> {
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
			Some(q) => self.sequence(move |w| w.char() == Some(q), "an attribute value")?,
			None => self.sequence(
				|w| {
					w.matches("/>")
						|| w
							.char()
							.is_none_or(|c| is_space(c) || matches!(c, '"' | '\'' | '=' | '<' | '>' | '`'))
				},
				"an attribute value",
			)?,
		};
		if chunks.is_empty() && quote.is_none() {
			return fail(self.at, self.at, Code::Expected, Some("an attribute value"));
		}
		if quote.is_some() {
			self.at += 1;
		}
		if quote.is_some() || chunks.len() > 1 || self.host_type(chunks[0]) == "Text" {
			let list = self.list(&chunks);
			Ok(Value::Nodes(list))
		} else {
			Ok(Value::Node(chunks[0]))
		}
	}

	/// Text and `{expression}` chunks up to where `done` says.
	fn sequence(&mut self, done: impl Fn(&Self) -> bool, place: &str) -> Result<Vec<NodeId>> {
		let mut chunks = Vec::new();
		let mut chunk_start = self.at;
		loop {
			if self.at >= self.len() {
				return fail(self.len(), self.len(), Code::UnexpectedEof, None);
			}
			if done(self) {
				let at = self.at;
				self.flush_text(chunk_start, at, &mut chunks);
				return Ok(chunks);
			}
			if self.eat("{") {
				let start = self.at - 1;
				if self.matches("#") || self.matches("@") {
					return fail(start, start + 1, Code::Placement, Some(&format!("A block or tag in {place}")));
				}
				self.flush_text(chunk_start, start, &mut chunks);
				self.space();
				let expression = self.expression("")?;
				self.space();
				self.expect("}")?;
				let tag = self.host("ExpressionTag", start, self.at, vec![("expression", Value::Node(expression))], None, true);
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

	/// A `{` tag: a block, a branch, a close, a special tag, a declaration or an expression.
	fn tag(&mut self) -> Result<()> {
		let start = self.at;
		self.at += 1;
		self.space();
		if self.eat("#") {
			return self.open_block(start);
		}
		if self.eat(":") {
			return self.branch(start);
		}
		if self.matches("/") && !self.matches("/*") && !self.matches("//") {
			self.at += 1;
			return self.close_block(start);
		}
		if self.eat("@") {
			return self.special(start);
		}
		if let Some(rule) = &self.grammar.declaration {
			if self.word("var") || self.word("interface") || self.word("enum") {
				return fail(self.at, self.at, Code::Placement, Some("A declaration of that kind"));
			}
			if self.word("let") || self.word("const") || self.word("type") {
				let at = self.at;
				let comments = self.tree().comments.len();
				let statement = self.js(JsEntry::Statement, "")?[0];
				let kind = self.tree().node(statement).kind;
				match kind {
					NodeKind::VariableDeclaration { kind, .. } if matches!(kind, VariableKind::Let | VariableKind::Const) => {
						self.space();
						self.expect("}")?;
						let node = self.host(rule.ty, start, self.at, vec![("declaration", Value::Node(statement))], None, true);
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
						return fail(node.start, node.end, Code::Placement, Some("A declaration of that kind"));
					}
				}
			}
		}
		let Some(rule) = &self.grammar.expression else {
			return fail(start, start + 1, Code::UnexpectedToken, None);
		};
		let expression = self.expression("")?;
		self.space();
		self.expect("}")?;
		let node = self.host(rule.ty, start, self.at, vec![("expression", Value::Node(expression))], None, true);
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
		let name_at = self.at;
		let name = self.lowercase_word();
		let Some(rule) = self.grammar.block(name) else {
			return fail(name_at, self.at, Code::Expected, Some("a block name"));
		};
		self.keyword = name_at;
		if !rule.open.items.is_empty() {
			self.require_space()?;
		}
		let mut read = Read::default();
		self.form(&rule.open, &mut read)?;
		self.space();
		self.expect("}")?;
		let Some(body) = read.body.take().or_else(|| rule.open.body.clone()) else {
			return fail(start, start + 1, Code::Placement, Some("A block without a body"));
		};
		let mut inside = Vec::new();
		let mut outside = Vec::new();
		self.declares(&read, &body, &mut inside, &mut outside);
		self.frames.push(Frame::Block {
			start,
			rule,
			fields: read.fields,
			body: (body.field, body.omit),
			nodes: Vec::new(),
			done: Vec::new(),
			inside,
			outside,
			chain: None,
		});
		Ok(())
	}

	fn branch(&mut self, start: u32) -> Result<()> {
		let Some(Frame::Block { rule, .. }) = self.frames.last() else {
			return fail(start, start + 1, Code::Placement, Some("A branch outside its block"));
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
			return fail(start, start + 1, Code::Expected, Some("a branch of the block"));
		};
		let body = branch.form.body.clone().unwrap();
		self.finish_body();
		if let Some(child_field) = body.chain {
			// the branch opens a block of its own inside the parent's field
			self.keyword = at;
			if !branch.form.items.is_empty() {
				self.require_space()?;
			}
			let mut read = Read::default();
			self.form(&branch.form, &mut read)?;
			self.space();
			self.expect("}")?;
			let child = Body {
				field: child_field,
				omit: false,
				chain: None,
				declares: body.declares.clone(),
			};
			let mut inside = Vec::new();
			let mut outside = Vec::new();
			self.declares(&read, &child, &mut inside, &mut outside);
			self.frames.push(Frame::Block {
				start,
				rule,
				fields: read.fields,
				body: (child_field, false),
				nodes: Vec::new(),
				done: Vec::new(),
				inside,
				outside,
				chain: Some(body.field),
			});
			return Ok(());
		}
		let mut read = Read::default();
		self.form(&branch.form, &mut read)?;
		self.space();
		self.expect("}")?;
		let mut inside = Vec::new();
		let mut outside = Vec::new();
		self.declares(&read, &body, &mut inside, &mut outside);
		let Some(Frame::Block {
			fields,
			body: current,
			done,
			inside: all_inside,
			outside: all_outside,
			..
		}) = self.frames.last_mut()
		else {
			unreachable!()
		};
		if done.iter().any(|(field, _)| *field == body.field) {
			return fail(start, start + 1, Code::Duplicate, Some(&format!("{{:{}}}", branch.words.join(" "))));
		}
		fields.extend(read.fields.iter().copied());
		*current = (body.field, body.omit);
		all_inside.extend(inside);
		all_outside.extend(outside);
		Ok(())
	}

	/// The patterns a body declares, by the fields that hold them.
	fn declares(&self, read: &Read, body: &Body, inside: &mut Vec<NodeId>, outside: &mut Vec<NodeId>) {
		for declare in &body.declares {
			for &(field, value) in &read.fields {
				if field != declare.field {
					continue;
				}
				let target = if declare.outside { &mut *outside } else { &mut *inside };
				match value {
					Value::Node(id) | Value::Name(id) => target.push(id),
					Value::Nodes(list) => target.extend(self.tree().list(list).iter().flatten()),
					_ => {}
				}
			}
		}
	}

	/// Closes the open body of the block on top: its nodes become a fragment under its field.
	fn finish_body(&mut self) {
		let Some(Frame::Block { nodes, body, .. }) = self.frames.last_mut() else { unreachable!() };
		let nodes = std::mem::take(nodes);
		let field = body.0;
		let fragment = self.fragment(nodes);
		let Some(Frame::Block { done, .. }) = self.frames.last_mut() else { unreachable!() };
		done.push((field, fragment));
	}

	fn close_block(&mut self, start: u32) -> Result<()> {
		let name_at = self.at;
		let name = self.lowercase_word();
		self.space();
		self.expect("}")?;
		let end = self.at;
		loop {
			let Some(Frame::Block { rule, .. }) = self.frames.last() else {
				return fail(start, start + 1, Code::UnexpectedClose, Some(name));
			};
			if rule.name != name {
				return fail(name_at, name_at + name.len() as u32, Code::UnexpectedClose, Some(name));
			}
			self.finish_body();
			let Some(Frame::Block {
				start: block_start,
				rule,
				fields,
				done,
				inside,
				outside,
				chain,
				..
			}) = self.frames.pop()
			else {
				unreachable!()
			};
			let node = self.block_node(rule, block_start, end, fields, done, inside, outside, chain.is_some());
			match chain {
				Some(field) => {
					let fragment = self.fragment(vec![node]);
					let Some(Frame::Block { done, .. }) = self.frames.last_mut() else { unreachable!() };
					done.push((field, fragment));
				}
				None => {
					self.append(node);
					return Ok(());
				}
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
		done: Vec<(&'static str, NodeId)>,
		inside: Vec<NodeId>,
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
		// the scope opens at the first field it declares; the bodies are inside it
		let declared_inside = |value: Value| match value {
			Value::Node(id) | Value::Name(id) => inside.contains(&id),
			Value::Nodes(list) => self.tree().list(list).iter().flatten().any(|id| inside.contains(id)),
			_ => false,
		};
		let from = fields
			.iter()
			.position(|&(_, value)| declared_inside(value))
			.unwrap_or(fields.len()) as u32;
		let mut bodies: Vec<(&'static str, bool)> = Vec::new();
		collect_bodies(&rule.open, &mut bodies);
		for branch in &rule.branches {
			collect_bodies(&branch.form, &mut bodies);
		}
		for (field, omit) in bodies {
			if let Some(&(_, fragment)) = done.iter().find(|(k, _)| *k == field) {
				fields.push((field, Value::Node(fragment)));
			} else if !omit {
				fields.push((field, Value::Null));
			}
		}
		let inside = self.list(&inside);
		let outside = self.list(&outside);
		let scope = Some(Opens { inside, outside, from });
		self.host(rule.ty, start, end, fields, scope, true)
	}

	fn special(&mut self, start: u32) -> Result<()> {
		let name_at = self.at;
		let name = self.lowercase_word();
		let Some(rule) = self.grammar.tag(name) else {
			return fail(name_at, self.at, Code::Expected, Some("a tag name"));
		};
		let rule: &'a TagRule = rule;
		self.keyword = name_at;
		let mut read = Read::default();
		let lists_names = matches!(rule.form.items.first(), Some(Item::Entry { entry: Entry::Identifiers, .. }));
		if !rule.form.items.is_empty() && !lists_names {
			self.require_space()?;
		}
		self.form(&rule.form, &mut read)?;
		self.space();
		self.expect("}")?;
		let node = self.host(rule.ty, start, self.at, read.fields, None, true);
		self.append(node);
		Ok(())
	}

	/// Runs a form at the cursor: every literal in place, every entry read, the first fitting
	/// alternative of an optional group taken.
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
				Item::Optional(alternatives) => {
					self.space();
					let after = first_literals(&items[i + 1..], follow);
					for alternative in alternatives {
						if self.alternative_here(alternative, &after) {
							self.items(&alternative.items, &after, read)?;
							if let Some(body) = &alternative.body {
								read.body = Some(body.clone());
							}
							break;
						}
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
	/// that follows the group, unless the entry has an opener of its own.
	fn alternative_here(&self, alternative: &Alternative, after: &[&'static str]) -> bool {
		match alternative.items.first() {
			Some(Item::Literal(literal)) => self.literal_here(literal),
			Some(Item::Entry { entry, .. }) => match entry {
				Entry::TypeParameters => self.matches("<"),
				Entry::Params => self.matches("("),
				Entry::Identifier | Entry::Name => self.char().is_some_and(is_id_start),
				_ => !self.matches("}") && self.at < self.len() && !after.iter().any(|stop| self.literal_here(stop)),
			},
			Some(Item::Optional(inner)) => inner.iter().any(|a| self.alternative_here(a, after)),
			None => false,
		}
	}

	fn entry(&mut self, entry: Entry, stops: &[&'static str]) -> Result<Value> {
		let stop = stops.join(" ");
		Ok(match entry {
			Entry::Expression => {
				// a token that continues an expression is JavaScript's before it is the host's
				let stop: Vec<&str> = stops.iter().copied().filter(|s| !matches!(*s, "(" | "[" | "." | "?." | "`")).collect();
				Value::Node(self.js(JsEntry::Expression, &stop.join(" "))?[0])
			}
			Entry::Pattern => Value::Node(self.js(JsEntry::Pattern, &stop)?[0]),
			Entry::Statement => Value::Node(self.js(JsEntry::Statement, &stop)?[0]),
			Entry::TypeParameters => {
				let node = self.js(JsEntry::TypeParameters, &stop)?[0];
				let node = self.tree().node(node);
				Value::Slice(node.start + 1, node.end - 1)
			}
			Entry::Params => {
				let params = self.js(JsEntry::Params, &stop)?;
				let list = self.list(&params);
				Value::Nodes(list)
			}
			Entry::Identifier => Value::Node(self.identifier()?),
			Entry::Name => Value::Name(self.identifier()?),
			Entry::Identifiers => {
				let mut ids = Vec::new();
				loop {
					self.space();
					if self.matches("}") || self.at >= self.len() {
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
				let id = self.js(JsEntry::Pattern, "=")?[0];
				let id_start = self.tree().node(id).start;
				self.space();
				self.expect("=")?;
				self.space();
				let init_at = self.at;
				let init = self.js(JsEntry::Expression, "")?[0];
				let declarator_end = self.at;
				let init_node = *self.tree().node(init);
				if matches!(init_node.kind, NodeKind::SequenceExpression { .. })
					&& !self.src[init_at as usize..init_node.start as usize].contains('(')
				{
					return fail(init_node.start, init_node.end, Code::Expected, Some("a single declaration"));
				}
				self.space();
				let end = self.at;
				let declarator = self.ast().add(NodeKind::VariableDeclarator { id, init: Some(init) }, id_start, declarator_end);
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
		Ok(self.js(JsEntry::Expression, stop)?[0])
	}

	/// The JavaScript at the cursor, read by the parser into the same tree; the cursor moves past it.
	fn js(&mut self, entry: JsEntry, stop: &str) -> Result<Vec<NodeId>> {
		let ast = self.ast.take().unwrap();
		let mut parser = Parser::<E>::new(self.src, self.at, self.options, 0, stop, ast)?;
		let roots = parser.read_entry(entry);
		let end = parser.consumed_end();
		self.ast = Some(parser.finish());
		let roots = roots?;
		self.at = end;
		Ok(roots)
	}

	fn program(&mut self, start: u32, end: u32) -> Result<NodeId> {
		let ast = self.ast.take().unwrap();
		let src = &self.src[..end as usize];
		// the template may declare what the script exports
		let mut options = self.options;
		options.allow_undeclared_exports = true;
		let mut parser = Parser::<E>::new(src, start, options, (end - start) as usize, "", ast)?;
		let program = parser.parse_program();
		self.ast = Some(parser.finish());
		program
	}

	/// An identifier the host reads itself, as a node.
	fn identifier(&mut self) -> Result<NodeId> {
		let start = self.at;
		match self.char() {
			Some(c) if is_id_start(c) => self.at += c.len_utf8() as u32,
			_ => return fail(start, start, Code::Expected, Some("an identifier")),
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

/// The length of a `</textarea>` closer at the start of `rest`, in any case, attributes and all.
fn closing_textarea(rest: &str) -> Option<usize> {
	if !rest.get(..10).is_some_and(|s| s.eq_ignore_ascii_case("</textarea")) {
		return None;
	}
	let tail = &rest[10..];
	if tail.starts_with('>') {
		return Some(11);
	}
	if !tail.starts_with(is_space) {
		return None;
	}
	tail.find('>').map(|i| 10 + i + 1)
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
			Item::Optional(alternatives) => {
				for alternative in alternatives {
					out.extend(first_literals(&alternative.items, &[]));
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
			Item::Optional(alternatives) => {
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
		if let Item::Optional(alternatives) = item {
			for alternative in alternatives {
				if let Some(body) = &alternative.body {
					push(body);
				}
			}
		}
	}
}
