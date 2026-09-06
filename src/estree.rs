//! Serializes an `Ast` to ESTree JSON, matching acorn's output shape.

use crate::ast::{Ast, Function, List, MethodKind, NodeId, NodeKind, PropertyKind};
use crate::interner::StrId;
use std::fmt::Write;

/// How an extension's data serializes: its own nodes, and the keys it adds to JavaScript nodes.
pub trait Emit: crate::ast::Walk {
	/// Emits one of the extension's own nodes and closes it, or what erasing puts in its place.
	fn node(&self, w: &mut Writer<Self>, id: NodeId, index: u32);
	fn extras(&self, _w: &mut Writer<Self>, _id: NodeId) {}
	/// Whether erasing leaves nothing of a node in a list: type-only declarations and imports.
	fn erased(&self, _w: &Writer<Self>, _id: NodeId) -> bool {
		false
	}
}

/// How a tree serializes beyond acorn's shape.
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
	/// The answer lists every comment read.
	pub comments: bool,
	/// TypeScript is erased: annotations, type-only declarations and imports go, assertions give
	/// way to their expression, and what erasure cannot express is listed as `typescript`.
	pub erase: bool,
}

impl Emit for () {
	fn node(&self, _w: &mut Writer<Self>, _id: NodeId, _index: u32) {
		unreachable!("the JavaScript parser adds no extension nodes")
	}
}

/// Serializes a node; `locations` adds acorn's `loc` to every node.
pub fn to_json<X: Emit>(ast: &Ast<X>, root: NodeId, source: &str, locations: bool) -> String {
	let positions = Positions::new(source, locations);
	let mut w = Writer::new(ast, source, &positions);
	w.node(root);
	w.out
}

/// Serializes a program; `comments` adds every comment to it as `comments`.
pub fn program_to_json<X: Emit>(
	ast: &Ast<X>,
	root: NodeId,
	source: &str,
	positions: &Positions,
	output: Output,
) -> String {
	let mut w = Writer::new(ast, source, positions);
	w.program_tail = true;
	w.output = output;
	w.node(root);
	w.out
}

/// Serializes a node parsed at an offset as `{"node":...,"end":N}`, `end` being the offset after
/// everything the parse consumed; `comments` adds every comment read as `comments`.
pub fn node_to_json<X: Emit>(
	ast: &Ast<X>,
	root: NodeId,
	end: u32,
	source: &str,
	positions: &Positions,
	output: Output,
) -> String {
	let mut w = Writer::new(ast, source, positions);
	w.output = output;
	w.out.push_str("{\"node\":");
	w.node(root);
	w.tail(end);
	w.out
}

/// Serializes parameters as `{"params":[...],"end":N}` the way `node_to_json` does a node.
pub fn params_to_json<X: Emit>(
	ast: &Ast<X>,
	params: &[NodeId],
	end: u32,
	source: &str,
	positions: &Positions,
	output: Output,
) -> String {
	let mut w = Writer::new(ast, source, positions);
	w.output = output;
	w.out.push_str("{\"params\":[");
	let mut first = true;
	for &param in params {
		if w.output.erase && ast.extension.erased(&w, param) {
			continue;
		}
		if !first {
			w.out.push(',');
		}
		first = false;
		w.node(param);
	}
	w.out.push(']');
	w.tail(end);
	w.out
}

/// Serializes a syntax error the way acorn reports one: UTF-16 `pos` plus a `loc`.
pub fn error_to_json(error: &crate::SyntaxError, source: &str) -> String {
	let positions = Positions::new(source, true);
	let mut cursor = Cursor::default();
	let pos = positions.offset(&mut cursor, error.pos);
	let (line, column) = positions.line_column(&mut cursor, error.pos, pos);
	let mut out = String::from("{\"error\":{\"message\":");
	write_json_string(&mut out, &error.message);
	write!(
		out,
		",\"pos\":{pos},\"loc\":{{\"line\":{line},\"column\":{column}}}}}}}"
	)
	.unwrap();
	out
}

