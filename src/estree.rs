//! Serializes an `Ast` to ESTree JSON, matching acorn's output shape.

use crate::ast::{Ast, Function, List, MethodKind, NodeId, NodeKind, PropertyKind};
use crate::interner::StrId;
use std::fmt::Write;

/// How an extension's data serializes: its own nodes, and the keys it adds to JavaScript nodes.
pub trait Emit: crate::ast::Walk {
	fn node(&self, w: &mut Writer<Self>, id: NodeId, index: u32);
	fn extras(&self, _w: &mut Writer<Self>, _id: NodeId) {}
}

impl Emit for () {
	fn node(&self, _w: &mut Writer<Self>, _id: NodeId, _index: u32) {
		unreachable!("the JavaScript parser adds no extension nodes")
	}
}

/// Serializes a node; `locations` adds acorn's `loc` to every node.
pub fn to_json<X: Emit>(ast: &Ast<X>, root: NodeId, source: &str, locations: bool) -> String {
	let mut w = Writer::new(ast, source, locations);
	w.node(root);
	w.out
}

/// Serializes several nodes as a JSON array.
pub fn list_to_json<X: Emit>(ast: &Ast<X>, roots: &[NodeId], source: &str, locations: bool) -> String {
	let mut w = Writer::new(ast, source, locations);
	w.out.push('[');
	for (i, &root) in roots.iter().enumerate() {
		if i > 0 {
			w.out.push(',');
		}
		w.node(root);
	}
	w.out.push(']');
	w.out
}

/// The UTF-16 offset of a byte offset.
pub fn utf16_offset(source: &str, byte: u32) -> u32 {
	Positions::new(source).offset(byte)
}

/// Serializes a syntax error the way acorn reports one: UTF-16 `pos` plus a `loc`.
pub fn error_to_json(error: &crate::SyntaxError, source: &str) -> String {
	let positions = Positions::new(source);
	let (line, column) = positions.line_column(error.pos);
	let mut out = String::from("{\"error\":{\"message\":");
	write_json_string(&mut out, &error.message);
	write!(
		out,
		",\"pos\":{},\"loc\":{{\"line\":{line},\"column\":{column}}}}}}}",
		positions.offset(error.pos)
	)
	.unwrap();
	out
}

pub struct Writer<'a, X = ()> {
	ast: &'a Ast<X>,
	source: &'a str,
	out: String,
	positions: Positions,
	locations: bool,
}

/// Maps byte offsets to the UTF-16 offsets and line/column pairs that acorn reports.
struct Positions {
	/// After each non-ASCII character: its end byte offset and the bytes-minus-code-units gap so far.
	gaps: Vec<(u32, u32)>,
	line_starts: Vec<u32>,
	len: u32,
}

impl Positions {
	fn new(source: &str) -> Self {
		let mut gaps = Vec::new();
		let mut gap = 0;
		for (i, c) in source.char_indices() {
			if !c.is_ascii() {
				gap += c.len_utf8() as u32 - c.len_utf16() as u32;
				gaps.push((i as u32 + c.len_utf8() as u32, gap));
			}
		}
		let mut line_starts = vec![0];
		let bytes = source.as_bytes();
		let mut i = 0;
		while i < bytes.len() {
			match bytes[i] {
				b'\n' => line_starts.push(i as u32 + 1),
				b'\r' => {
					if bytes.get(i + 1) != Some(&b'\n') {
						line_starts.push(i as u32 + 1);
					}
				}
				0xe2 if matches!(bytes.get(i + 1..i + 3), Some([0x80, 0xa8 | 0xa9])) => line_starts.push(i as u32 + 3),
				_ => {}
			}
			i += 1;
		}
		Self {
			gaps,
			line_starts,
			len: source.len() as u32,
		}
	}

	fn offset(&self, byte: u32) -> u32 {
		let byte = byte.min(self.len);
		let i = self.gaps.partition_point(|&(end, _)| end <= byte);
		let gap = if i == 0 { 0 } else { self.gaps[i - 1].1 };
		byte - gap
	}

	fn line_column(&self, byte: u32) -> (usize, u32) {
		let byte = byte.min(self.len);
		let line = self.line_starts.partition_point(|&s| s <= byte);
		let start = self.line_starts[line - 1];
		(line, self.offset(byte) - self.offset(start))
	}
}

impl<'a, X: Emit> Writer<'a, X> {
	fn new(ast: &'a Ast<X>, source: &'a str, locations: bool) -> Self {
		Self {
			ast,
			source,
			out: String::new(),
			positions: Positions::new(source),
			locations,
		}
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
		if comments.is_empty() {
			return;
		}
		self.key(key);
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
			self.key("start");
			push_int(&mut self.out, self.positions.offset(comment.start));
			self.key("end");
			push_int(&mut self.out, self.positions.offset(comment.end));
			self.out.push('}');
		}
		self.out.push(']');
	}

	pub(crate) fn span(&mut self, start: u32, end: u32) {
		self.out.push_str(",\"start\":");
		push_int(&mut self.out, self.positions.offset(start));
		self.out.push_str(",\"end\":");
		push_int(&mut self.out, self.positions.offset(end));
		if !self.locations {
			return;
		}
		let (sl, sc) = self.positions.line_column(start);
		let (el, ec) = self.positions.line_column(end);
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
		for (i, item) in self.ast.list(list).iter().enumerate() {
			if i > 0 {
				self.out.push(',');
			}
			match item {
				Some(id) => self.node(*id),
				None => self.out.push_str("null"),
			}
		}
		self.out.push(']');
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
		self.list("params", f.params);
		self.field("body", f.body);
	}

	fn node(&mut self, id: NodeId) {
		use NodeKind::*;
		let kind = self.ast.node(id).kind;
		match kind {
			Program { body, module } => {
				self.begin("Program", id);
				self.list("body", body);
				self.string("sourceType", if module { "module" } else { "script" });
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
				self.list("params", params);
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
			Extension(index) => {
				self.ast.extension.node(self, id, index);
			}
		}
		self.end();
	}
}

fn push_int(out: &mut String, mut value: u32) {
	let mut buf = [0u8; 10];
	let mut i = buf.len();
	loop {
		i -= 1;
		buf[i] = b'0' + (value % 10) as u8;
		value /= 10;
		if value == 0 {
			break;
		}
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
	for c in s.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			'\u{8}' => out.push_str("\\b"),
			'\u{c}' => out.push_str("\\f"),
			c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
			c => out.push(c),
		}
	}
	out.push('"');
}
