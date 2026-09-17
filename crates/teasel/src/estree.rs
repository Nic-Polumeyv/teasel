//! Serializes an `Ast` to ESTree as JSON text, and maps its positions to what JavaScript counts.

use crate::ast::{Ast, List, NodeId, NodeKind, Value};
use crate::handed::Handed;
use crate::interner::StrId;
use crate::layout::Ty;
use crate::names::{Name, c};
use crate::parser::Entry;
use crate::recipe::{Op, Slot};
use crate::scopes::Role;
use std::fmt::Write;

/// How an extension's data serializes: its own nodes, and the keys it adds to JavaScript nodes.
pub trait Emit: crate::ast::Walk {
	/// Emits one of the extension's own nodes and closes it, or what erasing puts in its place.
	fn node(&self, w: &mut Writer<Self>, id: NodeId, index: u32);
	fn extras(&self, _w: &mut Writer<Self>, _id: NodeId) {}
	/// Whether erasing leaves nothing of a node in a list: type-only declarations and imports.
	fn erased(&self, _ast: &Ast<Self>, _id: NodeId) -> bool {
		false
	}
}

/// What the answer carries beyond the tree.
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
	/// The answer lists every comment read.
	pub comments: bool,
	/// Nodes carry their scope and identifiers their binding, and the answer lists both tables.
	pub scopes: bool,
	/// TypeScript is erased: annotations, type-only declarations and imports go, assertions give
	/// way to their expression, and what erasure cannot express is listed as `typescript`.
	pub erase: bool,
	/// The answer lists the errors recovered from, as `errors`.
	pub errors: bool,
}

impl Emit for () {
	fn node(&self, _w: &mut Writer<Self>, _id: NodeId, _index: u32) {
		unreachable!("the JavaScript parser adds no extension nodes")
	}
}

/// JSON text.
pub struct Json {
	out: String,
	/// Whether the next entry of the open container is its first.
	first: bool,
	/// The open containers, `true` for a list.
	stack: Vec<bool>,
}

impl Default for Json {
	fn default() -> Self {
		Json {
			out: String::new(),
			first: true,
			stack: Vec::new(),
		}
	}
}

impl Json {
	pub fn finish(self) -> String {
		self.out
	}

	fn sep(&mut self) {
		if !self.first {
			self.out.push(',');
		}
		self.first = false;
	}

	fn open(&mut self, list: bool) {
		self.sep();
		self.out.push(if list { '[' } else { '{' });
		self.stack.push(list);
		self.first = true;
	}
}

impl Json {
	pub(crate) fn ints(&mut self, values: &[u32]) {
		self.list();
		for &value in values {
			self.int(value);
		}
		self.end();
	}

	pub(crate) fn begin(&mut self, ty: Name) {
		self.open(false);
		self.out.push_str("\"type\":\"");
		self.out.push_str(ty.text);
		self.out.push('"');
		self.first = false;
	}

	pub(crate) fn object(&mut self) {
		self.open(false);
	}

	pub(crate) fn list(&mut self) {
		self.open(true);
	}

	pub(crate) fn end(&mut self) {
		self.out.push(if self.stack.pop() == Some(true) { ']' } else { '}' });
		self.first = false;
	}

	pub(crate) fn key(&mut self, key: Name) {
		self.sep();
		self.out.push('"');
		self.out.push_str(key.text);
		self.out.push_str("\":");
		self.first = true;
	}

	pub(crate) fn int(&mut self, value: u32) {
		self.sep();
		push_int(&mut self.out, value);
	}

	pub(crate) fn float(&mut self, value: f64) {
		self.sep();
		write_number(&mut self.out, value);
	}

	pub(crate) fn bool(&mut self, value: bool) {
		self.sep();
		self.out.push_str(if value { "true" } else { "false" });
	}

	pub(crate) fn null(&mut self) {
		self.sep();
		self.out.push_str("null");
	}

	pub(crate) fn str(&mut self, value: Name) {
		self.text(value.text);
	}

	pub(crate) fn text(&mut self, value: &str) {
		self.sep();
		write_json_string(&mut self.out, value);
	}

	// the two entries every node has, written in one piece: a measurable share of the text
	fn span(&mut self, start: u32, end: u32) {
		self.sep();
		self.out.push_str("\"start\":");
		push_int(&mut self.out, start);
		self.out.push_str(",\"end\":");
		push_int(&mut self.out, end);
	}

	fn loc(&mut self, start_line: u32, start_column: u32, end_line: u32, end_column: u32) {
		self.sep();
		self.out.push_str("\"loc\":{\"start\":{\"line\":");
		push_int(&mut self.out, start_line);
		self.out.push_str(",\"column\":");
		push_int(&mut self.out, start_column);
		self.out.push_str("},\"end\":{\"line\":");
		push_int(&mut self.out, end_line);
		self.out.push_str(",\"column\":");
		push_int(&mut self.out, end_column);
		self.out.push_str("}}");
	}
}

/// The answer's words, read in place by a front end that takes the allocation over.
pub type Words = crate::handed::Handed<u32>;

