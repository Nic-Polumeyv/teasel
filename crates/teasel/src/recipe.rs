//! How each kind of node is spelled in ESTree: a recipe of operations over the fields the layout
//! describes, followed by the JSON writer here and by a front end building objects from the
//! tree in place, so the two cannot drift.

use crate::layout::{Field, Ty, Variant};
use crate::names::{Name, c};

/// A field of the kind's record by name, `function.id` for one inside a record; a `Slot` once
/// resolved against the layout.
pub type Path = &'static str;

#[derive(Clone, Copy)]
pub enum Op<F: Copy + 'static = Path> {
	/// The node's `type`, first.
	Type(Name),
	/// The `type` is the enum field's name: a keyword type.
	TypeOf(F),
	Node(Name, F),
	/// Null when missing.
	Opt(Name, F),
	/// Left out when missing.
	OptKey(Name, F),
	List(Name, F),
	OptListKey(Name, F),
	/// A parameter list: erasing drops a leading `this`.
	Params(Name, F),
	Bool(Name, F),
	/// Only when true.
	BoolIf(Name, F),
	OptBoolKey(Name, F),
	/// An interned string.
	Str(Name, F),
	OptStrKey(Name, F),
	/// The enum field's name as a string.
	Enum(Name, F),
	OptEnumKey(Name, F),
	/// A mapped type's modifier: `+` or `-` as a string, bare as `true`, left out when missing.
	Modifier(Name, F),
	/// One of two names by a bool: the first when true.
	BoolNames(Name, F, Name, Name),
	/// A number literal's value: the float, or null when not finite.
	Float(Name, F),
	/// `raw`: the node's source text.
	Raw,
	/// `bigint`: the literal's decimal digits.
	BigInt,
	Const(Name, Name),
	ConstBool(Name, bool),
	Null(Name),
	EmptyList(Name),
	Object(Name, &'static [Op<F>]),
	/// A specifier's other name: names only when it is the binding node, the second field.
	OtherName(Name, F, F),
}

/// A field resolved: its byte offset from the record's start and its type.
#[derive(Clone, Copy)]
pub struct Slot {
	pub at: usize,
	pub ty: Ty,
}

use Op::*;

