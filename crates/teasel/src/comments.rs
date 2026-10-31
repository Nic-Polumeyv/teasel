//! Attaches comments to nodes, for tools that read directives from them or print with them. A
//! comment before a node leads the first node that starts after it; a comment after a node,
//! separated from it by nothing but spaces, commas and closing parens, trails it, unless it is
//! past a comma with code after it on its line, which it then leads.
//! The last node of a block, program, array or object takes everything up to the closing
//! bracket, and an empty one keeps what is inside it as `innerComments`; what is left trails the
//! root. Children are visited in source order. A host's own nodes take no comments: those stay
//! in the list alone.

use crate::ast::{Ast, Attached, List, NodeId, NodeKind, Slots};
use crate::lexer::scan::{is_new_line, is_space};

/// Attaches the comments at or after `from` to the trees under `roots`, in order, replacing any
/// earlier attachment; what is left trails the last one.
pub fn attach(ast: &mut Ast, source: &str, roots: List, from: u32) {
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
		asked: (u32::MAX, false),
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
		let trailing = &mut attached.entry(last).trailing;
		for index in rest..ast.comments.len() as u32 {
			trailing.push(index);
		}
	}
	ast.attached = attached;
}

struct Attacher<'a> {
	ast: &'a Ast,
	source: &'a str,
	next: u32,
	attached: Slots<Attached>,
	scratch: Vec<NodeId>,
	asked: (u32, bool),
}