/// Serializes an answer as JSON: the roots as `node`, one node or the patterns of a parameter
/// list, then `end` and what the options add.
pub fn answer<X: Emit>(
	ast: &Ast<X>,
	entry: Entry,
	roots: List,
	end: u32,
	source: &str,
	positions: &Positions,
	output: Output,
) -> String {
	let mut w = Writer::new(ast, source, positions);
	w.output = output;
	w.sink.object();
	if entry == Entry::Params {
		w.list(c!("node"), roots);
	} else {
		w.field(c!("node"), ast.list(roots)[0].unwrap());
	}
	w.key(c!("end"));
	let end = w.positions.offset(&mut w.cursor, end);
	w.sink.int(end);
	w.trailers();
	w.sink.end();
	w.sink.finish()
}

/// Serializes a syntax error: its code and message, UTF-16 `pos` and `end`, and a `loc`; the
/// line table is built up to the error when `positions` has none.
pub fn error_to_json(error: &crate::SyntaxError, source: &str, positions: &Positions) -> String {
	let upto;
	let positions = if positions.lines {
		positions
	} else {
		let mut end = (error.pos.max(error.end) as usize).min(source.len());
		while !source.is_char_boundary(end) {
			end += 1;
		}
		upto = Positions::new(&source[..end], true);
		&upto
	};
	let mut cursor = Cursor::default();
	let pos = positions.offset(&mut cursor, error.pos);
	let (line, column) = positions.line_column(cursor.line, error.pos, pos);
	let end = positions.offset(&mut cursor, error.end);
	let mut out = format!("{{\"error\":{{\"code\":\"{}\",\"message\":", error.code.name());
	write_json_string(&mut out, &error.message);
	write!(
		out,
		",\"pos\":{pos},\"end\":{end},\"loc\":{{\"line\":{line},\"column\":{column}}}}}}}"
	)
	.unwrap();
	out
}

pub struct Writer<'a, X = ()> {
	ast: &'a Ast<X>,
	source: &'a str,
	pub(crate) sink: Json,
	positions: &'a Positions,
	cursor: Cursor,
	pub(crate) output: Output,
	/// The node being written names something bound by another node: no scope facts on it.
	name_only: bool,
	/// What erasure left in place, in emission order.
	kept: Vec<(Name, NodeId)>,
	/// Nodes erasure skipped whose facts the next node written takes over.
	adopted: Vec<NodeId>,
}

/// Maps byte offsets to UTF-16 offsets and line/column pairs, the positions JavaScript counts,
/// and UTF-16 offsets back to bytes. Built once per source; a host that parses many expressions out of one
/// source keeps it.
pub struct Positions {
	/// After each non-ASCII character: its end byte offset and the bytes-minus-code-units gap so far.
	gaps: Vec<(u32, u32)>,
	/// Each line's start as a byte offset and a UTF-16 offset; only with `lines`.
	line_starts: Vec<(u32, u32)>,
	len: u32,
	lines: bool,
}

/// Where the last node's start landed: starts come in source order, and a node's end follows
/// its start, so each lookup tries a few entries on from its hint before a binary search.
#[derive(Default)]
pub(crate) struct Cursor {
	gap: usize,
	line: usize,
}

impl Positions {
	/// `lines` builds the line table, which only `loc` needs.
	pub fn new(source: &str, lines: bool) -> Self {
		let bytes = source.as_bytes();
		let mut gaps = Vec::new();
		let mut line_starts = vec![(0, 0)];
		let mut gap = 0u32;
		let mut i = 0;
		if !lines && bytes.is_ascii() {
			i = bytes.len();
		}
		while i < bytes.len() {
			i = if lines {
				crate::lexer::scan::find(bytes, i, *b"\n\r", true)
			} else {
				crate::lexer::scan::find(bytes, i, [], true)
			};
			let Some(&b) = bytes.get(i) else { break };
			if b < 0x80 {
				i += 1;
				if lines && (b == b'\n' || (b == b'\r' && bytes.get(i) != Some(&b'\n'))) {
					line_starts.push((i as u32, i as u32 - gap));
				}
			} else {
				let len = if b >= 0xf0 {
					4
				} else if b >= 0xe0 {
					3
				} else {
					2
				};
				let separator = len == 3 && crate::lexer::is_separator(&bytes[i..]);
				i += len;
				gap += len as u32 - if len == 4 { 2 } else { 1 };
				gaps.push((i as u32, gap));
				if lines && separator {
					line_starts.push((i as u32, i as u32 - gap));
				}
			}
		}
		Self {
			gaps,
			line_starts,
			len: source.len() as u32,
			lines,
		}
	}

	/// The byte offset of a UTF-16 offset, or why there is none; a position inside a surrogate
	/// pair maps to the byte after the character's first, which is not a character boundary.
	pub fn byte_offset(&self, utf16: f64) -> Result<u32, String> {
		if !(utf16 >= 0.0 && utf16.fract() == 0.0 && utf16 <= u32::MAX as f64) {
			return Err(format!("offset {utf16} is not a valid position"));
		}
		let target = utf16 as u32;
		let total_gap = self.gaps.last().map_or(0, |g| g.1);
		if target > self.len - total_gap {
			return Err(format!("offset {target} is past the end of the source"));
		}
		let i = self.gaps.partition_point(|g| g.0 - g.1 <= target);
		Ok(target + if i == 0 { 0 } else { self.gaps[i - 1].1 })
	}

