//! Elements and attributes: tags as HTML closes them, Svelte's own elements and components,
//! directives, and attribute values with the expressions in them.

use super::blocks::{Mark, Which};
use super::js::reserved;
use super::{Frame, Walker};
use crate::ast::{CommentKind, DirectiveType, ElementType, List, NodeId, NodeKind};
use crate::error::Code;
use crate::interner::StrId;
use crate::lexer::scan::{find_pair, is_space};
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry as JsEntry, Result};

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

/// What reading an attribute gives: its node, and the kind and name it must not repeat on the
/// element, when it has such a name.
pub(crate) type Attribute = (NodeId, Option<(Once, StrId)>);

/// What an element holds once of a name: an attribute, a class directive or a style directive.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Once {
	Attribute,
	Class,
	Style,
}

/// An attribute's value as written: none, one tag, or text and tags.
#[derive(Clone, Copy)]
enum Held {
	Flag,
	Tag(NodeId),
	Chunks(List),
}

/// A directive by the name before its `:`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Directive {
	Bind,
	On,
	Use,
	Class,
	Style,
	Transition,
	In,
	Out,
	Animate,
	Let,
}

impl Directive {
	fn of(name: &str) -> Option<Directive> {
		Some(match name {
			"bind" => Directive::Bind,
			"on" => Directive::On,
			"use" => Directive::Use,
			"class" => Directive::Class,
			"style" => Directive::Style,
			"transition" => Directive::Transition,
			"in" => Directive::In,
			"out" => Directive::Out,
			"animate" => Directive::Animate,
			"let" => Directive::Let,
			_ => return None,
		})
	}

	/// The type of a directive that is neither `style:` nor a transition.
	fn ty(self) -> DirectiveType {
		match self {
			Directive::Bind => DirectiveType::Bind,
			Directive::On => DirectiveType::On,
			Directive::Use => DirectiveType::Use,
			Directive::Class => DirectiveType::Class,
			Directive::Animate => DirectiveType::Animate,
			Directive::Let => DirectiveType::Let,
			_ => unreachable!(),
		}
	}

	/// Without a value, the argument names the expression: `bind:value`.
	fn named(self) -> bool {
		matches!(self, Directive::Bind | Directive::Class | Directive::Let)
	}

	fn flags(self) -> Option<(bool, bool)> {
		match self {
			Directive::Transition => Some((true, true)),
			Directive::In => Some((true, false)),
			Directive::Out => Some((false, true)),
			_ => None,
		}
	}
}

/// An element by its name: its type, and what its rule says of it.
struct Rule {
	ty: ElementType,
	/// Only at the root, and once.
	root: bool,
	/// A `title` is its own type only inside `svelte:head`.
	title: bool,
	/// A `slot` is its own type only outside an element with `shadowrootmode`.
	slot: bool,
	raw: bool,
	rcdata: bool,
}

const fn rule(ty: ElementType) -> Rule {
	Rule {
		ty,
		root: false,
		title: false,
		slot: false,
		raw: false,
		rcdata: false,
	}
}

/// The rule of a tag's own name.
fn own_rule(name: &str) -> Option<Rule> {
	Some(match name {
		"svelte:element" => rule(ElementType::SvelteElement),
		"svelte:component" => rule(ElementType::SvelteComponent),
		"svelte:self" => rule(ElementType::SvelteSelf),
		"svelte:window" => Rule {
			root: true,
			..rule(ElementType::SvelteWindow)
		},
		"svelte:document" => Rule {
			root: true,
			..rule(ElementType::SvelteDocument)
		},
		"svelte:body" => Rule {
			root: true,
			..rule(ElementType::SvelteBody)
		},
		"svelte:head" => Rule {
			root: true,
			..rule(ElementType::SvelteHead)
		},
		"svelte:options" => Rule {
			root: true,
			..rule(ElementType::SvelteOptions)
		},
		"svelte:fragment" => rule(ElementType::SvelteFragment),
		"svelte:boundary" => rule(ElementType::SvelteBoundary),
		"title" => Rule {
			title: true,
			..rule(ElementType::TitleElement)
		},
		"slot" => Rule {
			slot: true,
			..rule(ElementType::SlotElement)
		},
		"textarea" => Rule {
			rcdata: true,
			..rule(ElementType::RegularElement)
		},
		"script" | "style" => Rule {
			raw: true,
			..rule(ElementType::RegularElement)
		},
		_ => return None,
	})
}

