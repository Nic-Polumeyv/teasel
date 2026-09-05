//! Attaches comments to nodes, for tools that read directives from them or print with them. A
//! comment before a node leads the first node that starts after it; a comment after a node,
//! separated from it by nothing but spaces, commas and closing parens, trails it; the last node
//! of a block, program, array or object takes everything up to the closing bracket, and an empty
//! one keeps what is inside it as `innerComments`; what is left trails the root. Children are
//! visited in source order.

use crate::ast::{Ast, Attached, List, NodeId, NodeKind, Walk};
use crate::interner::FastMap;

/// Attaches the comments at or after `from` to the tree under `root`, replacing any earlier
/// attachment.
pub fn attach<X: Walk>(ast: &mut Ast<X>, source: &str, root: NodeId, from: u32) {
	attach_all(ast, source, &[root], from);
}

/// Attaches comments to several trees in order, such as a parameter list; what is left trails
/// the last one.
pub fn attach_all<X: Walk>(ast: &mut Ast<X>, source: &str, roots: &[NodeId], from: u32) {
	ast.attached.clear();
	let first = ast.comments.partition_point(|c| c.start < from);
	let Some(&last) = roots.last() else { return };
	if first == ast.comments.len() {
		return;
	}
	let mut attacher = Attacher {
		ast,
		source,
		next: first as u32,
		attached: FastMap::default(),
		scratch: Vec::new(),
	};
	for &root in roots {
		attacher.visit(root, None);
	}
	let rest = attacher.next;
	let mut attached = attacher.attached;
	let last_node = *ast.node(last);
	if rest < ast.comments.len() as u32
		&& (ast.comments[rest as usize].start >= last_node.end || matches!(last_node.kind, NodeKind::Program { .. }))
	{
		let all = rest..ast.comments.len() as u32;
		attached.entry(last).or_default().trailing.extend(all);
	}
	ast.attached = attached;
}

struct Attacher<'a, X> {
	ast: &'a Ast<X>,
	source: &'a str,
	next: u32,
	attached: FastMap<NodeId, Attached>,
	scratch: Vec<NodeId>,
}

impl<X: Walk> Attacher<'_, X> {
	fn peek(&self) -> Option<u32> {
		(self.next < self.ast.comments.len() as u32).then_some(self.next)
	}

	fn start(&self, comment: u32) -> u32 {
		self.ast.comments[comment as usize].start
	}

	fn take(&mut self, node: NodeId, place: Place) {
		let entry = self.attached.entry(node).or_default();
		match place {
			Place::Leading => entry.leading.push(self.next),
			Place::Trailing => entry.trailing.push(self.next),
			Place::Inner => entry.inner.push(self.next),
		}
		self.next += 1;
	}

	fn visit(&mut self, node: NodeId, parent: Option<NodeId>) {
		let (start, end) = {
			let n = self.ast.node(node);
			(n.start, n.end)
		};
		while self.peek().is_some_and(|c| self.start(c) < start) {
			self.take(node, Place::Leading);
		}
		let base = self.scratch.len();
		self.ast.children(node, &mut self.scratch);
		let count = self.scratch.len() - base;
		if count == 0 && self.body_of(node).is_some() {
			while self.peek().is_some_and(|c| self.start(c) < end) {
				self.take(node, Place::Inner);
			}
		}
		for i in 0..count {
			let child = self.scratch[base + i];
			self.visit(child, Some(node));
		}
		self.scratch.truncate(base);
		let Some(comment) = self.peek() else { return };
		let parent_end = parent.map(|p| self.ast.node(p).end);
		if parent_end == Some(end) {
			return;
		}
		if parent.is_some_and(|p| self.is_last_in(p, node)) {
			let parent_end = parent_end.unwrap();
			while self.peek().is_some_and(|c| self.start(c) < parent_end) {
				self.take(node, Place::Trailing);
			}
		} else if end <= self.start(comment)
			&& self.source.as_bytes()[end as usize..self.start(comment) as usize]
				.iter()
				.all(|b| matches!(b, b',' | b')' | b' ' | b'\t'))
		{
			self.take(node, Place::Trailing);
		}
	}

	/// The list a block, program, array or object literal encloses in brackets.
	fn body_of(&self, node: NodeId) -> Option<List> {
		match self.ast.node(node).kind {
			NodeKind::BlockStatement { body } | NodeKind::Program { body, .. } => Some(body),
			NodeKind::ArrayExpression { elements } => Some(elements),
			NodeKind::ObjectExpression { properties } => Some(properties),
			_ => None,
		}
	}

	/// Whether `node` closes the body of its parent.
	fn is_last_in(&self, parent: NodeId, node: NodeId) -> bool {
		self.body_of(parent)
			.is_some_and(|list| self.ast.list(list).last() == Some(&Some(node)))
	}
}

