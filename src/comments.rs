//! Attaches comments to nodes, for tools that read directives from them or print with them. A
//! comment before a node leads the first node that starts after it; a comment after a node,
//! separated from it by nothing but spaces, commas and closing parens, trails it; the last node
//! of a block, program, array or object takes everything up to the closing bracket, and an empty
//! one keeps what is inside it as `innerComments`; what is left trails the root. Children are
//! visited in source order. A host's own nodes take no comments: those stay in the list alone.

use crate::ast::{Ast, Attached, List, NodeId, NodeKind, Walk};
use crate::interner::FastMap;

/// Attaches the comments at or after `from` to the trees under `roots`, in order, replacing any
/// earlier attachment; what is left trails the last one.
pub fn attach<X: Walk>(ast: &mut Ast<X>, source: &str, roots: List, from: u32) {
	let mut attached = std::mem::take(&mut ast.attached);
	attached.clear();
	let first = ast.comments.partition_point(|c| c.start < from);
	let Some(last) = ast.list(roots).last().copied().flatten() else {
		ast.attached = attached;
		return;
	};
	if first == ast.comments.len() {
		ast.attached = attached;
		return;
	}
	let mut attacher = Attacher {
		ast,
		source,
		next: first as u32,
		attached,
		scratch: Vec::new(),
	};
	for i in 0..roots.len {
		let root = attacher.ast.nth(roots, i).unwrap();
		attacher.visit(root, None, u32::MAX);
	}
	let rest = attacher.next;
	let mut attached = attacher.attached;
	let last_node = *ast.node(last);
	if rest < ast.comments.len() as u32
		&& !matches!(last_node.kind, NodeKind::Host(_))
		&& (ast.comments[rest as usize].start >= last_node.end || matches!(last_node.kind, NodeKind::Program { .. }))
	{
		let trailing = &mut attached.entry(last).or_default().trailing;
		for index in rest..ast.comments.len() as u32 {
			trailing.push(index);
		}
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

	/// `limit` is where the parent's own syntax resumes after `node`: the next child, or its end.
	fn visit(&mut self, node: NodeId, parent: Option<NodeId>, limit: u32) {
		let (start, end) = {
			let n = self.ast.node(node);
			(n.start, n.end)
		};
		let host = self.is_host(node);
		while self.peek().is_some_and(|c| self.start(c) < start) {
			if host {
				self.next += 1;
			} else {
				self.take(node, Place::Leading);
			}
		}
		let Some(next) = self.peek() else { return };
		let base = self.scratch.len();
		self.ast.children(node, &mut self.scratch);
		let count = self.scratch.len() - base;
		// a comment past the children reaches one only over commas, parens and blanks; a child
		// can end past its parent (a rest parameter's annotation in an async arrow), so it is the
		// children's reach that counts
		let reach = self.scratch[base..]
			.iter()
			.fold(end, |reach, &c| reach.max(self.ast.node(c).end));
		if self.start(next) < reach || self.only_separators(reach, self.start(next)) {
			if count == 0 && self.body_of(node).is_some() {
				while self.peek().is_some_and(|c| self.start(c) < end) {
					self.take(node, Place::Inner);
				}
			}
			for i in 0..count {
				let child = self.scratch[base + i];
				let limit = if i + 1 < count {
					self.ast.node(self.scratch[base + i + 1]).start
				} else {
					end
				};
				self.visit(child, Some(node), limit);
			}
		}
		self.scratch.truncate(base);
		let Some(comment) = self.peek() else { return };
		let parent_end = parent.map(|p| self.ast.node(p).end);
		if host || parent_end == Some(end) {
			return;
		}
		if parent.is_some_and(|p| self.is_last_in(p, node)) {
			let parent_end = parent_end.unwrap();
			while self.peek().is_some_and(|c| self.start(c) < parent_end) {
				self.take(node, Place::Trailing);
			}
		} else if parent.is_some_and(|p| self.is_host(p)) {
			// the last JavaScript before the host's own syntax resumes takes what lies between,
			// as long as nothing but blanks, commas and closing parens leads to it
			let mut from = end;
			while let Some(c) = self.peek() {
				let at = self.start(c);
				if at >= limit || (at >= from && !self.only_blanks_or_separators(from, at)) {
					break;
				}
				from = from.max(self.ast.comments[c as usize].end);
				self.take(node, Place::Trailing);
			}
		} else if end <= self.start(comment) && self.only_separators(end, self.start(comment)) {
			self.take(node, Place::Trailing);
		}
	}

	fn is_host(&self, node: NodeId) -> bool {
		matches!(self.ast.node(node).kind, NodeKind::Host(_))
	}

	fn only_separators(&self, from: u32, to: u32) -> bool {
		self.source.as_bytes()[from as usize..to as usize]
			.iter()
			.all(|b| matches!(b, b',' | b')' | b' ' | b'\t'))
	}

	fn only_blanks_or_separators(&self, from: u32, to: u32) -> bool {
		self.source.as_bytes()[from as usize..to as usize]
			.iter()
			.all(|b| matches!(b, b',' | b')' | b' ' | b'\t' | b'\n' | b'\r'))
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
	use crate::{Entry, Options};

	/// Every node with comments, in source order: `Kind leading=[..] trailing=[..]`.
	fn attached<X: Walk>(ast: &Ast<X>, src: &str) -> Vec<String> {
		let mut nodes: Vec<(&NodeId, &crate::ast::Attached)> = ast.attached.iter().collect();
		nodes.sort_by_key(|(id, _)| (ast.node(**id).start, id.0));
		let values = |run: crate::ast::Run| -> Vec<&str> {
			run.indices()
				.map(|i| &src[ast.comments[i as usize].text_range()])
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
					format!(" inner={:?}", values(a.inner))
				};
				format!(
					"{kind} leading={:?} trailing={:?}{inner}",
					values(a.leading),
					values(a.trailing)
				)
			})
			.collect()
	}

	fn module(src: &str) -> Vec<String> {
		at(Entry::Program, src, 0)
	}

	fn expression(src: &str, offset: u32) -> Vec<String> {
		at(Entry::Expression, src, offset)
	}

	fn at(entry: Entry, src: &str, offset: u32) -> Vec<String> {
		let options = Options {
			module: true,
			..Options::default()
		};
		let (mut ast, roots, _) = crate::parse_at(src, offset, None, entry, options, "").unwrap();
		super::attach(&mut ast, src, roots, offset);
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

	#[cfg(feature = "typescript")]
	#[test]
	fn a_child_past_its_parent_still_takes_its_comment() {
		let src = "async (...a: T[] /* c */) => {}";
		let options = Options {
			module: true,
			..Options::default()
		};
		let (mut ast, roots, _) = crate::typescript::parse_at(src, 0, None, Entry::Program, options, "").unwrap();
		super::attach(&mut ast, src, roots, 0);
		assert_eq!(attached(&ast, src), [r#"Extension leading=[] trailing=[" c "]"#]);
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
			[r#"Identifier leading=[" c "] trailing=[]"#]
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