/// A component name: capitalized, or a dotted path of identifiers.
fn component_name(name: &str) -> bool {
	let mut chars = name.chars();
	let Some(first) = chars.next() else { return false };
	if first.is_uppercase() {
		return chars.all(|c| is_id_continue(c) || c == '.');
	}
	if !is_id_start(first) || !name.contains('.') {
		return false;
	}
	let mut parts = name.split('.');
	parts.next().is_some_and(|part| part.chars().all(is_id_continue))
		&& parts.all(|part| !part.is_empty() && part.chars().all(is_id_continue))
}

fn is_void(name: &str) -> bool {
	name.starts_with('!')
		|| matches!(
			name,
			"area"
				| "base" | "br"
				| "col" | "command"
				| "embed" | "hr"
				| "img" | "input"
				| "keygen" | "link"
				| "meta" | "param"
				| "source" | "track"
				| "wbr"
		)
}

/// Where a run of text and expressions ends.
#[derive(Clone, Copy)]
enum Done<'n> {
	Quote(char),
	Bare,
	Closing(&'n str),
}

impl<'a> Walker<'a> {
	/// A plain attribute's name and value.
	fn attribute_of(&self, id: NodeId) -> Option<(StrId, Held)> {
		match self.tree().node(id).kind {
			NodeKind::Attribute { name, value } => Some((name, Held::Chunks(value))),
			NodeKind::AttributeTag { name, value } => Some((name, Held::Tag(value))),
			NodeKind::AttributeFlag { name } => Some((name, Held::Flag)),
			_ => None,
		}
	}

	fn attribute_node(&mut self, name: StrId, value: Held, start: u32, end: u32) -> NodeId {
		let kind = match value {
			Held::Flag => NodeKind::AttributeFlag { name },
			Held::Tag(value) => NodeKind::AttributeTag { name, value },
			Held::Chunks(value) => NodeKind::Attribute { name, value },
		};
		self.ast().add(kind, start, end)
	}

	pub(crate) fn attribute_named(&self, id: NodeId, name: &str) -> bool {
		self.attribute_of(id)
			.is_some_and(|(held, _)| self.tree().str(held) == name)
	}

	/// The one chunk of an attribute's value, when it has one.
	pub(crate) fn attribute_chunk(&self, id: NodeId) -> Option<NodeId> {
		match self.attribute_of(id)?.1 {
			Held::Tag(tag) => Some(tag),
			Held::Chunks(list) => match self.tree().list(list) {
				[Some(chunk)] => Some(*chunk),
				_ => None,
			},
			Held::Flag => None,
		}
	}

