//! How each kind of node, each table's rows and what TypeScript adds are spelled in ESTree: a
//! recipe of operations over the fields the layout describes, followed by the JSON writer and by
//! a front end building objects from the tree in place, so the two cannot drift.

use crate::layout::{Field, Ty, Variant};
use crate::names::{Name, c};
use crate::scopes::{Binding, Reference, Root, Scope};

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
	/// A word as a number.
	Int(Name, F),
	/// Two words as a list of two numbers.
	Pair(Name, F),
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
	/// Erasing lists the node under `typescript` by this name.
	Keep(Name),
	/// The same when the bool is set.
	KeepIf(Name, F),
	/// Erasing, the node gives way to this child, which takes its facts over.
	Through(F),
	/// The enum field's name, or the given one when missing.
	EnumOr(Name, F, Name),
}

/// A field resolved: its byte offset from the record's start and its type.
#[derive(Clone, Copy)]
pub struct Slot {
	pub at: usize,
	pub ty: Ty,
}

use Op::*;

/// What else an operation carries, past its key and field, as the layout tells it.
pub enum Rest {
	Nothing,
	/// The names for true and false.
	Names(Name, Name),
	Const(Name),
	Bool(bool),
	Text(&'static str),
	Ops(&'static [Op]),
}

impl Op {
	/// The operation as the layout tells it: its name, its key, its field and the rest.
	pub fn told(&self) -> (&'static str, Option<Name>, Option<Path>, Rest) {
		match *self {
			Type(t) => ("type", Some(t), None, Rest::Nothing),
			TypeOf(f) => ("typeof", None, Some(f), Rest::Nothing),
			Node(k, f) => ("node", Some(k), Some(f), Rest::Nothing),
			Opt(k, f) => ("opt", Some(k), Some(f), Rest::Nothing),
			OptKey(k, f) => ("optkey", Some(k), Some(f), Rest::Nothing),
			List(k, f) => ("list", Some(k), Some(f), Rest::Nothing),
			OptListKey(k, f) => ("optlistkey", Some(k), Some(f), Rest::Nothing),
			Params(k, f) => ("params", Some(k), Some(f), Rest::Nothing),
			Bool(k, f) => ("bool", Some(k), Some(f), Rest::Nothing),
			BoolIf(k, f) => ("boolif", Some(k), Some(f), Rest::Nothing),
			OptBoolKey(k, f) => ("optboolkey", Some(k), Some(f), Rest::Nothing),
			Str(k, f) => ("str", Some(k), Some(f), Rest::Nothing),
			OptStrKey(k, f) => ("optstrkey", Some(k), Some(f), Rest::Nothing),
			Enum(k, f) => ("enum", Some(k), Some(f), Rest::Nothing),
			OptEnumKey(k, f) => ("optenumkey", Some(k), Some(f), Rest::Nothing),
			Modifier(k, f) => ("modifier", Some(k), Some(f), Rest::Nothing),
			BoolNames(k, f, yes, no) => ("boolnames", Some(k), Some(f), Rest::Names(yes, no)),
			Int(k, f) => ("int", Some(k), Some(f), Rest::Nothing),
			Pair(k, f) => ("pair", Some(k), Some(f), Rest::Nothing),
			Float(k, f) => ("float", Some(k), Some(f), Rest::Nothing),
			Raw => ("raw", None, None, Rest::Nothing),
			BigInt => ("bigint", None, None, Rest::Nothing),
			Const(k, value) => ("const", Some(k), None, Rest::Const(value)),
			ConstBool(k, value) => ("constbool", Some(k), None, Rest::Bool(value)),
			Null(k) => ("null", Some(k), None, Rest::Nothing),
			EmptyList(k) => ("emptylist", Some(k), None, Rest::Nothing),
			Object(k, inner) => ("object", Some(k), None, Rest::Ops(inner)),
			OtherName(k, f, binding) => ("othername", Some(k), Some(f), Rest::Text(binding)),
			Keep(name) => ("keep", Some(name), None, Rest::Nothing),
			KeepIf(name, f) => ("keepif", Some(name), Some(f), Rest::Nothing),
			Through(f) => ("through", None, Some(f), Rest::Nothing),
			EnumOr(k, f, other) => ("enumor", Some(k), Some(f), Rest::Const(other)),
		}
	}
}

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

pub(crate) fn resolve_ops(ops: &[Op], fields: &[Field], kind: &str) -> &'static [Op<Slot>] {
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
			Opt(k, f) => Opt(k, slot(f, &[|t| matches!(t, Ty::OptNode | Ty::OptStr | Ty::OptU32)])),
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
			Int(k, f) => Int(k, slot(f, &[|t| matches!(t, Ty::U32)])),
			Pair(k, f) => Pair(k, slot(f, &[|t| matches!(t, Ty::Pair)])),
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
			Keep(name) => Keep(name),
			KeepIf(name, f) => KeepIf(name, slot(f, &[|t| matches!(t, Ty::Bool)])),
			Through(f) => Through(slot(f, &[|t| matches!(t, Ty::Node)])),
			EnumOr(k, f, other) => EnumOr(k, slot(f, &[|t| matches!(t, Ty::OptEnum(_))]), other),
		})
		.collect();
	Box::leak(resolved.into_boxed_slice())
}

