//! Elements and attributes: tags as HTML closes them, Svelte's own elements and components,
//! directives, and attribute values with the expressions in them.

use super::blocks::{Mark, Which};
use super::js::reserved;
use super::nodes::{Name, n};
use super::{Frame, Walker};
use crate::ast::{CommentKind, HostGroup, List, NodeId, NodeKind, Opens, Value};
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
pub(crate) type Attribute = (NodeId, Option<(Name, StrId)>);

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

	fn ty(self) -> Name {
		match self {
			Directive::Bind => n::BindDirective,
			Directive::On => n::OnDirective,
			Directive::Use => n::UseDirective,
			Directive::Class => n::ClassDirective,
			Directive::Style => n::StyleDirective,
			Directive::Transition | Directive::In | Directive::Out => n::TransitionDirective,
			Directive::Animate => n::AnimateDirective,
			Directive::Let => n::LetDirective,
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
	ty: Name,
	/// The field a `this` attribute fills, and whether text is taken there as a string.
	this: Option<(Name, bool)>,
	/// Only at the root, and once.
	root: bool,
	/// A `title` is its own type only inside `svelte:head`.
	title: bool,
	/// A `slot` is its own type only outside an element with `shadowrootmode`.
	slot: bool,
	raw: bool,
	rcdata: bool,
}

const fn rule(ty: Name) -> Rule {
	Rule {
		ty,
		this: None,
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
		"svelte:element" => Rule {
			this: Some((n::tag, true)),
			..rule(n::SvelteElement)
		},
		"svelte:component" => Rule {
			this: Some((n::expression, false)),
			..rule(n::SvelteComponent)
		},
		"svelte:self" => rule(n::SvelteSelf),
		"svelte:window" => Rule {
			root: true,
			..rule(n::SvelteWindow)
		},
		"svelte:document" => Rule {
			root: true,
			..rule(n::SvelteDocument)
		},
		"svelte:body" => Rule {
			root: true,
			..rule(n::SvelteBody)
		},
		"svelte:head" => Rule {
			root: true,
			..rule(n::SvelteHead)
		},
		"svelte:options" => Rule {
			root: true,
			..rule(n::SvelteOptions)
		},
		"svelte:fragment" => rule(n::SvelteFragment),
		"svelte:boundary" => rule(n::SvelteBoundary),
		"title" => Rule {
			title: true,
			..rule(n::TitleElement)
		},
		"slot" => Rule {
			slot: true,
			..rule(n::SlotElement)
		},
		"textarea" => Rule {
			rcdata: true,
			..rule(n::RegularElement)
		},
		"script" | "style" => Rule {
			raw: true,
			..rule(n::RegularElement)
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

	pub(crate) fn attribute_named(&self, id: NodeId, name: &str) -> bool {
		self.host_type(id) == Some(n::Attribute)
			&& self.field_of(id, n::name).and_then(|v| self.string_of(v)) == Some(name)
	}

	/// The one chunk of an attribute's value, when it has one.
	pub(crate) fn attribute_chunk(&self, id: NodeId) -> Option<NodeId> {
		match self.field_of(id, n::value)? {
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
		let Value::Nodes(_) = self.field_of(id, n::value)? else {
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
			n::Attribute,
			start,
			self.at,
			&[(n::name, Value::Str(name_id)), (n::value, value)],
		);
		Ok(Some((node, Some((n::Attribute, name_id)))))
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
				if *ty == n::RegularElement || *ty == n::Component {
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
			None if component_name(name) => rule(n::Component),
			None if valid_name(name) => rule(n::RegularElement),
			None if typing => rule(n::Component),
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
		let plain = n::RegularElement;
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
		self.declared.clear();
		loop {
			let attribute = if script || style {
				self.static_attribute()?
			} else {
				self.attribute()?
			};
			let Some((node, key)) = attribute else { break };
			if let Some((kind, key)) = key {
				let text = self.tree().strings.get(key);
				if kind == n::Attribute && ty == plain && text == "shadowrootmode" {
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
		let declared = std::mem::take(&mut self.declared);
		let mut this = None;
		if let Some((field, text)) = rule.this {
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
			this = Some((field, Value::Node(value)));
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
			self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
			return Ok(());
		}
		if rule.rcdata {
			let nodes = self.sequence(Done::Closing(name), &format!("<{name}>"), JsEntry::Expression)?;
			if let Some(len) = closing_tag(self.rest(), name) {
				self.at += len as u32;
			}
			let end = self.at;
			self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
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
			self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
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
			declared,
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
				if !matches!(self.field_of(id, n::value), Some(Value::Bool(true))) {
					let node = *self.tree().node(id);
					self.report_at(node.start, node.end, Code::Expected, Some("module without a value"))?;
				} else {
					module = true;
				}
			}
		}
		let context = self.intern(if module { "module" } else { "default" });
		let list = self.list(&attributes);
		self.nodes.give(attributes);
		let end = self.at;
		let node = self.host(
			n::Script,
			start,
			end,
			&[
				(n::context, Value::Str(context)),
				(n::content, Value::Node(program)),
				(n::attributes, Value::Nodes(list)),
			],
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

	#[allow(clippy::too_many_arguments)]
	fn element_node(
		&mut self,
		ty: Name,
		(start, end): (u32, u32),
		name: StrId,
		this: Option<(Name, Value)>,
		attributes: Vec<NodeId>,
		nodes: Vec<NodeId>,
		declared: &[NodeId],
	) {
		let list = self.list(&attributes);
		self.nodes.give(attributes);
		let fragment = self.fragment(nodes);
		let mut fields = [(n::name, Value::Str(name)); 4];
		let mut count = 1;
		if let Some(this) = this {
			fields[1] = this;
			count = 2;
		}
		let at = count;
		fields[count] = (n::attributes, Value::Nodes(list));
		fields[count + 1] = (n::fragment, Value::Node(fragment));
		let scope = self.element_scope(at, fragment, declared);
		let node = self.opening(ty, start, end, &fields[..count + 2], Some(scope));
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
			declared,
			..
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
		self.nodes.give(declared);
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
					if *ty != n::RegularElement {
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
				Value::Bool(true)
			};
			if has_value {
				end = self.at;
			}
			let name_id = self.intern(name);
			let node = self.host(
				n::Attribute,
				start,
				end,
				&[(n::name, Value::Str(name_id)), (n::value, value)],
			);
			return Ok(Some((node, Some((n::Attribute, name_id)))));
		};
		let arg_id = self.intern(arg);
		let mut fields = [(n::name, Value::Str(arg_id)); 5];
		let value = if kind == Directive::Style {
			let value = if has_value {
				self.plain_value(JsEntry::Expression)?
			} else {
				Value::Bool(true)
			};
			if has_value {
				end = self.at;
			}
			(n::value, value)
		} else {
			let entry = if kind == Directive::Let {
				JsEntry::Pattern
			} else {
				JsEntry::Expression
			};
			let value = if has_value {
				self.plain_value(entry)?
			} else {
				Value::Bool(true)
			};
			if has_value {
				end = self.at;
			}
			let expression = match value {
				Value::Node(tag) => self.chunk_expression(tag),
				Value::Nodes(list) => match *self.tree().list(list) {
					[Some(chunk)] if self.chunk_expression(chunk).is_some() => self.chunk_expression(chunk),
					[Some(chunk), ..] => {
						let node = *self.tree().node(chunk);
						return self.fail(node.start, node.end, Code::Expected, Some("an expression, not text"));
					}
					_ => None,
				},
				_ => None,
			};
			let expression = match expression {
				Some(e) => Value::Node(e),
				None if kind.named() => {
					let id = self.intern(arg);
					Value::Node(self.ast().add(NodeKind::Identifier { name: id }, arg_start, end))
				}
				None => Value::Null,
			};
			if let (Directive::Let, Value::Node(pattern)) = (kind, expression) {
				self.declared.push(pattern);
			}
			(n::expression, expression)
		};
		fields[1] = value;
		let from = self.ast().host_strings.len() as u32;
		let mut count = 0;
		for modifier in modifiers.split('|').filter(|m| !m.is_empty()) {
			let id = self.intern(modifier);
			self.ast().host_strings.push(id);
			count += 1;
		}
		fields[2] = (n::modifiers, Value::Strs(from, count));
		let mut len = 3;
		if let Some((intro, outro)) = kind.flags() {
			fields[3] = (n::intro, Value::Bool(intro));
			fields[4] = (n::outro, Value::Bool(outro));
			len = 5;
		}
		let node = self.host(kind.ty(), start, end, &fields[..len]);
		let key = match kind {
			Directive::Class | Directive::Style => Some((kind.ty(), self.intern(arg))),
			Directive::Bind => Some((n::Attribute, self.intern(arg))),
			_ => None,
		};
		Ok(Some((node, key)))
	}

	/// An attribute value: text with expressions, or one expression on its own; a slash right
	/// before `>` is the value, `<a href=/>`.
	fn plain_value(&mut self, entry: JsEntry) -> Result<Value> {
		if self.matches("/>") {
			let slash = self.at;
			self.at += 1;
			let text = self.text_chunk(slash, slash + 1, true);
			let list = self.list(&[text]);
			return Ok(Value::Nodes(list));
		}
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
			Value::Nodes(self.list(&chunks))
		} else {
			Value::Node(chunks[0])
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
		let node = self.host(
			n::Attribute,
			start,
			end,
			&[(n::name, Value::Str(name_id)), (n::value, Value::Node(tag))],
		);
		Ok(Some((node, Some((n::Attribute, name_id)))))
	}
}