	/// Every node's span as UTF-16 offsets into `spans`, when the source has gaps, and its lines
	/// and columns into `locs`, when lines are on.
	pub fn map_nodes(&self, nodes: &[crate::ast::Node], spans: &mut Handed<u32>, locs: &mut Handed<u32>) {
		spans.clear();
		locs.clear();
		if self.gaps.is_empty() && !self.lines {
			return;
		}
		let mut cursor = Cursor::default();
		for node in nodes {
			let (start, gap) = self.offset_from(cursor.gap, node.start);
			cursor.gap = gap;
			let (end, _) = self.offset_from(gap, node.end);
			if !self.gaps.is_empty() {
				spans.extend_from_slice(&[start, end]);
			}
			if self.lines {
				let (sl, sc) = self.line_column(cursor.line, node.start, start);
				cursor.line = sl;
				let (el, ec) = self.line_column(sl, node.end, end);
				locs.extend_from_slice(&[sl as u32, sc, el as u32, ec]);
			}
		}
	}

	/// Where each error is, as JavaScript counts: `pos`, `end`, and the line and column of `pos`;
	/// the line table is built up to the last of them when there is none.
	pub fn of_errors(&self, source: &str, errors: &[crate::SyntaxError]) -> Vec<[u32; 4]> {
		let upto;
		let positions = if self.lines || errors.is_empty() {
			self
		} else {
			let last = errors.iter().map(|e| e.pos.max(e.end)).max().unwrap() as usize;
			let mut end = last.min(source.len());
			while !source.is_char_boundary(end) {
				end += 1;
			}
			upto = Positions::new(&source[..end], true);
			&upto
		};
		let mut cursor = Cursor::default();
		errors
			.iter()
			.map(|error| {
				let pos = positions.offset(&mut cursor, error.pos);
				let (line, column) = positions.line_column(cursor.line, error.pos, pos);
				[pos, positions.offset(&mut cursor, error.end), line as u32, column]
			})
			.collect()
	}

	/// Every comment as nine words: whether it is a block, its text's span and its own in UTF-16,
	/// and its lines and columns when lines are on, zeros otherwise.
	pub fn map_comments(&self, comments: &[crate::ast::Comment], out: &mut Handed<u32>) {
		out.clear();
		let mut cursor = Cursor::default();
		for comment in comments {
			let text = comment.text_range();
			let start = self.offset(&mut cursor, comment.start);
			let from = self.offset(&mut cursor, text.start as u32);
			let to = self.offset(&mut cursor, text.end as u32);
			let end = self.offset(&mut cursor, comment.end);
			let mut loc = [0; 4];
			if self.lines {
				let (sl, sc) = self.line_column(cursor.line, comment.start, start);
				cursor.line = sl;
				let (el, ec) = self.line_column(sl, comment.end, end);
				loc = [sl as u32, sc, el as u32, ec];
			}
			out.extend_from_slice(&[comment.is_block() as u32, from, to, start, end]);
			out.extend_from_slice(&loc);
		}
	}

	pub(crate) fn offset(&self, cursor: &mut Cursor, byte: u32) -> u32 {
		let (offset, gap) = self.offset_from(cursor.gap, byte);
		cursor.gap = gap;
		offset
	}

	/// The UTF-16 offset of `byte` and the gap entry it lies after, looked up from `hint`.
	fn offset_from(&self, hint: usize, byte: u32) -> (u32, usize) {
		let byte = byte.min(self.len);
		if self.gaps.is_empty() {
			return (byte, 0);
		}
		let gap = locate(&self.gaps, hint, byte, |g| g.0);
		(byte - if gap == 0 { 0 } else { self.gaps[gap - 1].1 }, gap)
	}

	/// The line of `byte` and its column, given `byte` already mapped by `offset`.
	fn line_column(&self, hint: usize, byte: u32, offset: u32) -> (usize, u32) {
		let byte = byte.min(self.len);
		let line = locate(&self.line_starts, hint.max(1), byte, |l| l.0);
		(line, offset - self.line_starts[line - 1].1)
	}
}

/// The number of `items` whose key is at most `byte`, trying `hint` and the few after it first.
fn locate<T>(items: &[T], hint: usize, byte: u32, key: impl Fn(&T) -> u32) -> usize {
	for p in hint..=(hint + 3).min(items.len()) {
		if (p == 0 || key(&items[p - 1]) <= byte) && (p == items.len() || key(&items[p]) > byte) {
			return p;
		}
	}
	items.partition_point(|item| key(item) <= byte)
}

impl<'a, X: Emit> Writer<'a, X> {
	fn new(ast: &'a Ast<X>, source: &'a str, positions: &'a Positions) -> Self {
		Self {
			ast,
			source,
			sink: Json::default(),
			positions,
			cursor: Cursor::default(),
			output: Output::default(),
			name_only: false,
			kept: Vec::new(),
			adopted: Vec::new(),
		}
	}

