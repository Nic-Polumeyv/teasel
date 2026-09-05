//! Serializes an `Ast` to ESTree JSON, matching acorn's output shape.

use crate::ast::{Ast, Function, List, MethodKind, NodeId, NodeKind, PropertyKind};
use crate::interner::StrId;
use std::fmt::Write;

pub fn to_json(ast: &Ast, root: NodeId, source: &str) -> String {
	let mut w = Writer {
		ast,
		source,
		out: String::new(),
		positions: Positions::new(source),
	};
	w.node(root);
	w.out
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

struct Writer<'a> {
	ast: &'a Ast,
	source: &'a str,
	out: String,
	positions: Positions,
}

/// Maps byte offsets to the UTF-16 offsets and line/column pairs that acorn reports.
struct Positions {
	utf16: Option<Vec<u32>>,
	line_starts: Vec<u32>,
}

impl Positions {
	fn new(source: &str) -> Self {
		let utf16 = if source.is_ascii() {
			None
		} else {
			let mut table = Vec::with_capacity(source.len() + 1);
			let mut offset = 0;
			for c in source.chars() {
				for _ in 0..c.len_utf8() {
					table.push(offset);
				}
				offset += c.len_utf16() as u32;
			}
			table.push(offset);
			Some(table)
		};
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
		Self { utf16, line_starts }
	}

	fn offset(&self, byte: u32) -> u32 {
		match &self.utf16 {
			Some(table) => table[byte as usize],
			None => byte,
		}
	}

	fn line_column(&self, byte: u32) -> (usize, u32) {
		let line = self.line_starts.partition_point(|&s| s <= byte);
		let start = self.line_starts[line - 1];
		(line, self.offset(byte) - self.offset(start))
	}
}

impl Writer<'_> {
	fn str(&self, id: StrId) -> &str {
		self.ast.str(id)
	}

	fn begin(&mut self, ty: &str, id: NodeId) {
		let node = self.ast.node(id);
		self.out.push_str("{\"type\":\"");
		self.out.push_str(ty);
		self.out.push('"');
		self.span(node.start, node.end);
	}

	fn span(&mut self, start: u32, end: u32) {
		let (sl, sc) = self.positions.line_column(start);
		let (el, ec) = self.positions.line_column(end);
		write!(
			self.out,
			",\"start\":{},\"end\":{},\"loc\":{{\"start\":{{\"line\":{sl},\"column\":{sc}}},\"end\":{{\"line\":{el},\"column\":{ec}}}}}",
			self.positions.offset(start),
			self.positions.offset(end)
		)
		.unwrap();
	}

	fn end(&mut self) {
		self.out.push('}');
	}

	fn key(&mut self, key: &str) {
		self.out.push_str(",\"");
		self.out.push_str(key);
		self.out.push_str("\":");
	}

	fn field(&mut self, key: &str, id: NodeId) {
		self.key(key);
		self.node(id);
	}

	fn opt(&mut self, key: &str, id: Option<NodeId>) {
		self.key(key);
		match id {
			Some(id) => self.node(id),
			None => self.out.push_str("null"),
		}
	}

	fn list(&mut self, key: &str, list: List) {
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

	fn bool(&mut self, key: &str, value: bool) {
		self.key(key);
		self.out.push_str(if value { "true" } else { "false" });
	}

	fn string(&mut self, key: &str, value: &str) {
		self.key(key);
		write_json_string(&mut self.out, value);
	}

	fn raw(&mut self, id: NodeId) {
		let node = self.ast.node(id);
		let raw = &self.source[node.start as usize..node.end as usize];
		self.string("raw", raw);
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
				let name = self.str(name).to_owned();
				self.string("name", &name);
			}
			PrivateIdentifier { name } => {
				self.begin("PrivateIdentifier", id);
				let name = self.str(name).to_owned();
				self.string("name", &name);
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
				let bigint = raw.replace('_', "");
				self.string("bigint", &bigint);
			}
			StringLiteral { value } => {
				self.begin("Literal", id);
				let value = self.str(value).to_owned();
				self.string("value", &value);
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
				let pattern = self.str(pattern).to_owned();
				let flags = self.str(flags).to_owned();
				write_json_string(&mut self.out, &pattern);
				self.out.push_str(",\"flags\":");
				write_json_string(&mut self.out, &flags);
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
				let raw = self.str(raw).to_owned();
				write_json_string(&mut self.out, &raw);
				self.out.push_str(",\"cooked\":");
				match cooked {
					Some(cooked) => {
						let cooked = self.str(cooked).to_owned();
						write_json_string(&mut self.out, &cooked);
					}
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
					let directive = self.str(directive).to_owned();
					self.string("directive", &directive);
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
		}
		self.end();
	}
}

fn write_json_string(out: &mut String, s: &str) {
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
