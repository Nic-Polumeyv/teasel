//! Attaches comments to nodes, for tools that read directives from them or print with them. A
//! comment before a node leads the first node that starts after it; a comment after a node,
//! separated from it by nothing but spaces, commas and closing parens, trails it; the last node
//! of a block, program, array or object takes everything up to the closing bracket; what is left
//! trails the root. Children are visited in source order.

use crate::ast::{Ast, NodeId, NodeKind, Walk};

/// Attaches the comments at or after `from` to the tree under `root`.
pub fn attach<X: Walk>(ast: &mut Ast<X>, source: &str, root: NodeId, from: u32) {
	let first = ast.comments.partition_point(|c| c.start < from);
	if first == ast.comments.len() {
		return;
	}
	let mut attacher = Attacher {
		ast,
		source,
		next: first as u32,
	};
	attacher.visit(root, None);
	let rest = attacher.next;
	let root_node = *ast.node(root);
	if rest < ast.comments.len() as u32
		&& (ast.comments[rest as usize].start >= root_node.end || matches!(root_node.kind, NodeKind::Program { .. }))
	{
		let all = rest..ast.comments.len() as u32;
		ast.attached.entry(root).or_default().trailing.extend(all);
	}
}

struct Attacher<'a, X> {
	ast: &'a mut Ast<X>,
	source: &'a str,
	next: u32,
}

impl<X: Walk> Attacher<'_, X> {
	fn peek(&self) -> Option<u32> {
		(self.next < self.ast.comments.len() as u32).then_some(self.next)
	}

	fn start(&self, comment: u32) -> u32 {
		self.ast.comments[comment as usize].start
	}

	fn take(&mut self, node: NodeId, trailing: bool) {
		let entry = self.ast.attached.entry(node).or_default();
		if trailing {
			entry.trailing.push(self.next);
		} else {
			entry.leading.push(self.next);
		}
		self.next += 1;
	}

	fn visit(&mut self, node: NodeId, parent: Option<NodeId>) {
		let (start, end) = {
			let n = self.ast.node(node);
			(n.start, n.end)
		};
		while self.peek().is_some_and(|c| self.start(c) < start) {
			self.take(node, false);
		}
		let mut children = Vec::new();
		self.ast.children(node, &mut children);
		for child in children {
			self.visit(child, Some(node));
		}
		let Some(comment) = self.peek() else { return };
		let parent_end = parent.map(|p| self.ast.node(p).end);
		if parent_end == Some(end) {
			return;
		}
		if parent.is_some_and(|p| self.is_last_in(p, node)) {
			let parent_end = parent_end.unwrap();
			while self.peek().is_some_and(|c| self.start(c) < parent_end) {
				self.take(node, true);
			}
		} else if end <= self.start(comment)
			&& self.source.as_bytes()[end as usize..self.start(comment) as usize]
				.iter()
				.all(|b| matches!(b, b',' | b')' | b' ' | b'\t'))
		{
			self.take(node, true);
		}
	}

	/// Whether `node` closes the body of a block, program, array or object literal.
	fn is_last_in(&self, parent: NodeId, node: NodeId) -> bool {
		let list = match self.ast.node(parent).kind {
			NodeKind::BlockStatement { body } | NodeKind::Program { body, .. } => body,
			NodeKind::ArrayExpression { elements } => elements,
			NodeKind::ObjectExpression { properties } => properties,
			_ => return false,
		};
		self.ast.list(list).last() == Some(&Some(node))
	}
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
				format!(
					"{kind} leading={:?} trailing={:?}",
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
	fn the_last_node_takes_the_rest() {
		assert_eq!(
			module("let x; /* a */\n/* b */"),
			[r#"VariableDeclaration leading=[] trailing=[" a ", " b "]"#]
		);
		assert_eq!(module("/* only */"), [r#"Program leading=[] trailing=[" only "]"#]);
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