	/// Records a node erasure had to leave in place.
	pub(crate) fn keep(&mut self, ty: Name, id: NodeId) {
		self.kept.push((ty, id));
	}

	/// What erasure left in place, as `typescript`, in source order.
	fn all_kept(&mut self) {
		self.key(c!("typescript"));
		self.sink.list();
		let mut kept = std::mem::take(&mut self.kept);
		kept.sort_unstable_by_key(|&(_, id)| self.ast.node(id).start);
		for &(ty, id) in &kept {
			self.sink.begin(ty);
			let node = self.ast.node(id);
			self.span(node.start, node.end);
			self.sink.end();
		}
		self.sink.end();
	}

	pub(crate) fn begin(&mut self, ty: Name, id: NodeId) {
		let node = self.ast.node(id);
		self.sink.begin(ty);
		self.span(node.start, node.end);
		self.scope_facts(id);
		if self.ast.is_parenthesized(id) {
			self.bool(c!("parenthesized"), true);
		}
		self.ast.extension.extras(self, id);
		if let Some(&attached) = self.ast.attached.get(id) {
			self.comments(c!("leadingComments"), attached.leading);
			self.comments(c!("trailingComments"), attached.trailing);
			self.comments(c!("innerComments"), attached.inner);
		}
	}

	/// The scope a node opens and what an identifier is: the binding it declares, or the one it
	/// refers to (null for a global) and whether it writes to it or mutates its value.
	fn scope_facts(&mut self, id: NodeId) {
		let Some(scopes) = &self.ast.scopes else { return };
		if self.name_only {
			return;
		}
		if let Some(scope) = scopes.of_node.get(id) {
			self.key(c!("scope"));
			self.sink.int(scope);
		}
		match scopes.of_identifier.get(id) {
			Some(Role::Declares(binding)) => {
				self.key(c!("declares"));
				self.sink.int(binding);
			}
			Some(Role::Reference(reference)) => {
				self.key(c!("reference"));
				self.sink.int(reference);
			}
			None => {}
		}
		let adopted = std::mem::take(&mut self.adopted);
		for node in adopted.iter().copied().chain([id]) {
			if let Some(root) = scopes.root_of.get(node) {
				self.key(c!("root"));
				self.sink.int(root);
			}
			let bindings = scopes.declared_by.get(node);
			if !bindings.is_empty() {
				self.key(c!("defines"));
				self.sink.ints(bindings);
			}
			let writes = scopes.writes_of.get(node);
			if !writes.is_empty() {
				self.key(c!("writes"));
				self.sink.ints(writes);
			}
		}
	}

	/// The facts of a node erasure leaves out go on the next node written, the one standing in
	/// for it.
	pub(crate) fn adopt(&mut self, id: NodeId) {
		self.adopted.push(id);
	}

	/// The scope, binding and reference tables: what the `scope`, `declares`, `reference` and
	/// `writes` numbers index.
	fn all_scopes(&mut self) {
		let Some(scopes) = &self.ast.scopes else { return };
		self.rows(c!("scopes"), &scopes.scopes, 0);
		self.rows(c!("bindings"), &scopes.bindings, 1);
		self.rows(c!("references"), &scopes.references, 2);
		if !self.ast.hosts.is_empty() {
			self.rows(c!("roots"), &scopes.roots, 3);
		}
	}

	/// A table, each row spelled by its recipe of `recipe::RECIPES`.
	fn rows<T>(&mut self, key: Name, rows: &[T], table: usize) {
		static RESOLVED: std::sync::OnceLock<Vec<&'static [Op<Slot>]>> = std::sync::OnceLock::new();
		let ops = RESOLVED.get_or_init(|| {
			crate::recipe::RECIPES
				.iter()
				.zip(crate::recipe::ROWS)
				.map(|((name, ops), (_, _, fields))| crate::recipe::resolve_ops(ops, fields, name))
				.collect()
		})[table];
		self.key(key);
		self.sink.list();
		for row in rows {
			self.sink.object();
			self.run(NodeId::at(0), ops, row as *const T as *const u8);
			self.sink.end();
		}
		self.sink.end();
	}

	fn comments(&mut self, key: Name, comments: crate::ast::Run) {
		if !comments.is_empty() {
			self.key(key);
			self.comment_list(comments.indices());
		}
	}

	fn comment_list(&mut self, comments: impl IntoIterator<Item = u32>) {
		self.sink.list();
		for index in comments {
			let comment = self.ast.comments[index as usize];
			self.sink
				.begin(if comment.is_block() { c!("Block") } else { c!("Line") });
			self.key(c!("value"));
			let range = comment.text_range();
			self.slice(range.start as u32, range.end as u32);
			self.span(comment.start, comment.end);
			self.sink.end();
		}
		self.sink.end();
	}