/// The TypeScript kinds, by the variant's name.
#[cfg(feature = "typescript")]
#[rustfmt::skip]
pub const TS: &[(&str, &[Op])] = &[
	("TypeAnnotation", &[Type(c!("TSTypeAnnotation")), Node(c!("typeAnnotation"), "type_annotation")]),
	("Keyword", &[TypeOf("0")]),
	("ThisType", &[Type(c!("TSThisType"))]),
	("TypePredicate", &[Type(c!("TSTypePredicate")), Node(c!("parameterName"), "parameter_name"), Opt(c!("typeAnnotation"), "type_annotation"), Bool(c!("asserts"), "asserts")]),
	("TypeReference", &[Type(c!("TSTypeReference")), Node(c!("typeName"), "type_name"), OptKey(c!("typeArguments"), "type_arguments")]),
	("QualifiedName", &[Type(c!("TSQualifiedName")), Node(c!("left"), "left"), Node(c!("right"), "right")]),
	("TypeParameterInstantiation", &[Type(c!("TSTypeParameterInstantiation")), List(c!("params"), "params")]),
	("TypeParameterDeclaration", &[Type(c!("TSTypeParameterDeclaration")), List(c!("params"), "params")]),
	("TypeParameter", &[Type(c!("TSTypeParameter")), BoolIf(c!("in"), "is_in"), BoolIf(c!("out"), "is_out"), BoolIf(c!("const"), "is_const"), Str(c!("name"), "name"), OptKey(c!("constraint"), "constraint"), OptKey(c!("default"), "default")]),
	("FunctionType", &[Type(c!("TSFunctionType")), OptKey(c!("typeParameters"), "type_parameters"), List(c!("parameters"), "parameters"), Node(c!("typeAnnotation"), "type_annotation")]),
	("ConstructorType", &[Type(c!("TSConstructorType")), Bool(c!("abstract"), "is_abstract"), OptKey(c!("typeParameters"), "type_parameters"), List(c!("parameters"), "parameters"), Node(c!("typeAnnotation"), "type_annotation")]),
	("UnionType", &[Type(c!("TSUnionType")), List(c!("types"), "types")]),
	("IntersectionType", &[Type(c!("TSIntersectionType")), List(c!("types"), "types")]),
	("TypeOperator", &[Type(c!("TSTypeOperator")), Str(c!("operator"), "operator"), Node(c!("typeAnnotation"), "type_annotation")]),
	("InferType", &[Type(c!("TSInferType")), Node(c!("typeParameter"), "type_parameter")]),
	("LiteralType", &[Type(c!("TSLiteralType")), Node(c!("literal"), "literal")]),
	("ImportType", &[Type(c!("TSImportType")), Node(c!("argument"), "argument"), OptKey(c!("qualifier"), "qualifier"), OptKey(c!("typeArguments"), "type_arguments")]),
	("TypeQuery", &[Type(c!("TSTypeQuery")), Node(c!("exprName"), "expr_name"), OptKey(c!("typeArguments"), "type_arguments")]),
	("MappedType", &[Type(c!("TSMappedType")), Modifier(c!("readonly"), "readonly"), Node(c!("typeParameter"), "type_parameter"), Opt(c!("nameType"), "name_type"), Modifier(c!("optional"), "optional"), OptKey(c!("typeAnnotation"), "type_annotation")]),
	("TypeLiteral", &[Type(c!("TSTypeLiteral")), List(c!("members"), "members")]),
	("NamedTupleMember", &[Type(c!("TSNamedTupleMember")), Bool(c!("optional"), "optional"), Node(c!("label"), "label"), Node(c!("elementType"), "element_type")]),
	("OptionalType", &[Type(c!("TSOptionalType")), Node(c!("typeAnnotation"), "type_annotation")]),
	("RestType", &[Type(c!("TSRestType")), Node(c!("typeAnnotation"), "type_annotation")]),
	("TupleType", &[Type(c!("TSTupleType")), List(c!("elementTypes"), "element_types")]),
	("ParenthesizedType", &[Type(c!("TSParenthesizedType")), Node(c!("typeAnnotation"), "type_annotation")]),
	("ArrayType", &[Type(c!("TSArrayType")), Node(c!("elementType"), "element_type")]),
	("IndexedAccessType", &[Type(c!("TSIndexedAccessType")), Node(c!("objectType"), "object_type"), Node(c!("indexType"), "index_type")]),
	("ConditionalType", &[Type(c!("TSConditionalType")), Node(c!("checkType"), "check_type"), Node(c!("extendsType"), "extends_type"), Node(c!("trueType"), "true_type"), Node(c!("falseType"), "false_type")]),
	("IndexSignature", &[Type(c!("TSIndexSignature")), List(c!("parameters"), "parameters"), OptKey(c!("typeAnnotation"), "type_annotation")]),
	("CallSignatureDeclaration", &[Type(c!("TSCallSignatureDeclaration")), OptKey(c!("typeParameters"), "type_parameters"), List(c!("parameters"), "parameters"), OptKey(c!("typeAnnotation"), "type_annotation")]),
	("ConstructSignatureDeclaration", &[Type(c!("TSConstructSignatureDeclaration")), OptKey(c!("typeParameters"), "type_parameters"), List(c!("parameters"), "parameters"), OptKey(c!("typeAnnotation"), "type_annotation")]),
	("MethodSignature", &[Type(c!("TSMethodSignature")), Node(c!("key"), "key"), Bool(c!("computed"), "computed"), BoolIf(c!("optional"), "optional"), Enum(c!("kind"), "kind"), OptKey(c!("typeParameters"), "type_parameters"), List(c!("parameters"), "parameters"), OptKey(c!("typeAnnotation"), "type_annotation")]),
	("PropertySignature", &[Type(c!("TSPropertySignature")), Node(c!("key"), "key"), OptBoolKey(c!("computed"), "computed"), BoolIf(c!("optional"), "optional"), BoolIf(c!("readonly"), "readonly"), OptEnumKey(c!("kind"), "kind"), OptKey(c!("typeAnnotation"), "type_annotation")]),
	("InterfaceDeclaration", &[Type(c!("TSInterfaceDeclaration")), Node(c!("id"), "id"), OptKey(c!("typeParameters"), "type_parameters"), OptListKey(c!("extends"), "extends"), Node(c!("body"), "body")]),
	("InterfaceBody", &[Type(c!("TSInterfaceBody")), List(c!("body"), "body")]),
	("ExpressionWithTypeArguments", &[Type(c!("TSExpressionWithTypeArguments")), Node(c!("expression"), "expression"), OptKey(c!("typeParameters"), "type_arguments")]),
	("EnumDeclaration", &[Keep(c!("TSEnumDeclaration")), Type(c!("TSEnumDeclaration")), BoolIf(c!("const"), "is_const"), Node(c!("id"), "id"), List(c!("members"), "members")]),
	("EnumMember", &[Type(c!("TSEnumMember")), Node(c!("id"), "id"), OptKey(c!("initializer"), "initializer")]),
	("ModuleDeclaration", &[Keep(c!("TSModuleDeclaration")), Type(c!("TSModuleDeclaration")), BoolIf(c!("global"), "global"), Node(c!("id"), "id"), OptKey(c!("body"), "body")]),
	("ModuleBlock", &[Type(c!("TSModuleBlock")), List(c!("body"), "body")]),
	("TypeAliasDeclaration", &[Type(c!("TSTypeAliasDeclaration")), Node(c!("id"), "id"), OptKey(c!("typeParameters"), "type_parameters"), Node(c!("typeAnnotation"), "type_annotation")]),
	("ImportEqualsDeclaration", &[Keep(c!("TSImportEqualsDeclaration")), Type(c!("TSImportEqualsDeclaration")), Enum(c!("importKind"), "import_kind"), Bool(c!("isExport"), "is_export"), Node(c!("id"), "id"), Node(c!("moduleReference"), "module_reference")]),
	("ExternalModuleReference", &[Type(c!("TSExternalModuleReference")), Node(c!("expression"), "expression")]),
	("ExportAssignment", &[Keep(c!("TSExportAssignment")), Type(c!("TSExportAssignment")), Node(c!("expression"), "expression")]),
	("NamespaceExportDeclaration", &[Type(c!("TSNamespaceExportDeclaration")), Node(c!("id"), "id")]),
	("DeclareFunction", &[Type(c!("TSDeclareFunction")), Opt(c!("id"), "id"), Bool(c!("generator"), "generator"), Bool(c!("async"), "is_async"), ConstBool(c!("expression"), false), List(c!("params"), "params")]),
	("DeclareMethod", &[Type(c!("TSDeclareMethod")), Null(c!("id")), Bool(c!("generator"), "generator"), Bool(c!("async"), "is_async"), ConstBool(c!("expression"), false), List(c!("params"), "params")]),
	("AsExpression", &[Through("expression"), Type(c!("TSAsExpression")), Node(c!("expression"), "expression"), Node(c!("typeAnnotation"), "type_annotation")]),
	("SatisfiesExpression", &[Through("expression"), Type(c!("TSSatisfiesExpression")), Node(c!("expression"), "expression"), Node(c!("typeAnnotation"), "type_annotation")]),
	("NonNullExpression", &[Through("expression"), Type(c!("TSNonNullExpression")), Node(c!("expression"), "expression")]),
	("TypeAssertion", &[Through("expression"), Type(c!("TSTypeAssertion")), Node(c!("typeAnnotation"), "type_annotation"), Node(c!("expression"), "expression")]),
	("TypeCastExpression", &[Through("expression"), Type(c!("TSTypeCastExpression")), Node(c!("expression"), "expression"), Node(c!("typeAnnotation"), "type_annotation")]),
	("InstantiationExpression", &[Through("expression"), Type(c!("TSInstantiationExpression")), Node(c!("expression"), "expression"), Node(c!("typeArguments"), "type_arguments")]),
	("ParameterProperty", &[Keep(c!("TSParameterProperty")), Through("parameter"), Type(c!("TSParameterProperty")), Node(c!("parameter"), "parameter")]),
	("Decorator", &[Keep(c!("Decorator")), Type(c!("Decorator")), Node(c!("expression"), "expression")]),
];