/// The JavaScript kinds, by the variant's name; `Extension` and `Host` have none, the writer
/// spells them itself.
#[rustfmt::skip]
pub const JS: &[(&str, &[Op])] = &[
	("Program", &[Type(c!("Program")), List(c!("body"), "body"), BoolNames(c!("sourceType"), "module", c!("module"), c!("script"))]),
	("Identifier", &[Type(c!("Identifier")), Str(c!("name"), "name")]),
	("PrivateIdentifier", &[Type(c!("PrivateIdentifier")), Str(c!("name"), "name")]),
	("NumberLiteral", &[Type(c!("Literal")), Float(c!("value"), "value"), Raw]),
	("BigIntLiteral", &[Type(c!("Literal")), Null(c!("value")), Raw, BigInt]),
	("StringLiteral", &[Type(c!("Literal")), Str(c!("value"), "value"), Raw]),
	("BooleanLiteral", &[Type(c!("Literal")), Bool(c!("value"), "value"), Raw]),
	("NullLiteral", &[Type(c!("Literal")), Null(c!("value")), Raw]),
	("RegExpLiteral", &[Type(c!("Literal")), Null(c!("value")), Raw, Object(c!("regex"), &[Str(c!("pattern"), "pattern"), Str(c!("flags"), "flags")])]),
	("TemplateLiteral", &[Type(c!("TemplateLiteral")), List(c!("expressions"), "expressions"), List(c!("quasis"), "quasis")]),
	("TemplateElement", &[Type(c!("TemplateElement")), Object(c!("value"), &[Str(c!("raw"), "raw"), Opt(c!("cooked"), "cooked")]), Bool(c!("tail"), "tail")]),
	("TaggedTemplateExpression", &[Type(c!("TaggedTemplateExpression")), Node(c!("tag"), "tag"), Node(c!("quasi"), "quasi")]),
	("ThisExpression", &[Type(c!("ThisExpression"))]),
	("Super", &[Type(c!("Super"))]),
	("ArrayExpression", &[Type(c!("ArrayExpression")), List(c!("elements"), "elements")]),
	("ObjectExpression", &[Type(c!("ObjectExpression")), List(c!("properties"), "properties")]),
	("Property", &[Type(c!("Property")), Bool(c!("method"), "method"), Bool(c!("shorthand"), "shorthand"), Bool(c!("computed"), "computed"), Node(c!("key"), "key"), Node(c!("value"), "value"), Enum(c!("kind"), "kind")]),
	("SpreadElement", &[Type(c!("SpreadElement")), Node(c!("argument"), "argument")]),
	("UnaryExpression", &[Type(c!("UnaryExpression")), Enum(c!("operator"), "operator"), ConstBool(c!("prefix"), true), Node(c!("argument"), "argument")]),
	("UpdateExpression", &[Type(c!("UpdateExpression")), Enum(c!("operator"), "operator"), Bool(c!("prefix"), "prefix"), Node(c!("argument"), "argument")]),
	("BinaryExpression", &[Type(c!("BinaryExpression")), Node(c!("left"), "left"), Enum(c!("operator"), "operator"), Node(c!("right"), "right")]),
	("LogicalExpression", &[Type(c!("LogicalExpression")), Node(c!("left"), "left"), Enum(c!("operator"), "operator"), Node(c!("right"), "right")]),
	("AssignmentExpression", &[Type(c!("AssignmentExpression")), Enum(c!("operator"), "operator"), Node(c!("left"), "left"), Node(c!("right"), "right")]),
	("ConditionalExpression", &[Type(c!("ConditionalExpression")), Node(c!("test"), "test"), Node(c!("consequent"), "consequent"), Node(c!("alternate"), "alternate")]),
	("MemberExpression", &[Type(c!("MemberExpression")), Node(c!("object"), "object"), Node(c!("property"), "property"), Bool(c!("computed"), "computed"), Bool(c!("optional"), "optional")]),
	("CallExpression", &[Type(c!("CallExpression")), Node(c!("callee"), "callee"), List(c!("arguments"), "arguments"), Bool(c!("optional"), "optional")]),
	("ChainExpression", &[Type(c!("ChainExpression")), Node(c!("expression"), "expression")]),
	("NewExpression", &[Type(c!("NewExpression")), Node(c!("callee"), "callee"), List(c!("arguments"), "arguments")]),
	("SequenceExpression", &[Type(c!("SequenceExpression")), List(c!("expressions"), "expressions")]),
	("ArrowFunctionExpression", &[Type(c!("ArrowFunctionExpression")), Null(c!("id")), Bool(c!("expression"), "expression"), ConstBool(c!("generator"), false), Bool(c!("async"), "is_async"), Params(c!("params"), "params"), Node(c!("body"), "body")]),
	("FunctionExpression", &[Type(c!("FunctionExpression")), Opt(c!("id"), "function.id"), ConstBool(c!("expression"), false), Bool(c!("generator"), "function.generator"), Bool(c!("async"), "function.is_async"), Params(c!("params"), "function.params"), Node(c!("body"), "function.body")]),
	("FunctionDeclaration", &[Type(c!("FunctionDeclaration")), Opt(c!("id"), "function.id"), ConstBool(c!("expression"), false), Bool(c!("generator"), "function.generator"), Bool(c!("async"), "function.is_async"), Params(c!("params"), "function.params"), Node(c!("body"), "function.body")]),
	("ClassExpression", &[Type(c!("ClassExpression")), Opt(c!("id"), "class.id"), Opt(c!("superClass"), "class.super_class"), Node(c!("body"), "class.body")]),
	("ClassDeclaration", &[Type(c!("ClassDeclaration")), Opt(c!("id"), "class.id"), Opt(c!("superClass"), "class.super_class"), Node(c!("body"), "class.body")]),
	("ClassBody", &[Type(c!("ClassBody")), List(c!("body"), "body")]),
	("MethodDefinition", &[Type(c!("MethodDefinition")), Bool(c!("static"), "is_static"), Bool(c!("computed"), "computed"), Node(c!("key"), "key"), Enum(c!("kind"), "kind"), Node(c!("value"), "value")]),
	("PropertyDefinition", &[Type(c!("PropertyDefinition")), Bool(c!("static"), "is_static"), Bool(c!("computed"), "computed"), Node(c!("key"), "key"), Opt(c!("value"), "value")]),
	("StaticBlock", &[Type(c!("StaticBlock")), List(c!("body"), "body")]),
	("YieldExpression", &[Type(c!("YieldExpression")), Bool(c!("delegate"), "delegate"), Opt(c!("argument"), "argument")]),
	("AwaitExpression", &[Type(c!("AwaitExpression")), Node(c!("argument"), "argument")]),
	("MetaProperty", &[Type(c!("MetaProperty")), Node(c!("meta"), "meta"), Node(c!("property"), "property")]),
	("ImportExpression", &[Type(c!("ImportExpression")), Node(c!("source"), "source"), Opt(c!("options"), "options")]),
	("ObjectPattern", &[Type(c!("ObjectPattern")), List(c!("properties"), "properties")]),
	("ArrayPattern", &[Type(c!("ArrayPattern")), List(c!("elements"), "elements")]),
	("RestElement", &[Type(c!("RestElement")), Node(c!("argument"), "argument")]),
	("AssignmentPattern", &[Type(c!("AssignmentPattern")), Node(c!("left"), "left"), Node(c!("right"), "right")]),
	("ExpressionStatement", &[Type(c!("ExpressionStatement")), Node(c!("expression"), "expression"), OptStrKey(c!("directive"), "directive")]),
	("BlockStatement", &[Type(c!("BlockStatement")), List(c!("body"), "body")]),
	("EmptyStatement", &[Type(c!("EmptyStatement"))]),
	("DebuggerStatement", &[Type(c!("DebuggerStatement"))]),
	("WithStatement", &[Type(c!("WithStatement")), Node(c!("object"), "object"), Node(c!("body"), "body")]),
	("ReturnStatement", &[Type(c!("ReturnStatement")), Opt(c!("argument"), "argument")]),
	("LabeledStatement", &[Type(c!("LabeledStatement")), Node(c!("body"), "body"), Node(c!("label"), "label")]),
	("BreakStatement", &[Type(c!("BreakStatement")), Opt(c!("label"), "label")]),
	("ContinueStatement", &[Type(c!("ContinueStatement")), Opt(c!("label"), "label")]),
	("IfStatement", &[Type(c!("IfStatement")), Node(c!("test"), "test"), Node(c!("consequent"), "consequent"), Opt(c!("alternate"), "alternate")]),
	("SwitchStatement", &[Type(c!("SwitchStatement")), Node(c!("discriminant"), "discriminant"), List(c!("cases"), "cases")]),
	("SwitchCase", &[Type(c!("SwitchCase")), List(c!("consequent"), "consequent"), Opt(c!("test"), "test")]),
	("ThrowStatement", &[Type(c!("ThrowStatement")), Node(c!("argument"), "argument")]),
	("TryStatement", &[Type(c!("TryStatement")), Node(c!("block"), "block"), Opt(c!("handler"), "handler"), Opt(c!("finalizer"), "finalizer")]),
	("CatchClause", &[Type(c!("CatchClause")), Opt(c!("param"), "param"), Node(c!("body"), "body")]),
	("WhileStatement", &[Type(c!("WhileStatement")), Node(c!("test"), "test"), Node(c!("body"), "body")]),
	("DoWhileStatement", &[Type(c!("DoWhileStatement")), Node(c!("body"), "body"), Node(c!("test"), "test")]),
	("ForStatement", &[Type(c!("ForStatement")), Opt(c!("init"), "init"), Opt(c!("test"), "test"), Opt(c!("update"), "update"), Node(c!("body"), "body")]),
	("ForInStatement", &[Type(c!("ForInStatement")), Node(c!("left"), "left"), Node(c!("right"), "right"), Node(c!("body"), "body")]),
	("ForOfStatement", &[Type(c!("ForOfStatement")), Bool(c!("await"), "is_await"), Node(c!("left"), "left"), Node(c!("right"), "right"), Node(c!("body"), "body")]),
	("VariableDeclaration", &[Type(c!("VariableDeclaration")), List(c!("declarations"), "declarations"), Enum(c!("kind"), "kind")]),
	("VariableDeclarator", &[Type(c!("VariableDeclarator")), Node(c!("id"), "id"), Opt(c!("init"), "init")]),
	("ImportDeclaration", &[Type(c!("ImportDeclaration")), List(c!("specifiers"), "specifiers"), Node(c!("source"), "source"), List(c!("attributes"), "attributes")]),
	("ImportSpecifier", &[Type(c!("ImportSpecifier")), OtherName(c!("imported"), "imported", "local"), Node(c!("local"), "local")]),
	("ImportDefaultSpecifier", &[Type(c!("ImportDefaultSpecifier")), Node(c!("local"), "local")]),
	("ImportNamespaceSpecifier", &[Type(c!("ImportNamespaceSpecifier")), Node(c!("local"), "local")]),
	("ImportAttribute", &[Type(c!("ImportAttribute")), Node(c!("key"), "key"), Node(c!("value"), "value")]),
	("ExportDeclaration", &[Type(c!("ExportNamedDeclaration")), Node(c!("declaration"), "declaration"), EmptyList(c!("specifiers")), Null(c!("source")), EmptyList(c!("attributes"))]),
	("ExportNamedDeclaration", &[Type(c!("ExportNamedDeclaration")), Null(c!("declaration")), List(c!("specifiers"), "specifiers"), Opt(c!("source"), "source"), List(c!("attributes"), "attributes")]),
	("ExportSpecifier", &[Type(c!("ExportSpecifier")), Node(c!("local"), "local"), OtherName(c!("exported"), "exported", "local")]),
	("ExportDefaultDeclaration", &[Type(c!("ExportDefaultDeclaration")), Node(c!("declaration"), "declaration")]),
	("ExportAllDeclaration", &[Type(c!("ExportAllDeclaration")), Opt(c!("exported"), "exported"), Node(c!("source"), "source"), List(c!("attributes"), "attributes")]),
	("Extension", &[]),
	("Host", &[]),
];