	/// The recovered errors as the thrown one would be: code, message, `pos`, `end` and a `loc`.
	fn errors(&mut self) {
		let places = self.positions.of_errors(self.source, &self.ast.errors);
		self.key(c!("errors"));
		self.sink.list();
		for (error, [pos, end, line, column]) in self.ast.errors.iter().zip(places) {
			self.sink.object();
			self.key(c!("code"));
			self.sink.str(error.code.label());
			self.key(c!("message"));
			self.sink.text(&error.message);
			self.key(c!("pos"));
			self.sink.int(pos);
			self.key(c!("end"));
			self.sink.int(end);
			self.key(c!("loc"));
			self.sink.object();
			self.key(c!("line"));
			self.sink.int(line);
			self.key(c!("column"));
			self.sink.int(column);
			self.sink.end();
			self.sink.end();
		}
		self.sink.end();
	}

	/// What the output's switches add after a root: every comment, what erasure kept, the scopes.
	fn trailers(&mut self) {
		if self.output.comments {
			self.key(c!("comments"));
			self.comment_list(0..self.ast.comments.len() as u32);
		}
		if self.output.errors {
			self.errors();
		}
		if self.output.erase {
			self.all_kept();
		}
		if self.output.scopes {
			self.all_scopes();
		}
	}

	pub(crate) fn span(&mut self, start: u32, end: u32) {
		let (start_offset, gap) = self.positions.offset_from(self.cursor.gap, start);
		self.cursor.gap = gap;
		let (end_offset, _) = self.positions.offset_from(gap, end);
		self.sink.span(start_offset, end_offset);
		if !self.positions.lines {
			return;
		}
		let (sl, sc) = self.positions.line_column(self.cursor.line, start, start_offset);
		self.cursor.line = sl;
		let (el, ec) = self.positions.line_column(sl, end, end_offset);
		self.sink.loc(sl as u32, sc, el as u32, ec);
	}

	/// The source between two byte offsets, as a string value.
	fn slice(&mut self, start: u32, end: u32) {
		self.sink.text(&self.source[start as usize..end as usize]);
	}

	pub(crate) fn end(&mut self) {
		self.sink.end();
	}

	pub(crate) fn key(&mut self, key: Name) {
		self.sink.key(key);
	}

	pub(crate) fn field(&mut self, key: Name, id: NodeId) {
		self.key(key);
		self.node(id);
	}

	/// A specifier's other name, which is the same node as the binding one in `import { a }`
	/// and `export { a }`, and then only names: the binding facts stay on the binding one.
	fn other_name(&mut self, key: Name, id: NodeId, binding: NodeId) {
		let was = self.name_only;
		self.name_only = id == binding;
		self.field(key, id);
		self.name_only = was;
	}

	pub(crate) fn kind(&self, id: NodeId) -> NodeKind {
		self.ast.node(id).kind
	}

	/// The key only when there is a node; an unset property is left out.
	pub(crate) fn opt_key(&mut self, key: Name, id: Option<NodeId>) {
		if let Some(id) = id {
			self.field(key, id);
		}
	}

	pub(crate) fn opt(&mut self, key: Name, id: Option<NodeId>) {
		self.key(key);
		match id {
			Some(id) => self.node(id),
			None => self.sink.null(),
		}
	}

	pub(crate) fn list(&mut self, key: Name, list: List) {
		self.key(key);
		self.sink.list();
		let ast = self.ast;
		for item in ast.list(list) {
			if self.output.erase && item.is_some_and(|id| ast.extension.erased(ast, id)) {
				continue;
			}
			match item {
				Some(id) => self.node(*id),
				None => self.sink.null(),
			}
		}
		self.sink.end();
	}

	/// A parameter list; erasing drops TypeScript's `this` parameter.
	pub(crate) fn params(&mut self, key: Name, list: List) {
		if self.output.erase
			&& let Some(&Some(first)) = self.ast.list(list).first()
			&& let NodeKind::Identifier { name } = self.kind(first)
			&& self.ast.str(name) == "this"
		{
			let rest = List {
				start: list.start + 1,
				len: list.len - 1,
			};
			return self.list(key, rest);
		}
		self.list(key, list)
	}

	pub(crate) fn bool(&mut self, key: Name, value: bool) {
		self.key(key);
		self.sink.bool(value);
	}

	pub(crate) fn string(&mut self, key: Name, value: Name) {
		self.key(key);
		self.sink.str(value);
	}

	/// A string computed for this tree.
	pub(crate) fn text(&mut self, key: Name, value: &str) {
		self.key(key);
		self.sink.text(value);
	}

	pub(crate) fn interned(&mut self, key: Name, id: StrId) {
		self.key(key);
		self.sink.text(self.ast.str(id));
	}

	pub(crate) fn raw(&mut self, id: NodeId) {
		let node = self.ast.node(id);
		self.key(c!("raw"));
		self.slice(node.start, node.end);
	}