	/// The text of `name="text"`.
	pub(crate) fn attribute_text(&self, id: NodeId) -> Option<&str> {
		let Held::Chunks(_) = self.attribute_of(id)?.1 else {
			return None;
		};
		self.text_data(self.attribute_chunk(id)?)
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
	fn text_value(&mut self) -> Result<List> {
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
		Ok(self.list(&[text]))
	}

	/// `name`, `name=value` or `name="text"`: a plain attribute, as a script tag or a verbatim
	/// element takes them.
	pub(crate) fn static_attribute(&mut self) -> Result<Option<Attribute>> {
		let start = self.at;
		let name = self.tag_name(true)?;
		if name.is_empty() {
			return Ok(None);
		}
		let mut value = Held::Flag;
		if self.eat("=") {
			self.space();
			value = Held::Chunks(self.text_value()?);
		}
		if self.char().is_some_and(|c| c == '"' || c == '\'') {
			return self.fail(self.at, self.at, Code::Expected, Some("="));
		}
		let name_id = self.intern(name);
		let end = self.at;
		let node = self.attribute_node(name_id, value, start, end);
		Ok(Some((node, Some((Once::Attribute, name_id)))))
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

	// ── elements

	/// Whether the nearest element around the cursor, past blocks and meta elements, is `svelte:head`.
	fn inside_head(&self) -> bool {
		for frame in self.frames.iter().rev() {
			if let Frame::Element { name: span, ty, .. } = frame {
				if &self.src[span.0 as usize..span.1 as usize] == "svelte:head" {
					return true;
				}
				if *ty == ElementType::RegularElement || *ty == ElementType::Component {
					return false;
				}
			}
		}
		false
	}

	pub(super) fn element(&mut self) -> Result<()> {
		let start = self.at;
		self.at += 1;
		if self.eat("!--") {
			let Some(len) = self.rest().find("-->") else {
				let len = self.len();
				return self.fail(len, len, Code::Expected, Some("-->"));
			};
			self.at += len as u32 + 3;
			let end = self.at;
			let node = self.ast().add(NodeKind::MarkupComment, start, end);
			self.append(node);
			return Ok(());
		}
		if self.eat("/") {
			let name = self.tag_name(false)?;
			self.space();
			self.expect(">")?;
			// a void element is never open
			let open = match self.frames.last() {
				Some(Frame::Element { name: span, .. }) => &self.src[span.0 as usize..span.1 as usize] == name,
				_ => false,
			};
			if !open && is_void(name) {
				return self.report_at(
					start,
					start + 1,
					Code::Placement,
					Some("A closing tag of a void element"),
				);
			}
			return self.close_element(start, name);
		}
		let name = self.tag_name(false)?;
		let name_span = (start + 1, self.at);
		let own = own_rule(name);
		if own.is_none() && name.starts_with("svelte:") {
			return self.fail(name_span.0, name_span.1, Code::InvalidName, Some(name));
		}
		let typing = self.recovering() && name.ends_with('.') && component_name(&format!("{name}_"));
		let rule = match own {
			Some(rule) => rule,
			None if component_name(name) => rule(ElementType::Component),
			None if valid_name(name) => rule(ElementType::RegularElement),
			None if typing => rule(ElementType::Component),
			None => return self.fail(name_span.0, name_span.1, Code::InvalidName, Some(name)),
		};
		if rule.root && self.frames.len() > 1 {
			self.report_at(start, start + 1, Code::Placement, Some(name))?;
		}
		if rule.root {
			if self.once.contains(&rule.ty) {
				self.report_at(start, start + 1, Code::Duplicate, Some(name))?;
			} else {
				self.once.push(rule.ty);
			}
		}
		let plain = ElementType::RegularElement;
		let mut ty = rule.ty;
		if rule.title && !self.inside_head() {
			ty = plain;
		}
		if rule.slot
			&& self
				.frames
				.iter()
				.any(|frame| matches!(frame, Frame::Element { shadow: true, .. }))
		{
			ty = plain;
		}
		self.space();
		// the browser closes the open element when this one cannot sit inside it
		if let Some(Frame::Element {
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
		let script = name == "script" && at_root;
		let style = name == "style" && at_root;
		let mut attributes = self.nodes.take();
		let mut seen = std::mem::take(&mut self.seen);
		seen.clear();
		let mut shadow = false;
		loop {
			let attribute = if script || style {
				self.static_attribute()?
			} else {
				self.attribute()?
			};
			let Some((node, key)) = attribute else { break };
			if let Some((kind, key)) = key {
				let text = self.tree().strings.get(key);
				if kind == Once::Attribute && ty == plain && text == "shadowrootmode" {
					shadow = true;
				}
				let this = text == "this";
				if seen.contains(&(kind, key)) {
					let (at, end) = (self.tree().node(node).start, self.tree().node(node).end);
					let text = text.to_string();
					self.report_at(at, end, Code::Duplicate, Some(&text))?;
				}
				if !this {
					seen.push((kind, key));
				}
			}
			attributes.push(node);
			self.space();
		}
		self.seen = seen;
		let mut this = None;
		// the field `this` fills takes text as a string on `svelte:element` alone
		let text = match rule.ty {
			ElementType::SvelteElement => Some(true),
			ElementType::SvelteComponent => Some(false),
			_ => None,
		};
		if let Some(text) = text {
			let position = attributes.iter().position(|&id| self.attribute_named(id, "this"));
			let value = match position {
				None => {
					self.report_at(start, start + 1, Code::Expected, Some("a this attribute"))?;
					self.placeholder(name_span.1, name_span.1)
				}
				Some(position) => {
					let id = attributes.remove(position);
					match self.attribute_chunk(id).and_then(|chunk| self.chunk_expression(chunk)) {
						Some(expression) => expression,
						None => {
							let chunk = self.attribute_chunk(id).filter(|_| text);
							match chunk.map(|chunk| *self.tree().node(chunk)) {
								Some(
									node @ crate::ast::Node {
										kind: NodeKind::Text, ..
									},
								) => {
									let src = self.src;
									let value = self.intern(&src[node.start as usize..node.end as usize]);
									self.ast().add(NodeKind::StringLiteral { value }, node.start, node.end)
								}
								Some(
									node @ crate::ast::Node {
										kind: NodeKind::TextDecoded { data },
										..
									},
								) => self
									.ast()
									.add(NodeKind::StringLiteral { value: data }, node.start, node.end),
								_ => {
									let node = *self.tree().node(id);
									self.report_at(
										node.start,
										node.end,
										Code::Expected,
										Some("an expression as this"),
									)?;
									self.placeholder(node.start, node.end)
								}
							}
						}
					}
				}
			};
			this = Some(value);
		}
		if script {
			return self.script(start, name, attributes);
		}
		if style {
			self.expect(">")?;
			let node = self.style_sheet(start, name, attributes)?;
			let Some(Frame::Root { css, .. }) = self.frames.first_mut() else {
				unreachable!()
			};
			if css.is_none() {
				*css = Some(node);
				return Ok(());
			}
			return self.report_at(start, start + 1, Code::Duplicate, Some(name));
		}
		let self_closing = self.eat("/") || is_void(name);
		let unclosed = !self.eat(">");
		if unclosed {
			let at = self.at;
			self.report_at(at, at, Code::Expected, Some(">"))?;
		}
		let name_id = self.intern(name);
		if self_closing || unclosed {
			let end = self.at;
			let nodes = self.nodes.take();
			self.element_node(ty, (start, end), name_id, this, attributes, nodes);
			return Ok(());
		}
		if rule.rcdata {
			let nodes = self.sequence(Done::Closing(name), &format!("<{name}>"), JsEntry::Expression)?;
			if let Some(len) = closing_tag(self.rest(), name) {
				self.at += len as u32;
			}
			let end = self.at;
			self.element_node(ty, (start, end), name_id, this, attributes, nodes);
			return Ok(());
		}
		if rule.raw {
			let content_start = self.at;
			let (close, end) = self.closing(name);
			self.at = end;
			let node = self.ast().add(NodeKind::Text, content_start, close);
			if close == self.len() {
				let len = self.len();
				self.report_at(len, len, Code::Unclosed, Some(name))?;
			}
			let mut nodes = self.nodes.take();
			nodes.push(node);
			self.element_node(ty, (start, end), name_id, this, attributes, nodes);
			return Ok(());
		}
		let nodes = self.nodes.take();
		self.within_depth(start)?;
		self.frames.push(Frame::Element {
			start,
			name: name_span,
			name_id,
			ty,
			attributes,
			this,
			nodes,
			shadow,
		});
		Ok(())
	}

	/// A script at the root, after its attributes: the instance's or the module's.
	fn script(&mut self, start: u32, name: &str, attributes: Vec<NodeId>) -> Result<()> {
		self.expect(">")?;
		let content_start = self.at;
		let (close, end) = self.closing(name);
		if close == self.len() {
			let len = self.len();
			self.report_at(len, len, Code::Unclosed, Some(name))?;
		}
		let program = self.program(content_start, close)?;
		self.at = end;
		let mut module = false;
		for &id in &attributes {
			if self.attribute_named(id, "context") {
				if self.attribute_text(id) != Some("module") {
					let node = *self.tree().node(id);
					self.report_at(node.start, node.end, Code::Expected, Some("context to be \"module\""))?;
				} else {
					module = true;
				}
			}
			if self.attribute_named(id, "module") {
				if !matches!(self.tree().node(id).kind, NodeKind::AttributeFlag { .. }) {
					let node = *self.tree().node(id);
					self.report_at(node.start, node.end, Code::Expected, Some("module without a value"))?;
				} else {
					module = true;
				}
			}
		}
		let list = self.list(&attributes);
		self.nodes.give(attributes);
		let end = self.at;
		let node = self.ast().add(
			NodeKind::Script {
				content: program,
				attributes: list,
				module,
			},
			start,
			end,
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
		self.report_at(start, start + 1, Code::Duplicate, Some(name))
	}

	fn element_node(
		&mut self,
		ty: ElementType,
		(start, end): (u32, u32),
		name: StrId,
		this: Option<NodeId>,
		attributes: Vec<NodeId>,
		nodes: Vec<NodeId>,
	) {
		let list = self.list(&attributes);
		self.nodes.give(attributes);
		let fragment = self.fragment(nodes);
		let kind = match (ty, this) {
			(ElementType::SvelteElement, Some(tag)) => NodeKind::SvelteElement {
				name,
				tag,
				attributes: list,
				fragment,
			},
			(ElementType::SvelteComponent, Some(expression)) => NodeKind::SvelteComponent {
				name,
				expression,
				attributes: list,
				fragment,
			},
			_ => NodeKind::Element {
				name,
				attributes: list,
				fragment,
				ty,
			},
		};
		let node = self.ast().add(kind, start, end);
		self.append(node);
	}

	/// Closes the element on top of the stack at `end`.
	pub(super) fn close_top(&mut self, end: u32) {
		let Some(Frame::Element {
			start,
			name_id,
			ty,
			attributes,
			this,
			nodes,
			..
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		self.element_node(ty, (start, end), name_id, this, attributes, nodes);
	}

	fn close_element(&mut self, start: u32, name: &str) -> Result<()> {
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
			return self.report_at(start, start + 1, Code::UnexpectedClose, Some(&closed()));
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
					if *ty != ElementType::RegularElement {
						self.report_at(start, start + 1, Code::UnexpectedClose, Some(name))?;
					}
					// the browser closes it here
					self.close_top(start);
				}
				Some(Frame::Block(_)) if self.recovering() => self.pop_unclosed(start)?,
				_ => return self.fail(start, start + 1, Code::UnexpectedClose, Some(&closed())),
			}
		}
	}

	// ── attributes

	/// The expression node of a chunk written as one, `{expression}`.
	fn chunk_expression(&self, chunk: NodeId) -> Option<NodeId> {
		match self.tree().node(chunk).kind {
			NodeKind::ExpressionTag { expression } => Some(expression),
			_ => None,
		}
	}

	/// One attribute: a plain one, a shorthand, a spread, an attachment or a directive.
	fn attribute(&mut self) -> Result<Option<Attribute>> {
		while self.comment_between_attributes() {
			self.space();
		}
		let start = self.at;
		if self.byte() == Some(b'{') {
			return self.among_attributes(start);
		}
		let name = self.tag_name(true)?;
		if name.is_empty() || (self.recovering() && name.starts_with('<')) {
			self.at = start;
			return Ok(None);
		}
		let name_end = self.at;
		// a directive: its kind, its argument's text and start, and its modifiers
		let directive = match name.split_once(':') {
			Some((head, rest)) => match Directive::of(head) {
				Some(kind) => {
					let len = rest.find('|').unwrap_or(rest.len());
					if len == 0 {
						return self.fail(
							start,
							start + name.len() as u32,
							Code::Expected,
							Some("a directive name"),
						);
					}
					let arg_start = start + head.len() as u32 + 1;
					Some((kind, &rest[..len], arg_start, &rest[len..]))
				}
				None => None,
			},
			None => None,
		};
		let mut end = name_end;
		self.space();
		let has_value = self.eat("=");
		if !has_value && self.char().is_some_and(|c| c == '"' || c == '\'') {
			let at = self.at;
			return self.fail(at, at, Code::Expected, Some("="));
		}
		if has_value {
			self.space();
		}
		let Some((kind, arg, arg_start, modifiers)) = directive else {
			let value = if has_value {
				self.plain_value(JsEntry::Expression)?
			} else {
				Held::Flag
			};
			if has_value {
				end = self.at;
			}
			let name_id = self.intern(name);
			let node = self.attribute_node(name_id, value, start, end);
			return Ok(Some((node, Some((Once::Attribute, name_id)))));
		};
		let name = self.intern(arg);
		let entry = if kind == Directive::Let {
			JsEntry::Pattern
		} else {
			JsEntry::Expression
		};
		let value = if has_value {
			self.plain_value(entry)?
		} else {
			Held::Flag
		};
		if has_value {
			end = self.at;
		}
		// a directive other than `style:` holds the one expression of its value
		let mut expression = None;
		if kind != Directive::Style {
			expression = match value {
				Held::Tag(tag) => self.chunk_expression(tag),
				Held::Chunks(list) => match *self.tree().list(list) {
					[Some(chunk)] if self.chunk_expression(chunk).is_some() => self.chunk_expression(chunk),
					[Some(chunk), ..] => {
						let node = *self.tree().node(chunk);
						return self.fail(node.start, node.end, Code::Expected, Some("an expression, not text"));
					}
					_ => None,
				},
				Held::Flag => None,
			};
			if expression.is_none() && kind.named() {
				expression = Some(self.ast().add(NodeKind::Identifier { name }, arg_start, end));
			}
		}
		let from = self.ast().modifiers.len() as u32;
		let mut count = 0;
		for modifier in modifiers.split('|').filter(|m| !m.is_empty()) {
			let id = self.intern(modifier);
			self.ast().modifiers.push(id);
			count += 1;
		}
		let modifiers = [from, count];
		let directive = match (kind, kind.flags()) {
			(Directive::Style, _) => match value {
				Held::Flag => NodeKind::StyleDirectiveFlag { name, modifiers },
				Held::Tag(value) => NodeKind::StyleDirectiveTag { name, value, modifiers },
				Held::Chunks(value) => NodeKind::StyleDirective { name, value, modifiers },
			},
			(_, Some((intro, outro))) => NodeKind::TransitionDirective {
				name,
				expression,
				modifiers,
				intro,
				outro,
			},
			_ => NodeKind::Directive {
				name,
				expression,
				modifiers,
				ty: kind.ty(),
			},
		};
		let node = self.ast().add(directive, start, end);
		let key = match kind {
			Directive::Class => Some((Once::Class, name)),
			Directive::Style => Some((Once::Style, name)),
			Directive::Bind => Some((Once::Attribute, name)),
			_ => None,
		};
		Ok(Some((node, key)))
	}

	/// An attribute value: text with expressions, or one expression on its own; a slash right
	/// before `>` is the value, `<a href=/>`.
	fn plain_value(&mut self, entry: JsEntry) -> Result<Held> {
		if self.matches("/>") {
			let slash = self.at;
			self.at += 1;
			let text = self.text_chunk(slash, slash + 1, true);
			let list = self.list(&[text]);
			return Ok(Held::Chunks(list));
		}
		let quote = self.char().filter(|c| matches!(c, '"' | '\''));
		if let Some(q) = quote {
			self.at += 1;
			if self.char() == Some(q) {
				let at = self.at;
				self.at += 1;
				let text = self.text_chunk(at, at, true);
				let list = self.list(&[text]);
				return Ok(Held::Chunks(list));
			}
		}
		let done = match quote {
			Some(q) => Done::Quote(q),
			None => Done::Bare,
		};
		let chunks = self.sequence(done, "an attribute value", entry)?;
		if chunks.is_empty() && quote.is_none() {
			let at = self.at;
			return self.fail(at, at, Code::Expected, Some("an attribute value"));
		}
		if quote.is_some() && self.char() == quote {
			self.at += 1;
		}
		let value = if quote.is_some() || chunks.len() > 1 || self.text_data(chunks[0]).is_some() {
			Held::Chunks(self.list(&chunks))
		} else {
			Held::Tag(chunks[0])
		};
		self.nodes.give(chunks);
		Ok(value)
	}

	/// Text and expression chunks up to where `done` says.
	fn sequence(&mut self, done: Done, place: &str, entry: JsEntry) -> Result<Vec<NodeId>> {
		let mut chunks = self.nodes.take();
		let mut chunk_start = self.at;
		loop {
			if self.at >= self.len() {
				let len = self.len();
				self.report_at(len, len, Code::UnexpectedEof, None)?;
				let at = self.at;
				self.flush_text(chunk_start, at, &mut chunks);
				return Ok(chunks);
			}
			let finished = match done {
				Done::Quote(q) => self.char() == Some(q),
				Done::Bare => {
					self.matches("/>")
						|| self
							.char()
							.is_none_or(|c| is_space(c) || matches!(c, '"' | '\'' | '=' | '<' | '>' | '`'))
				}
				Done::Closing(name) => closing_tag(self.rest(), name).is_some(),
			};
			if finished {
				let at = self.at;
				self.flush_text(chunk_start, at, &mut chunks);
				return Ok(chunks);
			}
			if self.byte() == Some(b'{') {
				let start = self.at;
				// one expression here, unless the open of something else reads further
				if self.opens_reach(start) > start + 1 {
					return self.fail(
						start,
						start + 1,
						Code::Placement,
						Some(&format!("A block or tag in {place}")),
					);
				}
				self.flush_text(chunk_start, start, &mut chunks);
				self.at = start + 1;
				self.space();
				if self.matches("/>") || self.matches(">") {
					let at = self.at;
					return self.fail(at, at, Code::Expected, Some("}"));
				}
				let roots = self.js(entry, "}")?;
				let expression = self.first(roots);
				self.space();
				self.expect("}")?;
				let end = self.at;
				let tag = self.ast().add(NodeKind::ExpressionTag { expression }, start, end);
				chunks.push(tag);
				chunk_start = self.at;
				continue;
			}
			self.at += self.char().map_or(1, |c| c.len_utf8() as u32);
		}
	}

	/// What a `{` among the attributes starts: a tag that stands there, the shorthand `{name}`,
	/// or under recovery the end of the attributes.
	fn among_attributes(&mut self, start: u32) -> Result<Option<Attribute>> {
		let mut marks = Vec::new();
		let (whole, reached) = self.scan(start, &mut marks);
		let shorthand_end = start + 1;
		let mut found: Option<Mark> = None;
		if reached <= whole && whole > shorthand_end {
			for mark in &marks {
				if mark.which == Which::Open
					&& found.is_none_or(|known| !mark.construct.in_content() && known.construct.in_content())
				{
					found = Some(*mark);
				}
			}
		}
		if let Some(mark) = found
			&& mark.construct.block().is_none()
		{
			self.at = whole;
			self.keyword = mark.word;
			let node = self.tag(mark.construct, start)?;
			if mark.construct.in_content() {
				let at = self.at;
				return self.fail(
					start,
					at,
					Code::Placement,
					Some(&format!("{} among attributes", mark.construct.display())),
				);
			}
			return Ok(Some((node, None)));
		}
		if self.recovering() && (whole > shorthand_end || reached > shorthand_end) {
			// under recovery a block among the attributes ends them
			self.at = start;
			return Ok(None);
		}
		self.at += 1;
		self.space();
		if self.matches("/>") || self.matches(">") {
			let at = self.at;
			return self.fail(at, at, Code::Expected, Some("}"));
		}
		let id_start = self.at;
		let id = self.identifier()?;
		let id_end = self.at;
		let name = &self.src[id_start as usize..id_end as usize];
		if reserved(name) {
			return self.fail(id_start, id_end, Code::ReservedWord, Some(name));
		}
		self.space();
		self.expect("}")?;
		let tag = self
			.ast()
			.add(NodeKind::ExpressionTag { expression: id }, id_start, id_end);
		let name_id = self.intern(name);
		let end = self.at;
		let node = self.attribute_node(name_id, Held::Tag(tag), start, end);
		Ok(Some((node, Some((Once::Attribute, name_id)))))
	}
}