/// The recipes in tag order with their fields resolved, leaked once: the writer indexes them by
/// a node's tag.
pub fn resolve(recipes: &[(&str, &[Op])], variants: &[Variant]) -> &'static [&'static [Op<Slot>]] {
	let table: Vec<&'static [Op<Slot>]> = variants
		.iter()
		.map(|variant| {
			let (_, ops) = recipes
				.iter()
				.find(|(name, _)| *name == variant.name)
				.unwrap_or_else(|| panic!("no recipe for {}", variant.name));
			resolve_ops(ops, variant.fields, variant.name)
		})
		.collect();
	Box::leak(table.into_boxed_slice())
}

fn resolve_ops(ops: &[Op], fields: &[Field], kind: &str) -> &'static [Op<Slot>] {
	let slot = |path: Path, wanted: &[fn(&Ty) -> bool]| -> Slot {
		let (head, rest) = path.split_once('.').map_or((path, None), |(h, r)| (h, Some(r)));
		let field = fields
			.iter()
			.find(|f| f.name == head)
			.unwrap_or_else(|| panic!("{kind} has no field {path}"));
		let slot = match (rest, field.ty) {
			(None, ty) => Slot { at: field.at, ty },
			(Some(rest), Ty::Struct(inner)) => {
				let f = inner
					.iter()
					.find(|f| f.name == rest)
					.unwrap_or_else(|| panic!("{kind} has no field {path}"));
				Slot {
					at: field.at + f.at,
					ty: f.ty,
				}
			}
			_ => panic!("{kind}: {path} is not inside a record"),
		};
		assert!(
			wanted.iter().any(|ok| ok(&slot.ty)),
			"{kind}: {path} has the wrong type for its recipe"
		);
		slot
	};
	let resolved: Vec<Op<Slot>> = ops
		.iter()
		.map(|op| match *op {
			Type(name) => Type(name),
			TypeOf(f) => TypeOf(slot(f, &[|t| matches!(t, Ty::Enum(_))])),
			Node(k, f) => Node(k, slot(f, &[|t| matches!(t, Ty::Node)])),
			Opt(k, f) => Opt(k, slot(f, &[|t| matches!(t, Ty::OptNode | Ty::OptStr)])),
			OptKey(k, f) => OptKey(k, slot(f, &[|t| matches!(t, Ty::OptNode)])),
			List(k, f) => List(k, slot(f, &[|t| matches!(t, Ty::List)])),
			OptListKey(k, f) => OptListKey(k, slot(f, &[|t| matches!(t, Ty::OptList)])),
			Params(k, f) => Params(k, slot(f, &[|t| matches!(t, Ty::List)])),
			Bool(k, f) => Bool(k, slot(f, &[|t| matches!(t, Ty::Bool)])),
			BoolIf(k, f) => BoolIf(k, slot(f, &[|t| matches!(t, Ty::Bool)])),
			OptBoolKey(k, f) => OptBoolKey(k, slot(f, &[|t| matches!(t, Ty::OptBool)])),
			Str(k, f) => Str(k, slot(f, &[|t| matches!(t, Ty::Str)])),
			OptStrKey(k, f) => OptStrKey(k, slot(f, &[|t| matches!(t, Ty::OptStr)])),
			Enum(k, f) => Enum(k, slot(f, &[|t| matches!(t, Ty::Enum(_))])),
			OptEnumKey(k, f) => OptEnumKey(k, slot(f, &[|t| matches!(t, Ty::OptEnum(_))])),
			Modifier(k, f) => Modifier(k, slot(f, &[|t| matches!(t, Ty::OptEnum(_))])),
			BoolNames(k, f, a, b) => BoolNames(k, slot(f, &[|t| matches!(t, Ty::Bool)]), a, b),
			Float(k, f) => Float(k, slot(f, &[|t| matches!(t, Ty::U32)])),
			Raw => Raw,
			BigInt => BigInt,
			Const(k, v) => Const(k, v),
			ConstBool(k, v) => ConstBool(k, v),
			Null(k) => Null(k),
			EmptyList(k) => EmptyList(k),
			Object(k, inner) => Object(k, resolve_ops(inner, fields, kind)),
			OtherName(k, f, g) => OtherName(
				k,
				slot(f, &[|t| matches!(t, Ty::Node)]),
				slot(g, &[|t| matches!(t, Ty::Node)]),
			),
		})
		.collect();
	Box::leak(resolved.into_boxed_slice())
}