	/// The recipes of the JavaScript kinds, resolved once.
	fn js_recipes() -> &'static [&'static [Op<Slot>]] {
		static RESOLVED: std::sync::OnceLock<&'static [&'static [Op<Slot>]]> = std::sync::OnceLock::new();
		RESOLVED.get_or_init(|| crate::recipe::resolve(crate::recipe::JS, crate::ast::node_layout::VARIANTS))
	}

	/// Follows a kind's recipe over the record at `base`, a `repr(C, u32)` enum's payload; the
	/// caller ends the node, unless a child stood in for it: false then.
	pub(crate) fn run(&mut self, id: NodeId, ops: &[Op<Slot>], base: *const u8) -> bool {
		for op in ops {
			match *op {
				Op::Keep(name) => {
					if self.output.erase {
						self.keep(name, id);
					}
				}
				Op::KeepIf(name, slot) => {
					if self.output.erase && get::<bool>(base, slot) {
						self.keep(name, id);
					}
				}
				Op::Through(slot) => {
					if self.output.erase {
						self.adopt(id);
						self.node(get(base, slot));
						return false;
					}
				}
				Op::EnumOr(key, slot, other) => {
					let Ty::OptEnum(names) = slot.ty else { unreachable!() };
					self.string(key, names.get(get::<u8>(base, slot) as usize).copied().unwrap_or(other));
				}
				Op::Type(name) => self.begin(name, id),
				Op::TypeOf(slot) => {
					let Ty::Enum(names) = slot.ty else { unreachable!() };
					self.begin(names[get::<u8>(base, slot) as usize], id);
				}
				Op::Node(key, slot) => self.field(key, get(base, slot)),
				Op::Int(key, slot) => {
					self.key(key);
					self.sink.int(get(base, slot));
				}
				Op::Pair(key, slot) => {
					self.key(key);
					self.sink.ints(&get::<[u32; 2]>(base, slot));
				}
				Op::Opt(key, slot) => match slot.ty {
					Ty::OptU32 => {
						self.key(key);
						match get::<Option<u32>>(base, slot) {
							Some(value) => self.sink.int(value),
							None => self.sink.null(),
						}
					}
					Ty::OptStr => {
						self.key(key);
						match get::<Option<StrId>>(base, slot) {
							Some(string) => self.sink.text(self.ast.str(string)),
							None => self.sink.null(),
						}
					}
					_ => self.opt(key, get(base, slot)),
				},
				Op::OptKey(key, slot) => self.opt_key(key, get(base, slot)),
				Op::List(key, slot) => self.list(key, get(base, slot)),
				Op::OptListKey(key, slot) => {
					if let Some(list) = get::<Option<List>>(base, slot) {
						self.list(key, list);
					}
				}
				Op::Params(key, slot) => self.params(key, get(base, slot)),
				Op::Bool(key, slot) => self.bool(key, get(base, slot)),
				Op::BoolIf(key, slot) => {
					if get::<bool>(base, slot) {
						self.bool(key, true);
					}
				}
				Op::OptBoolKey(key, slot) => {
					if let Some(value) = get::<Option<bool>>(base, slot) {
						self.bool(key, value);
					}
				}
				Op::Str(key, slot) => self.interned(key, get(base, slot)),
				Op::OptStrKey(key, slot) => {
					if let Some(string) = get::<Option<StrId>>(base, slot) {
						self.interned(key, string);
					}
				}
				Op::Enum(key, slot) => {
					let Ty::Enum(names) = slot.ty else { unreachable!() };
					self.string(key, names[get::<u8>(base, slot) as usize]);
				}
				// a missing enum is a byte past the names, wherever the compiler put it
				Op::OptEnumKey(key, slot) => {
					let Ty::OptEnum(names) = slot.ty else { unreachable!() };
					if let Some(&name) = names.get(get::<u8>(base, slot) as usize) {
						self.string(key, name);
					}
				}
				Op::Modifier(key, slot) => {
					let Ty::OptEnum(names) = slot.ty else { unreachable!() };
					if let Some(&name) = names.get(get::<u8>(base, slot) as usize) {
						if name.text == "true" {
							self.bool(key, true);
						} else {
							self.string(key, name);
						}
					}
				}
				Op::BoolNames(key, slot, yes, no) => self.string(key, if get::<bool>(base, slot) { yes } else { no }),
				Op::Float(key, slot) => {
					let value = self.ast.numbers[get::<u32>(base, slot) as usize];
					self.key(key);
					if value.is_finite() {
						self.sink.float(value);
					} else {
						self.sink.null();
					}
				}
				Op::Raw => self.raw(id),
				Op::BigInt => {
					let node = self.ast.node(id);
					let raw = &self.source[node.start as usize..node.end as usize - 1];
					let bigint = bigint_decimal(raw);
					self.text(c!("bigint"), &bigint);
				}
				Op::Const(key, value) => self.string(key, value),
				Op::ConstBool(key, value) => self.bool(key, value),
				Op::Null(key) => {
					self.key(key);
					self.sink.null();
				}
				Op::EmptyList(key) => self.list(key, List::EMPTY),
				Op::Object(key, inner) => {
					self.key(key);
					self.sink.object();
					self.run(id, inner, base);
					self.sink.end();
				}
				Op::OtherName(key, name, binding) => self.other_name(key, get(base, name), get(base, binding)),
			}
		}
		true
	}

	pub(crate) fn node(&mut self, id: NodeId) {
		use NodeKind::*;
		let kind = &self.ast.nodes[id.index() as usize].kind;
		match *kind {
			// the extension closes its own node, since erasing may put another in its place
			Extension(index) => return self.ast.extension.node(self, id, index),
			Host(index) => {
				let host = self.ast.hosts[index as usize];
				if host.ty.is_empty() {
					// an object of the host's without a type, positions and all
					let node = self.ast.node(id);
					self.sink.object();
					self.span(node.start, node.end);
				} else if host.span {
					self.begin(Name::dynamic(host.ty), id);
				} else {
					self.sink.begin(Name::dynamic(host.ty));
					self.scope_facts(id);
				}
				let (from, len) = host.fields;
				for i in from..from + len {
					let (key, value) = self.ast.host_fields[i as usize];
					let key = Name::dynamic(key);
					match value {
						Value::Node(child) => self.field(key, child),
						Value::Nodes(children) => self.list(key, children),
						Value::Str(string) => self.interned(key, string),
						Value::Slice(start, end) => {
							self.key(key);
							self.slice(start, end);
						}
						Value::Strs(start, len) => {
							self.key(key);
							self.sink.list();
							for &string in &self.ast.host_strings[start as usize..(start + len) as usize] {
								self.sink.text(self.ast.str(string));
							}
							self.sink.end();
						}
						Value::Bool(value) => self.bool(key, value),
						Value::Int(value) => {
							self.key(key);
							self.sink.int(value);
						}
						Value::Null => {
							self.key(key);
							self.sink.null();
						}
						Value::Comments => {
							self.key(key);
							self.comment_list(0..self.ast.comments.len() as u32);
						}
					}
				}
			}
			_ => {
				let base = kind as *const NodeKind as *const u8;
				self.run(id, Self::js_recipes()[tag(base)], base);
			}
		}
		self.end();
	}
}