impl Attacher<'_> {
	fn peek(&self) -> Option<u32> {
		(self.next < self.ast.comments.len() as u32).then_some(self.next)
	}

	fn start(&self, comment: u32) -> u32 {
		self.ast.comments[comment as usize].start
	}

	fn take(&mut self, node: NodeId, place: Place) {
		let entry = self.attached.entry(node);
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
		// a comment past the node reaches a descendant only over commas, parens and blanks
		if self.start(next) < end || self.only_separators(end, self.start(next)) {
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
				let limit = if i + 1 < count {
					self.ast.node(self.scratch[base + i + 1]).start
				} else {
					end
				};
				self.visit(child, Some(node), limit);
			}
			self.scratch.truncate(base);
		}
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
			// the last JavaScript before the host's own syntax resumes takes what lies between
			let mut from = end;
			while let Some(c) = self.peek() {
				let at = self.start(c);
				if at >= limit || (at >= from && !self.only_blanks_or_separators(from, at)) || self.leads_past_comma(c)
				{
					break;
				}
				from = from.max(self.ast.comments[c as usize].end);
				self.take(node, Place::Trailing);
			}
		} else if end <= self.start(comment)
			&& self.only_separators(end, self.start(comment))
			&& !self.leads_past_comma(comment)
		{
			self.take(node, Place::Trailing);
		}
	}

	fn leads_past_comma(&mut self, comment: u32) -> bool {
		// every ancestor asks about the same comment: without this, depth × the run of comments after it
		if self.asked.0 == comment {
			return self.asked.1;
		}
		let leads = self.past_comma(comment) && self.code_follows(comment);
		self.asked = (comment, leads);
		leads
	}

	fn past_comma(&self, comment: u32) -> bool {
		let before = self.source[..self.start(comment) as usize].trim_end_matches(is_space);
		let after_comment = comment > 0 && self.ast.comments[comment as usize - 1].end as usize >= before.len();
		!after_comment && before.ends_with(',')
	}

	fn code_follows(&self, comment: u32) -> bool {
		let mut at = self.ast.comments[comment as usize].end as usize;
		let mut next = comment as usize + 1;
		loop {
			let rest = &self.source[at..];
			at += rest.len() - rest.trim_start_matches(|c| is_space(c) && !is_new_line(c)).len();
			match self.ast.comments.get(next) {
				Some(c) if c.start as usize == at => {
					if self.source[at..c.end as usize].contains(is_new_line) {
						return false;
					}
					at = c.end as usize;
					next += 1;
				}
				// a comma after the comment closes an array hole, and `>` a type list
				_ => {
					return self.source[at..]
						.chars()
						.next()
						.is_some_and(|c| !is_new_line(c) && !matches!(c, ',' | ')' | ']' | '}' | '>'));
				}
			}
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
	use crate::ast::{Ast, NodeId};
	use crate::{Entry, Options};

	/// Every node with comments, in source order: `Kind leading=[..] trailing=[..]`.
	fn attached(ast: &Ast, src: &str) -> Vec<String> {
		let mut nodes: Vec<(NodeId, &crate::ast::Attached)> = (0..ast.nodes.len() as u32)
			.map(NodeId::at)
			.filter_map(|id| ast.attached.get(id).map(|a| (id, a)))
			.collect();
		nodes.sort_by_key(|(id, _)| (ast.node(*id).start, id.index()));
		let values = |run: crate::ast::Run| -> Vec<&str> {
			run.indices()
				.map(|i| &src[ast.comments[i as usize].text_range()])
				.collect()
		};
		nodes
			.into_iter()
			.map(|(id, a)| {
				let kind = format!("{:?}", ast.node(id).kind);
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
		at(Entry::Program, src, 0, 0)
	}

	fn typescript(src: &str) -> Vec<String> {
		at(Entry::Program, src, 0, Options::TYPESCRIPT)
	}

	fn expression(src: &str, offset: u32) -> Vec<String> {
		at(Entry::Expression, src, offset, 0)
	}

	fn at(entry: Entry, src: &str, offset: u32, on: u32) -> Vec<String> {
		let options = Options(Options::MODULE | on);
		let (mut ast, parsed) = crate::parse_at(src, offset, entry, options, "");
		let (roots, _) = parsed.unwrap();
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
			[r#"NumberLiteral leading=[" a "] trailing=[" b "]"#]
		);
	}

	#[test]
	fn past_a_comma_a_comment_leads_the_code_after_it_on_its_line() {
		assert_eq!(
			module("f(a, /* @__PURE__ */ g());"),
			[r#"CallExpression leading=[" @__PURE__ "] trailing=[]"#]
		);
		assert_eq!(
			module("x = {\n\ta: 1, // one\n\tb: 2, /* two */\n};"),
			[
				r#"Property leading=[] trailing=[" one"]"#,
				r#"Property leading=[] trailing=[" two "]"#
			]
		);
		assert_eq!(module("f(a, /* a */)"), [r#"Identifier leading=[] trailing=[" a "]"#]);
		assert_eq!(module("[a, /* a */, ];"), [r#"Identifier leading=[] trailing=[" a "]"#]);
		assert_eq!(
			module("f(a, ) /* a */ + 1;"),
			[r#"Identifier leading=[] trailing=[" a "]"#]
		);
		assert_eq!(
			module("f(a, // a\u{2028});"),
			[r#"Identifier leading=[] trailing=[" a"]"#]
		);
		assert_eq!(
			module("f(a, /* a */\u{a0}\n g());"),
			[r#"Identifier leading=[] trailing=[" a "]"#]
		);
		assert_eq!(
			typescript("f<A, /* a */>(x);"),
			[r#"Extension leading=[] trailing=[" a "]"#]
		);
		assert_eq!(
			typescript("interface I { a: string, /* b */ b: number }"),
			[r#"Extension leading=[" b "] trailing=[]"#]
		);
	}

	#[test]
	fn an_async_rest_takes_the_comment_after_its_annotation() {
		let src = "async (...a: T[] /* c */) => {}";
		let options = Options(Options::MODULE | Options::TYPESCRIPT);
		let (mut ast, parsed) = crate::parse_at(src, 0, Entry::Program, options, "");
		let (roots, _) = parsed.unwrap();
		super::attach(&mut ast, src, roots, 0);
		assert_eq!(attached(&ast, src), [r#"RestElement leading=[] trailing=[" c "]"#]);
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