pub struct Writer<'a, X = ()> {
	ast: &'a Ast<X>,
	source: &'a str,
	out: String,
	positions: &'a Positions,
	cursor: Cursor,
	pub(crate) output: Output,
	/// Whether the program carries the comment list and the erasure leftovers.
	program_tail: bool,
	/// What erasure left in place, in emission order.
	kept: Vec<(&'static str, NodeId)>,
}

/// Maps byte offsets to the UTF-16 offsets and line/column pairs that acorn reports, and UTF-16
/// offsets back to bytes. Built once per source; a host that parses many expressions out of one
/// source keeps it.
pub struct Positions {
	/// After each non-ASCII character: its end byte offset and the bytes-minus-code-units gap so far.
	gaps: Vec<(u32, u32)>,
	/// Each line's start as a byte offset and a UTF-16 offset; only with `lines`.
	line_starts: Vec<(u32, u32)>,
	len: u32,
	lines: bool,
}

/// Where the last lookups landed: nodes serialize in source order, so each lookup first tries
/// the entry the previous one found and its successor before falling back to a binary search.
#[derive(Default)]
struct Cursor {
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
			let b = bytes[i];
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

	fn offset(&self, cursor: &mut Cursor, byte: u32) -> u32 {
		if self.gaps.is_empty() {
			return byte.min(self.len);
		}
		let byte = byte.min(self.len);
		cursor.gap = locate(&self.gaps, cursor.gap, byte, |g| g.0);
		byte - if cursor.gap == 0 {
			0
		} else {
			self.gaps[cursor.gap - 1].1
		}
	}

	/// The line of `byte` and its column, given `byte` already mapped by `offset`.
	fn line_column(&self, cursor: &mut Cursor, byte: u32, offset: u32) -> (usize, u32) {
		let byte = byte.min(self.len);
		cursor.line = locate(&self.line_starts, cursor.line.max(1), byte, |l| l.0);
		(cursor.line, offset - self.line_starts[cursor.line - 1].1)
	}
}

