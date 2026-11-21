//! Attaches comments to nodes, for tools that read directives from them or print with them. A
//! comment before a node leads the first node that starts after it; a comment after a node,
//! separated from it by nothing but spaces, commas and closing parens, trails it, unless it is
//! past a comma with code after it on its line, which it then leads.
//! The last node of a block, program, array or object takes everything up to the closing
//! bracket, and an empty one keeps what is inside it as `innerComments`; what is left trails the
//! root. Children are visited in source order. A host's own nodes take no comments: those stay
//! in the list alone.

use crate::ast::{Ast, Attached, Comment, CommentKind, Indent, List, NodeId, NodeKind, Slots, Value};
use crate::host;
use crate::host::grammar::{Name, own};
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

/// Attaches the comments of a document whose root lists them, as Svelte does (see
/// `Grammar::lists_comments`): every script and every piece of JavaScript in the markup is a
/// parse of its own, which attaches what it read, and a script reads every comment the document
/// listed before it too. A parse's walk is Svelte's: a comment before a node leads it; after a
/// node, past nothing but spaces, tabs, commas and closing parens, one trails it, unless the
/// node ends with its parent; the last node of a block, program, array or object takes
/// everything up to the closing bracket; what is left after the root trails it, and in a script
/// all that is left trails the program. A block comment over lines a parse read loses the
/// indentation of its first line from the start of each line, and a markup comment before a
/// script, past blank text only, leads its program as a line comment.
pub fn attach_parses(ast: &mut Ast, source: &str, root: NodeId) {
	let mut attached = std::mem::take(&mut ast.attached);
	attached.clear();
	ast.indents.clear();
	let listed = ast.listed_comments();
	ast.comments.truncate(listed as usize);
	let mut parses = Vec::new();
	let mut scripts = Vec::new();
	gather(ast, source, root, &mut parses, &mut scripts);
	let mut markup = Vec::new();
	if !scripts.is_empty()
		&& let Some(nodes) = root_nodes(ast, root)
	{
		let nodes: Vec<NodeId> = ast.list(nodes).iter().flatten().copied().collect();
		for (script, program) in scripts {
			if let Some(comment) = host::comment_before(ast, source, &nodes, ast.node(script).start) {
				markup.push((program, comment));
			}
		}
	}
	let mut walk = Parses {
		ast,
		source,
		listed,
		attached,
		indents: Vec::new(),
		next: 0,
		end: 0,
		scratch: Vec::new(),
	};
	for parse in parses {
		walk.parse(parse);
	}
	let (mut attached, mut indents) = (walk.attached, walk.indents);
	indents.sort_unstable_by_key(|indent| indent.comment);
	for (program, comment) in markup {
		let at = *ast.node(comment);
		attached.entry(program).leading = crate::ast::Run {
			start: ast.comments.len() as u32,
			len: 1,
		};
		ast.comments.push(Comment {
			kind: CommentKind::Markup,
			start: at.start,
			end: at.end,
		});
	}
	ast.attached = attached;
	ast.indents = indents;
}

/// The list of the document's nodes under `root`.
fn root_nodes(ast: &Ast, root: NodeId) -> Option<List> {
	let NodeKind::Host(index) = ast.node(root).kind else { return None };
	ast.host_rows[index as usize].fields.indices().find_map(|i| match ast.host_vals[i as usize] {
		Value::Node(child) => match host::field(ast, child, own::nodes) {
			Some(Value::Nodes(nodes)) if is_type(ast, child, own::Fragment) => Some(nodes),
			_ => None,
		},
		_ => None,
	})
}

fn is_type(ast: &Ast, id: NodeId, ty: Name) -> bool {
	matches!(ast.node(id).kind, NodeKind::Host(index) if ast.host_rows[index as usize].ty == ty.0)
}

/// One parse of Svelte's, by what it read.
enum Parse {
	/// A script's program.
	Program(NodeId),
	/// An expression or statement read from its first token: the root of its walk.
	Root(NodeId),
	/// Nodes under one Svelte reads them in and then drops, an arrow function's parameters or an
	/// assignment's target, with the comments it read: those from `from` to `to`.
	Under(Vec<NodeId>, u32, u32),
}

/// Each parse under the host node `id`, and each script with its program.
fn gather(ast: &Ast, source: &str, id: NodeId, parses: &mut Vec<Parse>, scripts: &mut Vec<(NodeId, NodeId)>) {
	let NodeKind::Host(index) = ast.node(id).kind else { return };
	for i in ast.host_rows[index as usize].fields.indices() {
		match ast.host_vals[i as usize] {
			Value::Node(child) => match ast.node(child).kind {
				NodeKind::Host(_) => gather(ast, source, child, parses, scripts),
				NodeKind::Program { .. } => {
					parses.push(Parse::Program(child));
					scripts.push((id, child));
				}
				_ => javascript(ast, source, child, parses),
			},
			Value::Nodes(list) => match ast.list(list).first().copied().flatten() {
				Some(first) if matches!(ast.node(first).kind, NodeKind::Host(_)) => {
					for child in ast.list(list).iter().flatten() {
						gather(ast, source, *child, parses, scripts);
					}
				}
				// parameters, read as an arrow function's: what lies between their parentheses
				Some(first) => {
					let nodes: Vec<NodeId> = ast.list(list).iter().flatten().copied().collect();
					let last = *nodes.last().unwrap_or(&first);
					let from = back_over_blanks(ast, source, ast.node(first).start);
					let to = on_over_blanks(ast, source, ast.node(last).end, true);
					parses.push(Parse::Under(nodes, from, to));
				}
				None => {}
			},
			_ => {}
		}
	}
}