/// The tag of a `repr(C, u32)` enum at `base`: its first word.
pub(crate) fn tag(base: *const u8) -> usize {
	unsafe { base.cast::<u32>().read() as usize }
}

/// The field at `slot` of the record at `base`, as the type the recipe reads it by.
fn get<T: Copy>(base: *const u8, slot: Slot) -> T {
	unsafe { base.add(slot.at).cast::<T>().read_unaligned() }
}

pub(crate) fn push_int(out: &mut String, mut value: u32) {
	const DIGITS: &[u8; 200] = b"0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";
	let mut buf = [0u8; 10];
	let mut i = buf.len();
	while value >= 100 {
		let pair = (value % 100) as usize * 2;
		i -= 2;
		buf[i..i + 2].copy_from_slice(&DIGITS[pair..pair + 2]);
		value /= 100;
	}
	if value >= 10 {
		let pair = value as usize * 2;
		i -= 2;
		buf[i..i + 2].copy_from_slice(&DIGITS[pair..pair + 2]);
	} else {
		i -= 1;
		buf[i] = b'0' + value as u8;
	}
	out.push_str(std::str::from_utf8(&buf[i..]).unwrap());
}

/// The decimal digits of a BigInt literal's text, without the `n`.
fn bigint_decimal(raw: &str) -> String {
	let (radix, digits) = match raw.get(..2) {
		Some("0x" | "0X") => (16, &raw[2..]),
		Some("0o" | "0O") => (8, &raw[2..]),
		Some("0b" | "0B") => (2, &raw[2..]),
		_ => return raw.replace('_', ""),
	};
	let mut limbs: Vec<u32> = vec![0];
	for digit in digits.bytes().filter(|b| *b != b'_') {
		let mut carry = (digit as char).to_digit(radix).unwrap() as u64;
		for limb in limbs.iter_mut() {
			let v = *limb as u64 * radix as u64 + carry;
			*limb = (v % 1_000_000_000) as u32;
			carry = v / 1_000_000_000;
		}
		if carry > 0 {
			limbs.push(carry as u32);
		}
	}
	use std::fmt::Write;
	let mut out = String::with_capacity(limbs.len() * 9);
	write!(out, "{}", limbs.last().unwrap()).unwrap();
	for limb in limbs.iter().rev().skip(1) {
		write!(out, "{limb:09}").unwrap();
	}
	out
}