fn json_str(out: &mut String, text: &str) {
	out.push('"');
	out.push_str(text);
	out.push('"');
}

fn json_ops(out: &mut String, ops: &[Op]) {
	out.push('[');
	for (i, op) in ops.iter().enumerate() {
		if i > 0 {
			out.push(',');
		}
		out.push('[');
		let (name, key, field, rest): (&str, Option<Name>, Option<Path>, Option<&[Op]>) = match *op {
			Type(t) => ("type", Some(t), None, None),
			TypeOf(f) => ("typeof", None, Some(f), None),
			Node(k, f) => ("node", Some(k), Some(f), None),
			Opt(k, f) => ("opt", Some(k), Some(f), None),
			OptKey(k, f) => ("optkey", Some(k), Some(f), None),
			List(k, f) => ("list", Some(k), Some(f), None),
			OptListKey(k, f) => ("optlistkey", Some(k), Some(f), None),
			Params(k, f) => ("params", Some(k), Some(f), None),
			Bool(k, f) => ("bool", Some(k), Some(f), None),
			BoolIf(k, f) => ("boolif", Some(k), Some(f), None),
			OptBoolKey(k, f) => ("optboolkey", Some(k), Some(f), None),
			Str(k, f) => ("str", Some(k), Some(f), None),
			OptStrKey(k, f) => ("optstrkey", Some(k), Some(f), None),
			Enum(k, f) => ("enum", Some(k), Some(f), None),
			OptEnumKey(k, f) => ("optenumkey", Some(k), Some(f), None),
			Modifier(k, f) => ("modifier", Some(k), Some(f), None),
			BoolNames(k, f, _, _) => ("boolnames", Some(k), Some(f), None),
			Float(k, f) => ("float", Some(k), Some(f), None),
			Raw => ("raw", None, None, None),
			BigInt => ("bigint", None, None, None),
			Const(k, _) => ("const", Some(k), None, None),
			ConstBool(k, _) => ("constbool", Some(k), None, None),
			Null(k) => ("null", Some(k), None, None),
			EmptyList(k) => ("emptylist", Some(k), None, None),
			Object(k, inner) => ("object", Some(k), None, Some(inner)),
			OtherName(k, f, _) => ("othername", Some(k), Some(f), None),
		};
		json_str(out, name);
		if let Some(key) = key {
			out.push(',');
			json_str(out, key.text);
		}
		if let Some(field) = field {
			out.push(',');
			json_str(out, field);
		}
		match *op {
			BoolNames(_, _, yes, no) => {
				out.push(',');
				json_str(out, yes.text);
				out.push(',');
				json_str(out, no.text);
			}
			Const(_, value) => {
				out.push(',');
				json_str(out, value.text);
			}
			ConstBool(_, value) => out.push_str(if value { ",true" } else { ",false" }),
			OtherName(_, _, binding) => {
				out.push(',');
				json_str(out, binding);
			}
			_ => {}
		}
		if let Some(inner) = rest {
			out.push(',');
			json_ops(out, inner);
		}
		out.push(']');
	}
	out.push(']');
}

/// The recipes as JSON: each kind's name and its operations, an operation as its name, then its
/// key, its field and what else it takes.
pub fn json(out: &mut String, recipes: &[(&str, &[Op])]) {
	out.push('[');
	for (i, (name, ops)) in recipes.iter().enumerate() {
		if i > 0 {
			out.push(',');
		}
		out.push('[');
		json_str(out, name);
		out.push(',');
		json_ops(out, ops);
		out.push(']');
	}
	out.push(']');
}