/// The parses of JavaScript the host holds at `id`.
fn javascript(ast: &Ast, source: &str, id: NodeId, parses: &mut Vec<Parse>) {
	let node = *ast.node(id);
	match node.kind {
		// a pattern is read as the target of an assignment, the source cut after it
		NodeKind::ObjectPattern { .. } | NodeKind::ArrayPattern { .. } => {
			parses.push(Parse::Under(vec![id], node.start, node.end));
		}
		// `{@const}`: its target read as above, its value as an expression of its own
		NodeKind::VariableDeclaration { declarations, .. } if source[..node.start as usize].ends_with('@') => {
			for declarator in ast.list(declarations).iter().flatten() {
				if let NodeKind::VariableDeclarator { id: target, init } = ast.node(*declarator).kind {
					if matches!(ast.node(target).kind, NodeKind::ObjectPattern { .. } | NodeKind::ArrayPattern { .. }) {
						javascript(ast, source, target, parses);
					}
					if let Some(init) = init {
						parses.push(Parse::Root(init));
					}
				}
			}
		}
		// a directive's name standing for its value is no JavaScript read
		_ if source[..node.start as usize].ends_with(':') => {}
		_ => parses.push(Parse::Root(id)),
	}
}

/// Where the comments before `at` begin that lie past nothing but blanks.
fn back_over_blanks(ast: &Ast, source: &str, mut at: u32) -> u32 {
	let mut i = ast.comments.partition_point(|c| c.start < at);
	while i > 0 {
		let c = ast.comments[i - 1];
		if c.end > at || !blank(source, c.end, at, false) {
			break;
		}
		at = c.start;
		i -= 1;
	}
	at
}

/// Where the comments after `at` end that lie past nothing but blanks, and commas when `commas`.
fn on_over_blanks(ast: &Ast, source: &str, mut at: u32, commas: bool) -> u32 {
	let mut i = ast.comments.partition_point(|c| c.start < at);
	while let Some(c) = ast.comments.get(i) {
		if !blank(source, at, c.start, commas) {
			break;
		}
		at = c.end;
		i += 1;
	}
	at
}

/// Whether nothing but what acorn skips as space lies between `from` and `to`.
fn blank(source: &str, from: u32, to: u32, commas: bool) -> bool {
	source[from as usize..to as usize]
		.chars()
		.all(|c| c.is_whitespace() || c == '\u{feff}' || (commas && c == ','))
}

/// Where a parse dropped a node's parent.
#[derive(Clone, Copy)]
enum Around {
	/// The node is the parse's root.
	Root,
	Node(NodeId),
	/// Svelte's parent, dropped after, ending there.
	Dropped(u32),
}

struct Parses<'a> {
	ast: &'a Ast,
	source: &'a str,
	/// The comments the document lists, before the markup ones.
	listed: u32,
	attached: Slots<Attached>,
	indents: Vec<Indent>,
	/// The parse's next comment, and the end of those it read.
	next: u32,
	end: u32,
	scratch: Vec<NodeId>,
}