pub fn write_number(out: &mut String, value: f64) {
	if !value.is_finite() {
		out.push_str("null");
		return;
	}
	if value == 0.0 {
		out.push('0');
		return;
	}
	let mut formatted = Digits::<32>::new();
	write!(formatted, "{value:e}").unwrap();
	let (mantissa, exponent) = formatted.as_str().split_once('e').unwrap();
	let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
	let mut digits = Digits::<32>::new();
	for byte in mantissa.bytes().filter(|b| *b != b'.') {
		digits.buf[digits.len] = byte;
		digits.len += 1;
	}
	even_on_tie(value.abs(), &mut digits.buf[..digits.len]);
	let digits = digits.as_str();
	let k = digits.len() as i32;
	let n = exponent.parse::<i32>().unwrap() + 1;
	out.push_str(sign);
	if k <= n && n <= 21 {
		out.push_str(digits);
		out.extend(std::iter::repeat_n('0', (n - k) as usize));
	} else if 0 < n && n <= 21 {
		out.push_str(&digits[..n as usize]);
		out.push('.');
		out.push_str(&digits[n as usize..]);
	} else if -6 < n && n <= 0 {
		out.push_str("0.");
		out.extend(std::iter::repeat_n('0', (-n) as usize));
		out.push_str(digits);
	} else {
		out.push_str(&digits[..1]);
		if k > 1 {
			out.push('.');
			out.push_str(&digits[1..]);
		}
		out.push('e');
		out.push(if n > 1 { '+' } else { '-' });
		push_int(out, (n - 1).unsigned_abs());
	}
}

/// The digits of one number, written on the stack.
struct Digits<const N: usize> {
	buf: [u8; N],
	len: usize,
}

impl<const N: usize> Digits<N> {
	fn new() -> Self {
		Digits { buf: [0; N], len: 0 }
	}

	fn as_str(&self) -> &str {
		std::str::from_utf8(&self.buf[..self.len]).unwrap()
	}
}

impl<const N: usize> std::fmt::Write for Digits<N> {
	fn write_str(&mut self, s: &str) -> std::fmt::Result {
		let end = self.len + s.len();
		if end > N {
			return Err(std::fmt::Error);
		}
		self.buf[self.len..end].copy_from_slice(s.as_bytes());
		self.len = end;
		Ok(())
	}
}

/// Rust rounds the shortest digits away from zero on an exact tie; JavaScript takes the even ones.
fn even_on_tie(value: f64, digits: &mut [u8]) {
	let k = digits.len();
	// two shortest forms sit at the same distance only at the edge of what a double resolves
	if k < 16 || digits[k - 1].is_multiple_of(2) {
		return;
	}
	let exact = format!("{value:.*e}", 1100);
	let all: Vec<u8> = exact
		.split_once('e')
		.unwrap()
		.0
		.bytes()
		.filter(|b| *b != b'.')
		.collect();
	if all[k] != b'5' || all[k + 1..].iter().any(|&b| b != b'0') {
		return;
	}
	let mut lower = digits.to_vec();
	lower[k - 1] -= 1;
	let lower = std::str::from_utf8(&lower).unwrap();
	let exponent = exact.split_once('e').unwrap().1;
	if let Ok(back) = format!("{}.{}e{exponent}", &lower[..1], &lower[1..]).parse::<f64>()
		&& back == value
	{
		digits[k - 1] -= 1;
	}
}

pub(crate) fn write_json_string(out: &mut String, s: &str) {
	out.push('"');
	let mut from = 0;
	for (i, b) in s.bytes().enumerate() {
		if b >= 0x20 && b != b'"' && b != b'\\' {
			continue;
		}
		out.push_str(&s[from..i]);
		match b {
			b'"' => out.push_str("\\\""),
			b'\\' => out.push_str("\\\\"),
			b'\n' => out.push_str("\\n"),
			b'\r' => out.push_str("\\r"),
			b'\t' => out.push_str("\\t"),
			8 => out.push_str("\\b"),
			12 => out.push_str("\\f"),
			_ => write!(out, "\\u{b:04x}").unwrap(),
		}
		from = i + 1;
	}
	out.push_str(&s[from..]);
	out.push('"');
}

#[cfg(test)]
mod tests {
	#[test]
	#[allow(clippy::excessive_precision)]
	fn numbers_as_javascript_writes_them() {
		for (value, text) in [
			(1e-14, "1e-14"),
			(1e-7, "1e-7"),
			(0.000001, "0.000001"),
			(0.1, "0.1"),
			(1.5, "1.5"),
			(123.456, "123.456"),
			(100.0, "100"),
			(1e20, "100000000000000000000"),
			(1e21, "1e+21"),
			(1.5e300, "1.5e+300"),
			(5e-324, "5e-324"),
			(-0.0, "0"),
			(-2.5e-8, "-2.5e-8"),
			(132405809496.45312, "132405809496.45312"),
			(0.3, "0.3"),
			(9007199254740993.0, "9007199254740992"),
			(1658206780088562.25, "1658206780088562.2"),
			(662936471232937.25, "662936471232937.2"),
			(f64::INFINITY, "null"),
		] {
			let mut out = String::new();
			super::write_number(&mut out, value);
			assert_eq!(out, text, "{value:?}");
		}
	}

	use super::Positions;

	#[test]
	fn byte_offsets() {
		let positions = Positions::new("aé𝒳b", false);
		let bytes: Vec<_> = (0..=6).map(|n| positions.byte_offset(n as f64).ok()).collect();
		assert_eq!(bytes, [Some(0), Some(1), Some(3), Some(4), Some(7), Some(8), None]);
		assert!(positions.byte_offset(-1.0).is_err());
		assert!(positions.byte_offset(1.5).is_err());
		assert_eq!(Positions::new("abc", true).byte_offset(3.0), Ok(3));
	}
}