/// The number of `items` whose key is at most `byte`, trying `hint` and the next index first.
fn locate<T>(items: &[T], hint: usize, byte: u32, key: impl Fn(&T) -> u32) -> usize {
	for p in [hint, hint + 1] {
		if p <= items.len() && (p == 0 || key(&items[p - 1]) <= byte) && (p == items.len() || key(&items[p]) > byte) {
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
			out: String::with_capacity(ast.nodes.len() * if positions.lines { 160 } else { 80 }),
			positions,
			cursor: Cursor::default(),
			output: Output::default(),
			program_tail: false,
			kept: Vec::new(),
		}
	}

	/// Records a node erasure had to leave in place.
	pub(crate) fn keep(&mut self, ty: &'static str, id: NodeId) {
		self.kept.push((ty, id));
	}

	/// What erasure left in place, as `typescript`.
	fn all_kept(&mut self) {
		self.key("typescript");
		self.out.push('[');
		let mut kept = std::mem::take(&mut self.kept);
		kept.sort_by_key(|&(_, id)| self.ast.node(id).start);
		for (i, &(ty, id)) in kept.iter().enumerate() {
			if i > 0 {
				self.out.push(',');
			}
			self.out.push_str("{\"type\":\"");
			self.out.push_str(ty);
			self.out.push('"');
			let node = self.ast.node(id);
			self.span(node.start, node.end);
			self.out.push('}');
		}
		self.out.push(']');
	}

	pub(crate) fn begin(&mut self, ty: &str, id: NodeId) {
		let node = self.ast.node(id);
		self.out.push_str("{\"type\":\"");
		self.out.push_str(ty);
		self.out.push('"');
		self.span(node.start, node.end);
		self.ast.extension.extras(self, id);
		if let Some(attached) = self.ast.attached.get(&id) {
			self.comments("leadingComments", &attached.leading);
			self.comments("trailingComments", &attached.trailing);
			self.comments("innerComments", &attached.inner);
		}
	}

	fn comments(&mut self, key: &str, comments: &[u32]) {
		if !comments.is_empty() {
			self.key(key);
			self.comment_list(comments);
		}
	}

	fn comment_list(&mut self, comments: &[u32]) {
		self.out.push('[');
		for (i, &index) in comments.iter().enumerate() {
			if i > 0 {
				self.out.push(',');
			}
			let comment = self.ast.comments[index as usize];
			self.out.push_str(if comment.is_block() {
				"{\"type\":\"Block\""
			} else {
				"{\"type\":\"Line\""
			});
			self.key("value");
			write_json_string(&mut self.out, &self.source[comment.text_range()]);
			self.span(comment.start, comment.end);
			self.out.push('}');
		}
		self.out.push(']');
	}

	/// Every comment read, in source order.
	fn all_comments(&mut self) {
		let all: Vec<u32> = (0..self.ast.comments.len() as u32).collect();
		self.key("comments");
		self.comment_list(&all);
	}

	/// Closes the object around a node parsed at an offset.
	fn tail(&mut self, end: u32) {
		self.out.push_str(",\"end\":");
		push_int(&mut self.out, self.positions.offset(&mut self.cursor, end));
		if self.output.comments {
			self.all_comments();
		}
		if self.output.erase {
			self.all_kept();
		}
		self.out.push('}');
	}

	pub(crate) fn span(&mut self, start: u32, end: u32) {
		let (start_offset, end_offset) = (
			self.positions.offset(&mut self.cursor, start),
			self.positions.offset(&mut self.cursor, end),
		);
		self.out.push_str(",\"start\":");
		push_int(&mut self.out, start_offset);
		self.out.push_str(",\"end\":");
		push_int(&mut self.out, end_offset);
		if !self.positions.lines {
			return;
		}
		let (sl, sc) = self.positions.line_column(&mut self.cursor, start, start_offset);
		let (el, ec) = self.positions.line_column(&mut self.cursor, end, end_offset);
		self.out.push_str(",\"loc\":{\"start\":{\"line\":");
		push_int(&mut self.out, sl as u32);
		self.out.push_str(",\"column\":");
		push_int(&mut self.out, sc);
		self.out.push_str("},\"end\":{\"line\":");
		push_int(&mut self.out, el as u32);
		self.out.push_str(",\"column\":");
		push_int(&mut self.out, ec);
		self.out.push_str("}}");
	}

	pub(crate) fn end(&mut self) {
		self.out.push('}');
	}

	pub(crate) fn key(&mut self, key: &str) {
		self.out.push_str(",\"");
		self.out.push_str(key);
		self.out.push_str("\":");
	}

	pub(crate) fn field(&mut self, key: &str, id: NodeId) {
		self.key(key);
		self.node(id);
	}

	pub(crate) fn kind(&self, id: NodeId) -> NodeKind {
		self.ast.node(id).kind
	}

	pub(crate) fn ast(&self) -> &'a Ast<X> {
		self.ast
	}

	/// The key only when there is a node, the way acorn leaves unset properties out.
	pub(crate) fn opt_key(&mut self, key: &str, id: Option<NodeId>) {
		if let Some(id) = id {
			self.field(key, id);
		}
	}

	pub(crate) fn opt(&mut self, key: &str, id: Option<NodeId>) {
		self.key(key);
		match id {
			Some(id) => self.node(id),
			None => self.out.push_str("null"),
		}
	}

	pub(crate) fn list(&mut self, key: &str, list: List) {
		self.key(key);
		self.out.push('[');
		let ast = self.ast;
		let mut first = true;
		for item in ast.list(list) {
			if self.output.erase && item.is_some_and(|id| ast.extension.erased(self, id)) {
				continue;
			}
			if !first {
				self.out.push(',');
			}
			first = false;
			match item {
				Some(id) => self.node(*id),
				None => self.out.push_str("null"),
			}
		}
		self.out.push(']');
	}

	/// A parameter list; erasing drops TypeScript's `this` parameter.
	pub(crate) fn params(&mut self, key: &str, list: List) {
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

	pub(crate) fn bool(&mut self, key: &str, value: bool) {
		self.key(key);
		self.out.push_str(if value { "true" } else { "false" });
	}

	pub(crate) fn string(&mut self, key: &str, value: &str) {
		self.key(key);
		write_json_string(&mut self.out, value);
	}

	pub(crate) fn interned(&mut self, key: &str, id: StrId) {
		self.key(key);
		write_json_string(&mut self.out, self.ast.str(id));
	}

	pub(crate) fn raw(&mut self, id: NodeId) {
		let node = self.ast.node(id);
		self.key("raw");
		write_json_string(&mut self.out, &self.source[node.start as usize..node.end as usize]);
	}

	fn function(&mut self, f: Function, expression: bool) {
		self.opt("id", f.id);
		self.bool("expression", expression);
		self.bool("generator", f.generator);
		self.bool("async", f.is_async);
		self.params("params", f.params);
		self.field("body", f.body);
	}

	pub(crate) fn node(&mut self, id: NodeId) {
		use NodeKind::*;
		let kind = self.ast.node(id).kind;
		match kind {
			Program { body, module } => {
				self.begin("Program", id);
				self.list("body", body);
				self.string("sourceType", if module { "module" } else { "script" });
				if self.program_tail && self.output.comments {
					self.all_comments();
				}
				if self.program_tail && self.output.erase {
					self.all_kept();
				}
			}
			Identifier { name } => {
				self.begin("Identifier", id);
				self.interned("name", name);
			}
			PrivateIdentifier { name } => {
				self.begin("PrivateIdentifier", id);
				self.interned("name", name);
			}
			NumberLiteral { value } => {
				self.begin("Literal", id);
				self.key("value");
				if value.is_finite() {
					write!(self.out, "{value}").unwrap();
				} else {
					self.out.push_str("null");
				}
				self.raw(id);
			}
			BigIntLiteral => {
				self.begin("Literal", id);
				self.key("value");
				self.out.push_str("null");
				self.raw(id);
				let node = self.ast.node(id);
				let raw = &self.source[node.start as usize..node.end as usize - 1];
				let bigint = bigint_decimal(raw);
				self.string("bigint", &bigint);
			}
			StringLiteral { value } => {
				self.begin("Literal", id);
				self.interned("value", value);
				self.raw(id);
			}
			BooleanLiteral { value } => {
				self.begin("Literal", id);
				self.bool("value", value);
				self.raw(id);
			}
			NullLiteral => {
				self.begin("Literal", id);
				self.key("value");
				self.out.push_str("null");
				self.raw(id);
			}
			RegExpLiteral { pattern, flags } => {
				self.begin("Literal", id);
				self.key("value");
				self.out.push_str("null");
				self.raw(id);
				self.key("regex");
				self.out.push_str("{\"pattern\":");
				write_json_string(&mut self.out, self.ast.str(pattern));
				self.out.push_str(",\"flags\":");
				write_json_string(&mut self.out, self.ast.str(flags));
				self.out.push('}');
			}
			TemplateLiteral { quasis, expressions } => {
				self.begin("TemplateLiteral", id);
				self.list("expressions", expressions);
				self.list("quasis", quasis);
			}
			TemplateElement { cooked, raw, tail } => {
				self.begin("TemplateElement", id);
				self.key("value");
				self.out.push_str("{\"raw\":");
				write_json_string(&mut self.out, self.ast.str(raw));
				self.out.push_str(",\"cooked\":");
				match cooked {
					Some(cooked) => write_json_string(&mut self.out, self.ast.str(cooked)),
					None => self.out.push_str("null"),
				}
				self.out.push('}');
				self.bool("tail", tail);
			}
			TaggedTemplateExpression { tag, quasi } => {
				self.begin("TaggedTemplateExpression", id);
				self.field("tag", tag);
				self.field("quasi", quasi);
			}
			ThisExpression => self.begin("ThisExpression", id),
			Super => self.begin("Super", id),
			ArrayExpression { elements } => {
				self.begin("ArrayExpression", id);
				self.list("elements", elements);
			}
			ObjectExpression { properties } => {
				self.begin("ObjectExpression", id);
				self.list("properties", properties);
			}
			Property {
				key,
				value,
				kind,
				computed,
				method,
				shorthand,
			} => {
				self.begin("Property", id);
				self.bool("method", method);
				self.bool("shorthand", shorthand);
				self.bool("computed", computed);
				self.field("key", key);
				self.field("value", value);
				self.string(
					"kind",
					match kind {
						PropertyKind::Init => "init",
						PropertyKind::Get => "get",
						PropertyKind::Set => "set",
					},
				);
			}
			SpreadElement { argument } => {
				self.begin("SpreadElement", id);
				self.field("argument", argument);
			}
			UnaryExpression { operator, argument } => {
				self.begin("UnaryExpression", id);
				self.string("operator", operator.as_str());
				self.bool("prefix", true);
				self.field("argument", argument);
			}
			UpdateExpression {
				operator,
				prefix,
				argument,
			} => {
				self.begin("UpdateExpression", id);
				self.string("operator", operator.as_str());
				self.bool("prefix", prefix);
				self.field("argument", argument);
			}
			BinaryExpression { operator, left, right } => {
				self.begin("BinaryExpression", id);
				self.field("left", left);
				self.string("operator", operator.as_str());
				self.field("right", right);
			}
			LogicalExpression { operator, left, right } => {
				self.begin("LogicalExpression", id);
				self.field("left", left);
				self.string("operator", operator.as_str());
				self.field("right", right);
			}
			AssignmentExpression { operator, left, right } => {
				self.begin("AssignmentExpression", id);
				self.string("operator", operator.as_str());
				self.field("left", left);
				self.field("right", right);
			}
			ConditionalExpression {
				test,
				consequent,
				alternate,
			} => {
				self.begin("ConditionalExpression", id);
				self.field("test", test);
				self.field("consequent", consequent);
				self.field("alternate", alternate);
			}
			MemberExpression {
				object,
				property,
				computed,
				optional,
			} => {
				self.begin("MemberExpression", id);
				self.field("object", object);
				self.field("property", property);
				self.bool("computed", computed);
				self.bool("optional", optional);
			}
			CallExpression {
				callee,
				arguments,
				optional,
			} => {
				self.begin("CallExpression", id);
				self.field("callee", callee);
				self.list("arguments", arguments);
				self.bool("optional", optional);
			}
			ChainExpression { expression } => {
				self.begin("ChainExpression", id);
				self.field("expression", expression);
			}
			NewExpression { callee, arguments } => {
				self.begin("NewExpression", id);
				self.field("callee", callee);
				self.list("arguments", arguments);
			}
			SequenceExpression { expressions } => {
				self.begin("SequenceExpression", id);
				self.list("expressions", expressions);
			}
			ParenthesizedExpression { expression } => {
				self.begin("ParenthesizedExpression", id);
				self.field("expression", expression);
			}
			ArrowFunctionExpression {
				params,
				body,
				expression,
				is_async,
			} => {
				self.begin("ArrowFunctionExpression", id);
				self.key("id");
				self.out.push_str("null");
				self.bool("expression", expression);
				self.bool("generator", false);
				self.bool("async", is_async);
				self.params("params", params);
				self.field("body", body);
			}
			FunctionExpression { function } => {
				self.begin("FunctionExpression", id);
				self.function(function, false);
			}
			FunctionDeclaration { function } => {
				self.begin("FunctionDeclaration", id);
				self.function(function, false);
			}
			ClassExpression { class } | ClassDeclaration { class } => {
				self.begin(
					if matches!(kind, ClassExpression { .. }) {
						"ClassExpression"
					} else {
						"ClassDeclaration"
					},
					id,
				);
				self.opt("id", class.id);
				self.opt("superClass", class.super_class);
				self.field("body", class.body);
			}
			ClassBody { body } => {
				self.begin("ClassBody", id);
				self.list("body", body);
			}
			MethodDefinition {
				key,
				value,
				kind,
				computed,
				is_static,
			} => {
				self.begin("MethodDefinition", id);
				self.bool("static", is_static);
				self.bool("computed", computed);
				self.field("key", key);
				self.string(
					"kind",
					match kind {
						MethodKind::Constructor => "constructor",
						MethodKind::Method => "method",
						MethodKind::Get => "get",
						MethodKind::Set => "set",
					},
				);
				self.field("value", value);
			}
			PropertyDefinition {
				key,
				value,
				computed,
				is_static,
			} => {
				self.begin("PropertyDefinition", id);
				self.bool("static", is_static);
				self.bool("computed", computed);
				self.field("key", key);
				self.opt("value", value);
			}
			StaticBlock { body } => {
				self.begin("StaticBlock", id);
				self.list("body", body);
			}
			YieldExpression { argument, delegate } => {
				self.begin("YieldExpression", id);
				self.bool("delegate", delegate);
				self.opt("argument", argument);
			}
			AwaitExpression { argument } => {
				self.begin("AwaitExpression", id);
				self.field("argument", argument);
			}
			MetaProperty { meta, property } => {
				self.begin("MetaProperty", id);
				self.field("meta", meta);
				self.field("property", property);
			}
			ImportExpression { source, options } => {
				self.begin("ImportExpression", id);
				self.field("source", source);
				self.opt("options", options);
			}
			ObjectPattern { properties } => {
				self.begin("ObjectPattern", id);
				self.list("properties", properties);
			}
			ArrayPattern { elements } => {
				self.begin("ArrayPattern", id);
				self.list("elements", elements);
			}
			RestElement { argument } => {
				self.begin("RestElement", id);
				self.field("argument", argument);
			}
			AssignmentPattern { left, right } => {
				self.begin("AssignmentPattern", id);
				self.field("left", left);
				self.field("right", right);
			}
			ExpressionStatement { expression, directive } => {
				self.begin("ExpressionStatement", id);
				self.field("expression", expression);
				if let Some(directive) = directive {
					self.interned("directive", directive);
				}
			}
			BlockStatement { body } => {
				self.begin("BlockStatement", id);
				self.list("body", body);
			}
			EmptyStatement => self.begin("EmptyStatement", id),
			DebuggerStatement => self.begin("DebuggerStatement", id),
			WithStatement { object, body } => {
				self.begin("WithStatement", id);
				self.field("object", object);
				self.field("body", body);
			}
			ReturnStatement { argument } => {
				self.begin("ReturnStatement", id);
				self.opt("argument", argument);
			}
			LabeledStatement { label, body } => {
				self.begin("LabeledStatement", id);
				self.field("body", body);
				self.field("label", label);
			}
			BreakStatement { label } => {
				self.begin("BreakStatement", id);
				self.opt("label", label);
			}
			ContinueStatement { label } => {
				self.begin("ContinueStatement", id);
				self.opt("label", label);
			}
			IfStatement {
				test,
				consequent,
				alternate,
			} => {
				self.begin("IfStatement", id);
				self.field("test", test);
				self.field("consequent", consequent);
				self.opt("alternate", alternate);
			}
			SwitchStatement { discriminant, cases } => {
				self.begin("SwitchStatement", id);
				self.field("discriminant", discriminant);
				self.list("cases", cases);
			}
			SwitchCase { test, consequent } => {
				self.begin("SwitchCase", id);
				self.list("consequent", consequent);
				self.opt("test", test);
			}
			ThrowStatement { argument } => {
				self.begin("ThrowStatement", id);
				self.field("argument", argument);
			}
			TryStatement {
				block,
				handler,
				finalizer,
			} => {
				self.begin("TryStatement", id);
				self.field("block", block);
				self.opt("handler", handler);
				self.opt("finalizer", finalizer);
			}
			CatchClause { param, body } => {
				self.begin("CatchClause", id);
				self.opt("param", param);
				self.field("body", body);
			}
			WhileStatement { test, body } => {
				self.begin("WhileStatement", id);
				self.field("test", test);
				self.field("body", body);
			}
			DoWhileStatement { body, test } => {
				self.begin("DoWhileStatement", id);
				self.field("body", body);
				self.field("test", test);
			}
			ForStatement {
				init,
				test,
				update,
				body,
			} => {
				self.begin("ForStatement", id);
				self.opt("init", init);
				self.opt("test", test);
				self.opt("update", update);
				self.field("body", body);
			}
			ForInStatement { left, right, body } => {
				self.begin("ForInStatement", id);
				self.field("left", left);
				self.field("right", right);
				self.field("body", body);
			}
			ForOfStatement {
				left,
				right,
				body,
				is_await,
			} => {
				self.begin("ForOfStatement", id);
				self.bool("await", is_await);
				self.field("left", left);
				self.field("right", right);
				self.field("body", body);
			}
			VariableDeclaration { declarations, kind } => {
				self.begin("VariableDeclaration", id);
				self.list("declarations", declarations);
				self.string("kind", kind.as_str());
			}
			VariableDeclarator { id: pattern, init } => {
				self.begin("VariableDeclarator", id);
				self.field("id", pattern);
				self.opt("init", init);
			}
			ImportDeclaration {
				specifiers,
				source,
				attributes,
			} => {
				self.begin("ImportDeclaration", id);
				self.list("specifiers", specifiers);
				self.field("source", source);
				self.list("attributes", attributes);
			}
			ImportSpecifier { imported, local } => {
				self.begin("ImportSpecifier", id);
				self.field("imported", imported);
				self.field("local", local);
			}
			ImportDefaultSpecifier { local } => {
				self.begin("ImportDefaultSpecifier", id);
				self.field("local", local);
			}
			ImportNamespaceSpecifier { local } => {
				self.begin("ImportNamespaceSpecifier", id);
				self.field("local", local);
			}
			ImportAttribute { key, value } => {
				self.begin("ImportAttribute", id);
				self.field("key", key);
				self.field("value", value);
			}
			ExportNamedDeclaration {
				declaration,
				specifiers,
				source,
				attributes,
			} => {
				self.begin("ExportNamedDeclaration", id);
				self.opt("declaration", declaration);
				self.list("specifiers", specifiers);
				self.opt("source", source);
				self.list("attributes", attributes);
			}
			ExportSpecifier { local, exported } => {
				self.begin("ExportSpecifier", id);
				self.field("local", local);
				self.field("exported", exported);
			}
			ExportDefaultDeclaration { declaration } => {
				self.begin("ExportDefaultDeclaration", id);
				self.field("declaration", declaration);
			}
			ExportAllDeclaration {
				exported,
				source,
				attributes,
			} => {
				self.begin("ExportAllDeclaration", id);
				self.opt("exported", exported);
				self.field("source", source);
				self.list("attributes", attributes);
			}
			// the extension closes its own node, since erasing may put another in its place
			Extension(index) => return self.ast.extension.node(self, id, index),
		}
		self.end();
	}
}

fn push_int(out: &mut String, mut value: u32) {
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
	let mut out = limbs.last().unwrap().to_string();
	for limb in limbs.iter().rev().skip(1) {
		out.push_str(&format!("{limb:09}"));
	}
	out
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