/// What TypeScript adds to a JavaScript kind whatever its extras say, over the extras record.
#[cfg(feature = "typescript")]
#[rustfmt::skip]
pub const ADDS: &[(&str, &[Op])] = &[
	("ImportDeclaration", &[EnumOr(c!("importKind"), "import_kind", c!("value"))]),
	("ImportSpecifier", &[EnumOr(c!("importKind"), "import_kind", c!("value"))]),
	("ExportDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportNamedDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportDefaultDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportAllDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportSpecifier", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
];

/// The keys a node's extras add, after the kind's own.
#[cfg(feature = "typescript")]
#[rustfmt::skip]
pub const EXTRAS: &[Op] = &[
	OptKey(c!("typeAnnotation"), "type_annotation"), OptKey(c!("returnType"), "return_type"), OptKey(c!("typeParameters"), "type_parameters"),
	OptKey(c!("typeArguments"), "type_arguments"), OptKey(c!("superTypeParameters"), "super_type_arguments"),
	OptListKey(c!("implements"), "implements"), OptListKey(c!("decorators"), "decorators"), OptEnumKey(c!("accessibility"), "accessibility"),
	BoolIf(c!("optional"), "optional"), BoolIf(c!("definite"), "definite"), BoolIf(c!("declare"), "declare"), BoolIf(c!("abstract"), "is_abstract"),
	BoolIf(c!("readonly"), "readonly"), BoolIf(c!("override"), "is_override"), BoolIf(c!("accessor"), "accessor"), BoolIf(c!("static"), "is_static"),
];

/// The same when erasing: the proposals JavaScript itself has, decorators and accessor fields.
#[cfg(feature = "typescript")]
pub const EXTRAS_ERASED: &[Op] = &[
	OptListKey(c!("decorators"), "decorators"),
	BoolIf(c!("accessor"), "accessor"),
	KeepIf(c!("AccessorProperty"), "accessor"),
];

/// How each table's rows are spelled: the recipes of the answer's `scopes`, `bindings`,
/// `references` and `roots`.
#[rustfmt::skip]
pub const RECIPES: &[(&str, &[Op])] = &[
	("scopes", &[Enum(c!("kind"), "kind"), Opt(c!("parent"), "parent"), Bool(c!("topLevelAwait"), "top_level_await")]),
	("bindings", &[Str(c!("name"), "name"), Enum(c!("kind"), "kind"), Int(c!("scope"), "scope"), Bool(c!("write"), "write")]),
	("references", &[Int(c!("scope"), "scope"), Opt(c!("binding"), "binding"), Bool(c!("write"), "write"), Bool(c!("read"), "read"), Bool(c!("mutate"), "mutate"), Bool(c!("declares"), "declares")]),
	("roots", &[Int(c!("scope"), "scope"), Pair(c!("scopes"), "scopes"), Pair(c!("bindings"), "bindings"), Pair(c!("references"), "references")]),
];

/// Each table's name, its record's size and fields, in the order of `RECIPES`.
pub const ROWS: &[(&str, usize, &[Field])] = &[
	("scopes", size_of::<Scope>(), Scope::FIELDS),
	("bindings", size_of::<Binding>(), Binding::FIELDS),
	("references", size_of::<Reference>(), Reference::FIELDS),
	("roots", size_of::<Root>(), Root::FIELDS),
];