impl Parses<'_> {
	fn first_at(&self, at: u32) -> u32 {
		self.ast.comments[..self.listed as usize].partition_point(|c| c.start < at) as u32
	}

	fn parse(&mut self, parse: Parse) {
		match parse {
			Parse::Program(program) => {
				let node = *self.ast.node(program);
				(self.next, self.end) = (0, self.first_at(node.end));
				self.read(self.first_at(node.start), self.end, Some(node.start));
				self.visit(program, Around::Root);
				while self.peek().is_some() {
					self.take(program, Place::Trailing);
				}
			}
			Parse::Root(root) => {
				let node = *self.ast.node(root);
				// acorn keeps an expression's parentheses as a node, which Svelte drops after
				let (open, close) = self.parens(node.start, node.end);
				let from = back_over_blanks(self.ast, self.source, open);
				let to = on_over_blanks(self.ast, self.source, close, false);
				(self.next, self.end) = (self.first_at(from), self.first_at(to));
				self.read(self.next, self.end, None);
				if close == node.end {
					self.visit(root, Around::Root);
					if self.peek().is_some_and(|c| self.start(c) >= node.end) {
						while self.peek().is_some() {
							self.take(root, Place::Trailing);
						}
					}
				} else {
					// the parenthesized expression Svelte drops takes what is before it
					self.next = self.first_at(open);
					self.visit(root, Around::Dropped(close));
				}
			}
			Parse::Under(nodes, from, to) => {
				(self.next, self.end) = (self.first_at(from), self.first_at(to));
				self.read(self.next, self.end, None);
				for node in nodes {
					self.visit(node, Around::Dropped(u32::MAX));
				}
			}
		}
	}

	/// The indentation off each block comment over lines from `from` to `to`, read by a parse of
	/// a script's source from `script` on, blanked before it, or of the markup.
	fn read(&mut self, from: u32, to: u32, script: Option<u32>) {
		for index in from..to {
			let comment = self.ast.comments[index as usize];
			if !comment.is_block() || !self.source[comment.text_range()].contains('\n') {
				continue;
			}
			let line = self.source[..comment.start as usize].rfind('\n').map_or(0, |i| i + 1) as u32;
			let (pad, from) = match script {
				Some(script) if line < script => {
					let blanked = &self.source[line as usize..script as usize];
					(blanked.encode_utf16().count() as u32, script)
				}
				_ => (0, line),
			};
			let indent = self.source.as_bytes()[from as usize..comment.start as usize]
				.iter()
				.take_while(|&&b| b == b' ' || b == b'\t')
				.count() as u32;
			if pad > 0 || indent > 0 {
				self.indents.push(Indent {
					comment: index,
					pad,
					from,
					to: from + indent,
				});
			}
		}
	}

	/// The parentheses around the expression from `start` to `end`: where the outermost opens,
	/// and where its close ends, past blanks and comments between them.
	fn parens(&self, mut start: u32, mut end: u32) -> (u32, u32) {
		let mut depth = 0;
		loop {
			let at = back_over_blanks(self.ast, self.source, start);
			let before = self.source[..at as usize].trim_end_matches(|c: char| c.is_whitespace());
			if !before.ends_with('(') {
				break;
			}
			start = before.len() as u32 - 1;
			depth += 1;
		}
		for _ in 0..depth {
			let at = on_over_blanks(self.ast, self.source, end, false);
			let rest = &self.source[at as usize..];
			let after = rest.trim_start_matches(|c: char| c.is_whitespace());
			if !after.starts_with(')') {
				break;
			}
			end = at + (rest.len() - after.len()) as u32 + 1;
		}
		(start, end)
	}

	fn peek(&self) -> Option<u32> {
		(self.next < self.end).then_some(self.next)
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

	fn visit(&mut self, node: NodeId, parent: Around) {
		if self.peek().is_none() {
			return;
		}
		let n = *self.ast.node(node);
		// acorn starts a program at the start of the source, blanked up to the script
		let start = if matches!(n.kind, NodeKind::Program { .. }) { 0 } else { n.start };
		while self.peek().is_some_and(|c| self.start(c) < start) {
			self.take(node, Place::Leading);
		}
		let Some(next) = self.peek() else { return };
		// a comment past the node reaches a descendant only over commas, parens and blanks
		if self.start(next) < n.end || self.only_separators(n.end, self.start(next)) {
			let base = self.scratch.len();
			self.ast.children(node, &mut self.scratch);
			for i in base..self.scratch.len() {
				let child = self.scratch[i];
				self.visit(child, Around::Node(node));
			}
			self.scratch.truncate(base);
		}
		// the walk goes on into the node's leading comments, and the first of an empty block,
		// program, array or object takes what is inside it: on a copy only, so here nothing does
		if self.is_empty_body(node) && self.attached.get(node).is_some_and(|a| !a.leading.is_empty()) {
			while self.peek().is_some_and(|c| self.start(c) < n.end) {
				self.next += 1;
			}
		}
		let Some(comment) = self.peek() else { return };
		let parent_end = match parent {
			Around::Root => None,
			Around::Node(p) => Some(self.ast.node(p).end),
			Around::Dropped(end) => Some(end),
		};
		if parent_end == Some(n.end) {
			return;
		}
		match parent {
			Around::Node(p) if self.is_last_in(p, node) => {
				let end = self.ast.node(p).end;
				while self.peek().is_some_and(|c| self.start(c) < end) {
					self.take(node, Place::Trailing);
				}
			}
			_ => {
				if n.end <= self.start(comment) && self.only_separators(n.end, self.start(comment)) {
					self.take(node, Place::Trailing);
				}
			}
		}
	}

	fn only_separators(&self, from: u32, to: u32) -> bool {
		self.source.as_bytes()[from as usize..to as usize]
			.iter()
			.all(|b| matches!(b, b',' | b')' | b' ' | b'\t'))
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

	fn is_empty_body(&self, node: NodeId) -> bool {
		self.body_of(node).is_some_and(|list| list.len == 0)
	}

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
			[r#"TypeReference leading=[] trailing=[" a "]"#]
		);
		assert_eq!(
			typescript("interface I { a: string, /* b */ b: number }"),
			[r#"PropertySignature leading=[" b "] trailing=[]"#]
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
