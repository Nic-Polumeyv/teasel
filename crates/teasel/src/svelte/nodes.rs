//! Svelte's nodes as the walker writes them: each a row of the tree, its type and fields named
//! by number, numbered by shape; and what a walk over them follows.

use std::borrow::Cow;

use super::Walker;
use super::entities::decode;
use crate::ast::{HostRow, List, NodeId, NodeKind, Opens, Run, Value};

/// A name a node type or a field has, by its number in `svelte::NAMES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Name(pub(crate) u32);

impl Name {
	/// No name: a node without a type, a style element's content; a block whose body is not filed.
	pub(crate) const NONE: Name = Name(u32::MAX);
}

macro_rules! named {
	($($name:ident)*) => {
		/// The names of Svelte's types and fields, a stylesheet's among them, by their numbers.
		#[allow(non_upper_case_globals, dead_code)]
		pub(crate) mod n {
			use super::Name;
			#[allow(non_camel_case_types)]
			#[repr(u32)]
			enum Number {
				$($name),*
			}
			$(pub(crate) const $name: Name = Name(Number::$name as u32);)*
			pub(super) const SPELLED: &[&str] = &[$(stringify!($name)),*];
		}
	};
}

named! {
	Fragment nodes Text raw data Comment Attribute name value attributes fragment expression modifiers
	Script context content StyleSheet children comments styles comment CSSComment position Atrule
	prelude block Rule SelectorList ComplexSelector RelativeSelector combinator selectors Combinator
	TypeSelector namespace IdSelector ClassSelector NestingSelector PseudoClassSelector
	PseudoElementSelector args AttributeSelector matcher flags Nth Percentage Block Declaration property
	branches
	tag SvelteElement SvelteComponent SvelteSelf SvelteWindow SvelteDocument SvelteBody SvelteHead
	SvelteOptions SvelteFragment SvelteBoundary TitleElement SlotElement RegularElement Component
	BindDirective OnDirective UseDirective ClassDirective StyleDirective intro outro TransitionDirective
	AnimateDirective LetDirective test consequent ElseIf alternate IfBlock fallback index key body EachBlock
	then error r#catch pending AwaitBlock KeyBlock typeParams parameters SnippetBlock HtmlTag identifiers
	DebugTag declaration ConstTag RenderTag AttachTag SpreadAttribute DeclarationTag ExpressionTag css js
	options module instance Root
}

const COUNT: usize = n::SPELLED.len();

/// Every name by its number, as JavaScript spells it.
pub(crate) const NAMES: [&str; COUNT] = {
	let mut out = [""; COUNT];
	let mut i = 0;
	while i < COUNT {
		let name = n::SPELLED[i];
		out[i] = match name.as_bytes() {
			[b'r', b'#', ..] => name.split_at(2).1,
			_ => name,
		};
		i += 1;
	}
	out
};

/// The shapes of Svelte's nodes, numbered for good as the walker meets them: a node's
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

/// Every type of node a component holds, each with the fields that hold nodes or lists of them:
/// what a walk over a component follows.
pub(crate) fn children() -> Vec<(&'static str, Vec<&'static str>)> {
	let element = ["attributes", "fragment"];
	let mut out: Vec<(&'static str, Vec<&'static str>)> = vec![
		("Root", vec!["css", "js", "module", "instance", "fragment"]),
		("Fragment", vec!["nodes"]),
		("Text", vec![]),
		("Comment", vec![]),
		("SvelteElement", vec!["attributes", "fragment", "tag"]),
		("SvelteComponent", vec!["attributes", "fragment", "expression"]),
	];
	for ty in [
		"SvelteSelf",
		"SvelteWindow",
		"SvelteDocument",
		"SvelteBody",
		"SvelteHead",
		"SvelteOptions",
		"SvelteFragment",
		"SvelteBoundary",
		"TitleElement",
		"SlotElement",
		"RegularElement",
		"Component",
	] {
		out.push((ty, element.to_vec()));
	}
	out.push(("Attribute", vec!["value"]));
	out.push(("Script", vec!["content", "attributes"]));
	for ty in [
		"BindDirective",
		"OnDirective",
		"UseDirective",
		"ClassDirective",
		"StyleDirective",
		"TransitionDirective",
		"AnimateDirective",
		"LetDirective",
	] {
		out.push((ty, vec![if ty == "StyleDirective" { "value" } else { "expression" }]));
	}
	out.extend([
		("ElseIf", vec!["test", "consequent"]),
		("IfBlock", vec!["test", "consequent", "branches", "alternate"]),
		(
			"EachBlock",
			vec!["expression", "context", "index", "key", "body", "fallback"],
		),
		(
			"AwaitBlock",
			vec!["expression", "value", "then", "error", "catch", "pending"],
		),
		("KeyBlock", vec!["expression", "fragment"]),
		("SnippetBlock", vec!["expression", "parameters", "fragment"]),
		("HtmlTag", vec!["expression"]),
		("DebugTag", vec!["identifiers"]),
		("ConstTag", vec!["declaration"]),
		("RenderTag", vec!["expression"]),
		("AttachTag", vec!["expression"]),
		("SpreadAttribute", vec!["expression"]),
		("DeclarationTag", vec!["declaration"]),
		("ExpressionTag", vec!["expression"]),
	]);
	out.extend(super::css::children(true));
	out
}

/// Every type of node a stylesheet holds, each with the fields that hold nodes.
pub(crate) fn stylesheet_children() -> Vec<(&'static str, Vec<&'static str>)> {
	super::css::children(false)
}

impl Walker<'_> {
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
		let kind = match decode(raw, attribute) {
			Cow::Borrowed(_) => NodeKind::Text,
			Cow::Owned(data) => NodeKind::TextDecoded {
				data: self.intern(&data),
			},
		};
		self.ast().add(kind, start, end)
	}

	/// A text's `data`: its source, or what its character references decode to.
	pub(crate) fn text_data(&self, id: NodeId) -> Option<&str> {
		let node = self.tree().node(id);
		match node.kind {
			NodeKind::Text => Some(&self.src[node.start as usize..node.end as usize]),
			NodeKind::TextDecoded { data } => Some(self.tree().str(data)),
			_ => None,
		}
	}

	pub(crate) fn flush_text(&mut self, from: u32, to: u32, chunks: &mut Vec<NodeId>) {
		if to > from {
			let text = self.text_chunk(from, to, true);
			chunks.push(text);
		}
	}

	/// The nodes as a fragment: positioned for the walks that order nodes, though none are written,
	/// at the cursor when there are none. The buffer is given back.
	pub(super) fn fragment(&mut self, nodes: Vec<NodeId>) -> NodeId {
		let list = self.list(&nodes);
		let (start, end) = match (nodes.first(), nodes.last()) {
			(Some(&first), Some(&last)) => (self.tree().node(first).start, self.tree().node(last).end),
			_ => (self.at, self.at),
		};
		self.nodes.give(nodes);
		self.ast().add(NodeKind::Fragment { nodes: list }, start, end)
	}

	pub(crate) fn list(&mut self, nodes: &[NodeId]) -> List {
		self.ast().add_list_from(nodes.iter().map(|&id| Some(id)))
	}
}