#[derive(Clone, Copy)]
enum Place {
	Leading,
	Trailing,
	Inner,
}

#[cfg(test)]
mod tests {
	use crate::ast::{Ast, NodeId, Walk};
	use crate::{Options, parse, parse_expression_at};

	/// Every node with comments, in source order: `Kind leading=[..] trailing=[..]`.
	fn attached<X: Walk>(ast: &Ast<X>, src: &str) -> Vec<String> {
		let mut nodes: Vec<(&NodeId, &crate::ast::Attached)> = ast.attached.iter().collect();
		nodes.sort_by_key(|(id, _)| (ast.node(**id).start, id.0));
		let values = |indices: &[u32]| -> Vec<&str> {
			indices
				.iter()
				.map(|&i| &src[ast.comments[i as usize].text_range()])
				.collect()
		};
		nodes
			.into_iter()
			.map(|(id, a)| {
				let kind = format!("{:?}", ast.node(*id).kind);
				let kind = kind.split([' ', '(']).next().unwrap();
				let inner = if a.inner.is_empty() {
					String::new()
				} else {
					format!(" inner={:?}", values(&a.inner))
				};
				format!(
					"{kind} leading={:?} trailing={:?}{inner}",
					values(&a.leading),
					values(&a.trailing)
				)
			})
			.collect()
	}

	fn module(src: &str) -> Vec<String> {
		let mut ast = parse(
			src,
			Options {
				module: true,
				..Options::default()
			},
		)
		.unwrap();
		let root = ast.last();
		super::attach(&mut ast, src, root, 0);
		attached(&ast, src)
	}

	fn expression(src: &str, offset: u32) -> Vec<String> {
		let options = Options {
			module: true,
			preserve_parens: true,
			..Options::default()
		};
		let (mut ast, root) = parse_expression_at(src, offset, options).unwrap();
		super::attach(&mut ast, src, root, offset);
		attached(&ast, src)
	}

	#[test]
	fn leading_and_trailing() {
		assert_eq!(
			module("/* a */ let x = 1; // b\nlet y;"),
			[r#"VariableDeclaration leading=[" a "] trailing=[" b"]"#]
		);
		assert_eq!(
			module("f(a /* a */, b);"),
			[r#"Identifier leading=[] trailing=[" a "]"#]
		);
		assert_eq!(
			module("{ x; /* a */ /* b */\n // c\n }"),
			[r#"ExpressionStatement leading=[] trailing=[" a ", " b ", " c"]"#]
		);
		assert_eq!(
			module("[1, /* a */ 2 /* b */]"),
			[
				r#"NumberLiteral leading=[] trailing=[" a "]"#,
				r#"NumberLiteral leading=[] trailing=[" b "]"#
			]
		);
	}

	#[test]
	fn empty_containers_keep_their_inside() {
		assert_eq!(
			module("function f() /* a */ { /* b */ }"),
			[r#"BlockStatement leading=[" a "] trailing=[] inner=[" b "]"#]
		);
		assert_eq!(
			module("x = [ /* a */ ];"),
			[r#"ArrayExpression leading=[] trailing=[] inner=[" a "]"#]
		);
	}

	#[test]
	fn the_last_node_takes_the_rest() {
		assert_eq!(
			module("let x; /* a */\n/* b */"),
			[r#"VariableDeclaration leading=[] trailing=[" a ", " b "]"#]
		);
		assert_eq!(
			module("/* only */"),
			[r#"Program leading=[] trailing=[] inner=[" only "]"#]
		);
	}

	#[test]
	fn expression_root_keeps_what_follows() {
		assert_eq!(
			expression("{x /* a */}", 1),
			[r#"Identifier leading=[] trailing=[" a "]"#]
		);
		assert_eq!(
			expression("{a ? /* c */ (b) : d}", 1),
			[r#"ParenthesizedExpression leading=[" c "] trailing=[]"#]
		);
	}

	#[test]
	fn source_order() {
		assert_eq!(
			module("switch (x) { case /* a */ 1: y; }"),
			[r#"NumberLiteral leading=[" a "] trailing=[]"#]
		);
		assert_eq!(module("`${/* a */ x}`"), [r#"Identifier leading=[" a "] trailing=[]"#]);
		assert_eq!(module("l /* a */ : x;"), [r#"Identifier leading=[] trailing=[" a "]"#]);
	}
}
