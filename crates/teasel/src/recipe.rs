//! How each kind of node, each table's rows and what TypeScript adds are spelled in ESTree: a
//! recipe of operations over the fields the layout describes, each field found as the crate
//! compiles. The JSON writer follows them, and the decoder building objects from the tree in
//! place, so the two cannot drift.

use crate::ast::Extras;
use crate::ast::node_layout::VARIANTS;
use crate::layout::{Field, Ty};
use crate::scopes::{Binding, Reference, Root, Scope};

/// A field of the kind's record by name, `function.id` for one inside a record: how a recipe is
/// written. Compiled, it is the field's `Slot`.
pub type Path = &'static str;

#[derive(Clone, Copy)]
pub enum Op<F: Copy + 'static = Slot> {
	/// The node's `type`, first.
	Type(&'static str),
	/// The `type` is the enum field's name: a keyword type.
	TypeOf(F),
	Node(&'static str, F),
	/// Null when missing.
	Opt(&'static str, F),
	/// What TypeScript adds: undefined when missing, a list empty.
	Maybe(&'static str, F),
	/// A key TypeScript adds, read from the node's extras record where the kind's own has no room.
	Extra(&'static str, F),
	List(&'static str, F),
	/// Left out when missing.
	OptListKey(&'static str, F),
	/// A parameter list: erasing drops a leading `this`.
	Params(&'static str, F),
	Bool(&'static str, F),
	/// An interned string.
	Str(&'static str, F),
	OptStrKey(&'static str, F),
	/// The enum field's name as a string.
	Enum(&'static str, F),
	/// A mapped type's modifier: `+` or `-` as a string, bare as `true`, undefined when missing.
	Modifier(&'static str, F),
	/// One of two names by a bool: the first when true.
	BoolNames(&'static str, F, &'static [&'static str; 2]),
	/// A word as a number.
	Int(&'static str, F),
	/// Two words as a list of two numbers.
	Pair(&'static str, F),
	/// A number literal's value: the float, or null when not finite.
	Float(&'static str, F),
	/// `raw`: the node's source text.
	Raw,
	/// `bigint`: the literal's decimal digits.
	BigInt,
	/// A regular expression literal's value, null in JSON because JSON cannot hold one.
	RegExp(&'static str, F, F),
	/// A BigInt literal's value, null in JSON because JSON cannot hold one.
	BigIntValue(&'static str),
	ConstBool(&'static str, bool),
	Null(&'static str),
	EmptyList(&'static str),
	/// An object of its own under the key.
	Object(&'static str, &'static [Op]),
	/// A specifier's other name: names only when it is the binding node, the second field.
	OtherName(&'static str, F, F),
	/// Erasing lists the node under `typescript` by this name.
	Keep(&'static str),
	/// The same when the enum of two names is at the second.
	KeepIf(&'static str, F),
	/// Erasing, the node gives way to this child, which takes its facts over.
	Through(F),
	/// The enum field's name, or the given one when missing.
	EnumOr(&'static str, F, &'static str),
}

/// A field's place: its byte offset from the record's start and its type.
#[derive(Clone, Copy)]
pub struct Slot {
	pub at: usize,
	pub ty: &'static Ty,
}

use Op::*;

/// Recipes by name.
pub type Table = &'static [(&'static str, &'static [Op])];

/// What else an operation carries, past its key and field, as the layout tells it.
pub enum Rest {
	Nothing,
	/// The binding's field of `othername`, the flags' of `regexp`.
	Second(Slot),
	Bool(bool),
	/// The names for true and false.
	Names(&'static str, &'static str),
	/// What `enumor` spells for no name.
	Other(&'static str),
	Ops(&'static [Op]),
}

impl Op {
	/// The operation as the layout tells it: its name, its key, its field and the rest.
	#[rustfmt::skip]
	pub fn told(&self) -> (&'static str, &'static str, Option<Slot>, Rest) {
		match *self {
			Type(t) => ("type", t, None, Rest::Nothing),
			TypeOf(f) => ("typeof", "", Some(f), Rest::Nothing),
			Node(k, f) => ("node", k, Some(f), Rest::Nothing),
			Opt(k, f) => ("opt", k, Some(f), Rest::Nothing),
			Maybe(k, f) => ("maybe", k, Some(f), Rest::Nothing),
			Extra(k, f) => ("extra", k, Some(f), Rest::Nothing),
			List(k, f) => ("list", k, Some(f), Rest::Nothing),
			OptListKey(k, f) => ("optlistkey", k, Some(f), Rest::Nothing),
			Params(k, f) => ("params", k, Some(f), Rest::Nothing),
			Bool(k, f) => ("bool", k, Some(f), Rest::Nothing),
			Str(k, f) => ("str", k, Some(f), Rest::Nothing),
			OptStrKey(k, f) => ("optstrkey", k, Some(f), Rest::Nothing),
			Enum(k, f) => ("enum", k, Some(f), Rest::Nothing),
			Modifier(k, f) => ("modifier", k, Some(f), Rest::Nothing),
			BoolNames(k, f, &[yes, no]) => ("boolnames", k, Some(f), Rest::Names(yes, no)),
			Int(k, f) => ("int", k, Some(f), Rest::Nothing),
			Pair(k, f) => ("pair", k, Some(f), Rest::Nothing),
			Float(k, f) => ("float", k, Some(f), Rest::Nothing),
			Raw => ("raw", "", None, Rest::Nothing),
			BigInt => ("bigint", "", None, Rest::Nothing),
			BigIntValue(k) => ("bigintvalue", k, None, Rest::Nothing),
			RegExp(k, pattern, flags) => ("regexp", k, Some(pattern), Rest::Second(flags)),
			ConstBool(k, value) => ("constbool", k, None, Rest::Bool(value)),
			Null(k) => ("null", k, None, Rest::Nothing),
			EmptyList(k) => ("emptylist", k, None, Rest::Nothing),
			Object(k, inner) => ("object", k, None, Rest::Ops(inner)),
			OtherName(k, f, binding) => ("othername", k, Some(f), Rest::Second(binding)),
			Keep(name) => ("keep", name, None, Rest::Nothing),
			KeepIf(name, f) => ("keepif", name, Some(f), Rest::Nothing),
			Through(f) => ("through", "", Some(f), Rest::Nothing),
			EnumOr(k, f, other) => ("enumor", k, Some(f), Rest::Other(other)),
		}
	}
}

/// Whether a JavaScript or TypeScript node has this `type`.
pub fn names_type(name: &str) -> bool {
	KINDS
		.iter()
		.any(|(_, ops)| ops.iter().any(|op| matches!(op, Type(t) if *t == name)))
		|| crate::ast::Keyword::NAMES.contains(&name)
}

const fn same(a: &[u8], b: &[u8]) -> bool {
	let mut i = 0;
	while i < a.len() && i < b.len() && a[i] == b[i] {
		i += 1;
	}
	i == a.len() && i == b.len()
}

/// The field at a path in a record's fields.
const fn slot(mut fields: &'static [Field], path: Path) -> Slot {
	let (mut path, mut at) = (path.as_bytes(), 0);
	loop {
		let mut dot = 0;
		while dot < path.len() && path[dot] != b'.' {
			dot += 1;
		}
		let (name, rest) = path.split_at(dot);
		let mut i = 0;
		while i < fields.len() && !same(fields[i].name.as_bytes(), name) {
			i += 1;
		}
		assert!(i < fields.len(), "a recipe names a field its record does not have");
		at += fields[i].at;
		match (rest, &fields[i].ty) {
			([], ty) => return Slot { at, ty },
			([_, inner @ ..], Ty::Struct(inside)) => (path, fields) = (inner, inside),
			_ => panic!("a recipe's path goes through a field that is not a record"),
		}
	}
}

/// An operation with its fields found; one whose type its operation cannot read does not compile.
const fn resolved(op: Op<Path>, fields: &'static [Field]) -> Op {
	macro_rules! at {
		($path:expr, $ty:pat) => {
			at!($path, $ty, fields)
		};
		($path:expr, $ty:pat, $fields:expr) => {{
			let slot = slot($fields, $path);
			assert!(matches!(slot.ty, $ty), "a field has the wrong type for its recipe");
			slot
		}};
	}
	match op {
		Type(name) => Type(name),
		TypeOf(f) => TypeOf(at!(f, Ty::Enum(_))),
		Node(k, f) => Node(k, at!(f, Ty::Node)),
		Opt(k, f) => Opt(k, at!(f, Ty::OptNode | Ty::OptStr | Ty::OptU32)),
		Maybe(k, f) => Maybe(k, at!(f, Ty::OptNode | Ty::OptList | Ty::OptEnum(_))),
		Extra(k, f) => Extra(k, at!(f, Ty::OptNode | Ty::OptList | Ty::OptEnum(_), Extras::FIELDS)),
		List(k, f) => List(k, at!(f, Ty::List)),
		OptListKey(k, f) => OptListKey(k, at!(f, Ty::OptList)),
		Params(k, f) => Params(k, at!(f, Ty::List)),
		Bool(k, f) => Bool(k, at!(f, Ty::Bool)),
		Str(k, f) => Str(k, at!(f, Ty::Str)),
		OptStrKey(k, f) => OptStrKey(k, at!(f, Ty::OptStr)),
		Enum(k, f) => Enum(k, at!(f, Ty::Enum(_))),
		Modifier(k, f) => Modifier(k, at!(f, Ty::OptEnum(_))),
		BoolNames(k, f, names) => BoolNames(k, at!(f, Ty::Bool), names),
		Int(k, f) => Int(k, at!(f, Ty::U32)),
		Pair(k, f) => Pair(k, at!(f, Ty::Pair)),
		Float(k, f) => Float(k, at!(f, Ty::U32)),
		Raw => Raw,
		BigInt => BigInt,
		BigIntValue(k) => BigIntValue(k),
		RegExp(k, f, g) => RegExp(k, at!(f, Ty::Str), at!(g, Ty::Str)),
		ConstBool(k, value) => ConstBool(k, value),
		Null(k) => Null(k),
		EmptyList(k) => EmptyList(k),
		Object(k, inner) => Object(k, inner),
		OtherName(k, f, g) => OtherName(k, at!(f, Ty::Node), at!(g, Ty::Node)),
		Keep(name) => Keep(name),
		KeepIf(name, f) => KeepIf(name, at!(f, Ty::Enum([_, _]))),
		Through(f) => Through(at!(f, Ty::Node)),
		EnumOr(k, f, other) => EnumOr(k, at!(f, Ty::OptEnum(_)), other),
	}
}

const fn compiled<const N: usize>(raw: &[Op<Path>], fields: &'static [Field]) -> [Op; N] {
	let (mut out, mut i) = ([Raw; N], 0);
	while i < N {
		out[i] = resolved(raw[i], fields);
		i += 1;
	}
	out
}

/// Recipes written with paths, each compiled against the fields `$of` finds for its name.
macro_rules! recipes {
	($of:ident: $( ($name:literal, $ops:expr) ),* $(,)?) => {
		&[ $( ($name, {
			const RAW: &[Op<Path>] = $ops;
			const OPS: [Op; RAW.len()] = compiled(RAW, $of($name));
			&OPS as &[Op]
		}) ),* ]
	};
}

/// A kind's fields.
const fn kind(name: &str) -> &'static [Field] {
	let mut v = 0;
	while !same(VARIANTS[v].name.as_bytes(), name.as_bytes()) {
		v += 1;
		assert!(v < VARIANTS.len(), "a recipe names a kind there is none of");
	}
	VARIANTS[v].fields
}

/// The extras record, for a kind there is.
const fn extras(name: &str) -> &'static [Field] {
	kind(name);
	Extras::FIELDS
}

/// A table's record.
const fn row(name: &str) -> &'static [Field] {
	let mut i = 0;
	while !same(ROWS[i].0.as_bytes(), name.as_bytes()) {
		i += 1;
	}
	ROWS[i].2
}

/// Every kind in tag order, by the variant's name; `Host` has none, the writer spells it itself.
pub const KINDS: Table = recipes! { kind:
	("Program", &[Type("Program"), List("body", "body"), BoolNames("sourceType", "module", &["module", "script"])]),
	("Identifier", &[Type("Identifier"), Str("name", "name")]),
	("PrivateIdentifier", &[Type("PrivateIdentifier"), Str("name", "name")]),
	("NumberLiteral", &[Type("Literal"), Float("value", "value"), Raw]),
	("BigIntLiteral", &[Type("Literal"), BigIntValue("value"), Raw, BigInt]),
	("StringLiteral", &[Type("Literal"), Str("value", "value"), Raw]),
	("BooleanLiteral", &[Type("Literal"), Bool("value", "value"), Raw]),
	("NullLiteral", &[Type("Literal"), Null("value"), Raw]),
	("RegExpLiteral", &[Type("Literal"), RegExp("value", "pattern", "flags"), Raw, Object("regex", REGEX)]),
	("TemplateLiteral", &[Type("TemplateLiteral"), List("expressions", "expressions"), List("quasis", "quasis")]),
	("TemplateElement", &[Type("TemplateElement"), Object("value", VALUE), Bool("tail", "tail")]),
	("TaggedTemplateExpression", &[Type("TaggedTemplateExpression"), Node("tag", "tag"), Node("quasi", "quasi")]),
	("ThisExpression", &[Type("ThisExpression")]),
	("Super", &[Type("Super")]),
	("ArrayExpression", &[Type("ArrayExpression"), List("elements", "elements")]),
	("ObjectExpression", &[Type("ObjectExpression"), List("properties", "properties")]),
	("Property", &[Type("Property"), Bool("method", "method"), Bool("shorthand", "shorthand"), Bool("computed", "computed"), Node("key", "key"), Node("value", "value"), Enum("kind", "kind")]),
	("SpreadElement", &[Type("SpreadElement"), Node("argument", "argument")]),
	("UnaryExpression", &[Type("UnaryExpression"), Enum("operator", "operator"), ConstBool("prefix", true), Node("argument", "argument")]),
	("UpdateExpression", &[Type("UpdateExpression"), Enum("operator", "operator"), Bool("prefix", "prefix"), Node("argument", "argument")]),
	("BinaryExpression", &[Type("BinaryExpression"), Node("left", "left"), Enum("operator", "operator"), Node("right", "right")]),
	("LogicalExpression", &[Type("LogicalExpression"), Node("left", "left"), Enum("operator", "operator"), Node("right", "right")]),
	("AssignmentExpression", &[Type("AssignmentExpression"), Enum("operator", "operator"), Node("left", "left"), Node("right", "right")]),
	("ConditionalExpression", &[Type("ConditionalExpression"), Node("test", "test"), Node("consequent", "consequent"), Node("alternate", "alternate")]),
	("MemberExpression", &[Type("MemberExpression"), Node("object", "object"), Node("property", "property"), Bool("computed", "computed"), Bool("optional", "optional")]),
	("CallExpression", &[Type("CallExpression"), Node("callee", "callee"), List("arguments", "arguments"), Bool("optional", "optional")]),
	("ChainExpression", &[Type("ChainExpression"), Node("expression", "expression")]),
	("NewExpression", &[Type("NewExpression"), Node("callee", "callee"), List("arguments", "arguments")]),
	("SequenceExpression", &[Type("SequenceExpression"), List("expressions", "expressions")]),
	("ArrowFunctionExpression", &[Type("ArrowFunctionExpression"), Null("id"), Bool("expression", "expression"), ConstBool("generator", false), Bool("async", "is_async"), Params("params", "params"), Node("body", "body")]),
	("FunctionExpression", &[Type("FunctionExpression"), Opt("id", "function.id"), ConstBool("expression", false), Bool("generator", "function.generator"), Bool("async", "function.is_async"), Params("params", "function.params"), Node("body", "function.body")]),
	("FunctionDeclaration", &[Type("FunctionDeclaration"), Opt("id", "function.id"), ConstBool("expression", false), Bool("generator", "function.generator"), Bool("async", "function.is_async"), Params("params", "function.params"), Node("body", "function.body")]),
	("ClassExpression", &[Type("ClassExpression"), Opt("id", "class.id"), Opt("superClass", "class.super_class"), Node("body", "class.body")]),
	("ClassDeclaration", &[Type("ClassDeclaration"), Opt("id", "class.id"), Opt("superClass", "class.super_class"), Node("body", "class.body")]),
	("ClassBody", &[Type("ClassBody"), List("body", "body")]),
	("MethodDefinition", &[Type("MethodDefinition"), Bool("static", "is_static"), Bool("computed", "computed"), Node("key", "key"), Enum("kind", "kind"), Node("value", "value")]),
	("PropertyDefinition", &[TypeOf("kind"), Bool("static", "is_static"), Bool("computed", "computed"), Node("key", "key"), Opt("value", "value"), KeepIf("AccessorProperty", "kind")]),
	("StaticBlock", &[Type("StaticBlock"), List("body", "body")]),
	("YieldExpression", &[Type("YieldExpression"), Bool("delegate", "delegate"), Opt("argument", "argument")]),
	("AwaitExpression", &[Type("AwaitExpression"), Node("argument", "argument")]),
	("MetaProperty", &[Type("MetaProperty"), Node("meta", "meta"), Node("property", "property")]),
	("ImportExpression", &[Type("ImportExpression"), Node("source", "source"), Opt("options", "options")]),
	("ObjectPattern", &[Type("ObjectPattern"), List("properties", "properties")]),
	("ArrayPattern", &[Type("ArrayPattern"), List("elements", "elements")]),
	("RestElement", &[Type("RestElement"), Node("argument", "argument")]),
	("AssignmentPattern", &[Type("AssignmentPattern"), Node("left", "left"), Node("right", "right")]),
	("ExpressionStatement", &[Type("ExpressionStatement"), Node("expression", "expression"), OptStrKey("directive", "directive")]),
	("BlockStatement", &[Type("BlockStatement"), List("body", "body")]),
	("EmptyStatement", &[Type("EmptyStatement")]),
	("DebuggerStatement", &[Type("DebuggerStatement")]),
	("WithStatement", &[Type("WithStatement"), Node("object", "object"), Node("body", "body")]),
	("ReturnStatement", &[Type("ReturnStatement"), Opt("argument", "argument")]),
	("LabeledStatement", &[Type("LabeledStatement"), Node("body", "body"), Node("label", "label")]),
	("BreakStatement", &[Type("BreakStatement"), Opt("label", "label")]),
	("ContinueStatement", &[Type("ContinueStatement"), Opt("label", "label")]),
	("IfStatement", &[Type("IfStatement"), Node("test", "test"), Node("consequent", "consequent"), Opt("alternate", "alternate")]),
	("SwitchStatement", &[Type("SwitchStatement"), Node("discriminant", "discriminant"), List("cases", "cases")]),
	("SwitchCase", &[Type("SwitchCase"), List("consequent", "consequent"), Opt("test", "test")]),
	("ThrowStatement", &[Type("ThrowStatement"), Node("argument", "argument")]),
	("TryStatement", &[Type("TryStatement"), Node("block", "block"), Opt("handler", "handler"), Opt("finalizer", "finalizer")]),
	("CatchClause", &[Type("CatchClause"), Opt("param", "param"), Node("body", "body")]),
	("WhileStatement", &[Type("WhileStatement"), Node("test", "test"), Node("body", "body")]),
	("DoWhileStatement", &[Type("DoWhileStatement"), Node("body", "body"), Node("test", "test")]),
	("ForStatement", &[Type("ForStatement"), Opt("init", "init"), Opt("test", "test"), Opt("update", "update"), Node("body", "body")]),
	("ForInStatement", &[Type("ForInStatement"), Node("left", "left"), Node("right", "right"), Node("body", "body")]),
	("ForOfStatement", &[Type("ForOfStatement"), Bool("await", "is_await"), Node("left", "left"), Node("right", "right"), Node("body", "body")]),
	("VariableDeclaration", &[Type("VariableDeclaration"), List("declarations", "declarations"), Enum("kind", "kind")]),
	("VariableDeclarator", &[Type("VariableDeclarator"), Node("id", "id"), Opt("init", "init")]),
	("ImportDeclaration", &[Type("ImportDeclaration"), List("specifiers", "specifiers"), Node("source", "source"), List("attributes", "attributes")]),
	("ImportSpecifier", &[Type("ImportSpecifier"), OtherName("imported", "imported", "local"), Node("local", "local")]),
	("ImportDefaultSpecifier", &[Type("ImportDefaultSpecifier"), Node("local", "local")]),
	("ImportNamespaceSpecifier", &[Type("ImportNamespaceSpecifier"), Node("local", "local")]),
	("ImportAttribute", &[Type("ImportAttribute"), Node("key", "key"), Node("value", "value")]),
	("ExportDeclaration", &[Type("ExportNamedDeclaration"), Node("declaration", "declaration"), EmptyList("specifiers"), Null("source"), EmptyList("attributes")]),
	("ExportNamedDeclaration", &[Type("ExportNamedDeclaration"), Null("declaration"), List("specifiers", "specifiers"), Opt("source", "source"), List("attributes", "attributes")]),
	("ExportSpecifier", &[Type("ExportSpecifier"), Node("local", "local"), OtherName("exported", "exported", "local")]),
	("ExportDefaultDeclaration", &[Type("ExportDefaultDeclaration"), Node("declaration", "declaration")]),
	("ExportAllDeclaration", &[Type("ExportAllDeclaration"), Opt("exported", "exported"), Node("source", "source"), List("attributes", "attributes")]),
	("Host", &[]),
	("TypeAnnotation", &[Type("TSTypeAnnotation"), Node("typeAnnotation", "type_annotation")]),
	("Keyword", &[TypeOf("0")]),
	("ThisType", &[Type("TSThisType")]),
	("TypePredicate", &[Type("TSTypePredicate"), Node("parameterName", "parameter_name"), Opt("typeAnnotation", "type_annotation"), Bool("asserts", "asserts")]),
	("TypeReference", &[Type("TSTypeReference"), Node("typeName", "type_name"), Maybe("typeArguments", "type_arguments")]),
	("QualifiedName", &[Type("TSQualifiedName"), Node("left", "left"), Node("right", "right")]),
	("TypeParameterInstantiation", &[Type("TSTypeParameterInstantiation"), List("params", "params")]),
	("TypeParameterDeclaration", &[Type("TSTypeParameterDeclaration"), List("params", "params")]),
	("TypeParameter", &[Type("TSTypeParameter"), Bool("in", "is_in"), Bool("out", "is_out"), Bool("const", "is_const"), Str("name", "name"), Maybe("constraint", "constraint"), Maybe("default", "default")]),
	("FunctionType", &[Type("TSFunctionType"), Maybe("typeParameters", "type_parameters"), List("parameters", "parameters"), Node("typeAnnotation", "type_annotation")]),
	("ConstructorType", &[Type("TSConstructorType"), Bool("abstract", "is_abstract"), Maybe("typeParameters", "type_parameters"), List("parameters", "parameters"), Node("typeAnnotation", "type_annotation")]),
	("UnionType", &[Type("TSUnionType"), List("types", "types")]),
	("IntersectionType", &[Type("TSIntersectionType"), List("types", "types")]),
	("TypeOperator", &[Type("TSTypeOperator"), Str("operator", "operator"), Node("typeAnnotation", "type_annotation")]),
	("InferType", &[Type("TSInferType"), Node("typeParameter", "type_parameter")]),
	("LiteralType", &[Type("TSLiteralType"), Node("literal", "literal")]),
	("ImportType", &[Type("TSImportType"), Node("argument", "argument"), Opt("qualifier", "qualifier"), Opt("typeArguments", "type_arguments")]),
	("TypeQuery", &[Type("TSTypeQuery"), Node("exprName", "expr_name"), Maybe("typeArguments", "type_arguments")]),
	("MappedType", &[Type("TSMappedType"), Modifier("readonly", "readonly"), Node("typeParameter", "type_parameter"), Opt("nameType", "name_type"), Modifier("optional", "optional"), Maybe("typeAnnotation", "type_annotation")]),
	("TypeLiteral", &[Type("TSTypeLiteral"), List("members", "members")]),
	("NamedTupleMember", &[Type("TSNamedTupleMember"), Bool("optional", "optional"), Node("label", "label"), Node("elementType", "element_type")]),
	("OptionalType", &[Type("TSOptionalType"), Node("typeAnnotation", "type_annotation")]),
	("RestType", &[Type("TSRestType"), Node("typeAnnotation", "type_annotation")]),
	("TupleType", &[Type("TSTupleType"), List("elementTypes", "element_types")]),
	("ParenthesizedType", &[Type("TSParenthesizedType"), Node("typeAnnotation", "type_annotation")]),
	("ArrayType", &[Type("TSArrayType"), Node("elementType", "element_type")]),
	("IndexedAccessType", &[Type("TSIndexedAccessType"), Node("objectType", "object_type"), Node("indexType", "index_type")]),
	("ConditionalType", &[Type("TSConditionalType"), Node("checkType", "check_type"), Node("extendsType", "extends_type"), Node("trueType", "true_type"), Node("falseType", "false_type")]),
	("IndexSignature", &[Type("TSIndexSignature"), List("parameters", "parameters"), Maybe("typeAnnotation", "type_annotation")]),
	("CallSignatureDeclaration", &[Type("TSCallSignatureDeclaration"), Maybe("typeParameters", "type_parameters"), List("parameters", "parameters"), Maybe("typeAnnotation", "type_annotation")]),
	("ConstructSignatureDeclaration", &[Type("TSConstructSignatureDeclaration"), Maybe("typeParameters", "type_parameters"), List("parameters", "parameters"), Maybe("typeAnnotation", "type_annotation")]),
	("MethodSignature", &[Type("TSMethodSignature"), Node("key", "key"), Bool("computed", "computed"), Bool("optional", "optional"), Enum("kind", "kind"), Extra("typeParameters", "type_parameters"), List("parameters", "parameters"), Maybe("typeAnnotation", "type_annotation")]),
	("PropertySignature", &[Type("TSPropertySignature"), Node("key", "key"), Bool("computed", "computed"), Bool("optional", "optional"), Bool("readonly", "readonly"), Maybe("typeAnnotation", "type_annotation")]),
	("InterfaceDeclaration", &[Type("TSInterfaceDeclaration"), Node("id", "id"), Maybe("typeParameters", "type_parameters"), List("extends", "extends"), Node("body", "body")]),
	("InterfaceBody", &[Type("TSInterfaceBody"), List("body", "body")]),
	("Heritage", &[TypeOf("clause"), Node("expression", "expression"), Maybe("typeArguments", "type_arguments")]),
	("EnumDeclaration", &[Keep("TSEnumDeclaration"), Type("TSEnumDeclaration"), Bool("const", "is_const"), Node("id", "id"), List("members", "members")]),
	("EnumMember", &[Type("TSEnumMember"), Node("id", "id"), Maybe("initializer", "initializer")]),
	("ModuleDeclaration", &[Keep("TSModuleDeclaration"), Type("TSModuleDeclaration"), Bool("global", "global"), Node("id", "id"), Maybe("body", "body")]),
	("ModuleBlock", &[Type("TSModuleBlock"), List("body", "body")]),
	("TypeAliasDeclaration", &[Type("TSTypeAliasDeclaration"), Node("id", "id"), Maybe("typeParameters", "type_parameters"), Node("typeAnnotation", "type_annotation")]),
	("ImportEqualsDeclaration", &[Keep("TSImportEqualsDeclaration"), Type("TSImportEqualsDeclaration"), Enum("importKind", "import_kind"), Bool("isExport", "is_export"), Node("id", "id"), Node("moduleReference", "module_reference")]),
	("ExternalModuleReference", &[Type("TSExternalModuleReference"), Node("expression", "expression")]),
	("ExportAssignment", &[Keep("TSExportAssignment"), Type("TSExportAssignment"), Node("expression", "expression")]),
	("NamespaceExportDeclaration", &[Type("TSNamespaceExportDeclaration"), Node("id", "id")]),
	("DeclareFunction", &[Type("TSDeclareFunction"), Opt("id", "id"), Bool("generator", "generator"), Bool("async", "is_async"), ConstBool("expression", false), List("params", "params")]),
	("DeclareMethod", &[Type("TSDeclareMethod"), Null("id"), Bool("generator", "generator"), Bool("async", "is_async"), ConstBool("expression", false), List("params", "params")]),
	("AsExpression", &[Through("expression"), Type("TSAsExpression"), Node("expression", "expression"), Node("typeAnnotation", "type_annotation")]),
	("SatisfiesExpression", &[Through("expression"), Type("TSSatisfiesExpression"), Node("expression", "expression"), Node("typeAnnotation", "type_annotation")]),
	("NonNullExpression", &[Through("expression"), Type("TSNonNullExpression"), Node("expression", "expression")]),
	("TypeAssertion", &[Through("expression"), Type("TSTypeAssertion"), Node("typeAnnotation", "type_annotation"), Node("expression", "expression")]),
	("InstantiationExpression", &[Through("expression"), Type("TSInstantiationExpression"), Node("expression", "expression"), Node("typeArguments", "type_arguments")]),
	("ParameterProperty", &[Keep("TSParameterProperty"), Through("parameter"), Type("TSParameterProperty"), Node("parameter", "parameter")]),
	("Decorator", &[Keep("Decorator"), Type("Decorator"), Node("expression", "expression")]),
};

// a node's tag finds its recipe
const _: () = {
	assert!(KINDS.len() == VARIANTS.len(), "every kind has one recipe");
	let mut v = 0;
	while v < KINDS.len() {
		assert!(
			same(KINDS[v].0.as_bytes(), VARIANTS[v].name.as_bytes()),
			"the recipes follow the kinds' order"
		);
		v += 1;
	}
};

const VALUE: &[Op] = &[
	resolved(Str("raw", "raw"), kind("TemplateElement")),
	resolved(Opt("cooked", "cooked"), kind("TemplateElement")),
];
const REGEX: &[Op] = &[
	resolved(Str("pattern", "pattern"), kind("RegExpLiteral")),
	resolved(Str("flags", "flags"), kind("RegExpLiteral")),
];

/// What TypeScript adds to every node of a kind from its extras record, by the kind's name.
pub const ADDS: Table = recipes! { extras:
	("Identifier", PATTERN),
	("ArrayPattern", PATTERN),
	("ObjectPattern", PATTERN),
	("AssignmentPattern", PATTERN),
	("RestElement", PATTERN),
	("ArrowFunctionExpression", &[Maybe("returnType", "return_type"), Maybe("typeParameters", "type_parameters")]),
	("FunctionDeclaration", FUNCTION),
	("FunctionExpression", FUNCTION),
	("ClassDeclaration", CLASS),
	("ClassExpression", CLASS),
	("MethodDefinition", &[Maybe("typeParameters", "type_parameters"), Maybe("decorators", "decorators"), Maybe("accessibility", "accessibility"), Bool("optional", "optional"), Bool("abstract", "is_abstract"), Bool("override", "is_override")]),
	("PropertyDefinition", &[Maybe("typeAnnotation", "type_annotation"), Maybe("decorators", "decorators"), Maybe("accessibility", "accessibility"), Bool("optional", "optional"), Bool("definite", "definite"), Bool("declare", "declare"), Bool("abstract", "is_abstract"), Bool("readonly", "readonly"), Bool("override", "is_override")]),
	("Property", &[Bool("optional", "optional")]),
	("CallExpression", &[Maybe("typeArguments", "type_arguments")]),
	("NewExpression", &[Maybe("typeArguments", "type_arguments")]),
	("TaggedTemplateExpression", &[Maybe("typeArguments", "type_arguments")]),
	("VariableDeclaration", &[Bool("declare", "declare")]),
	("VariableDeclarator", &[Bool("definite", "definite")]),
	("ImportDeclaration", &[EnumOr("importKind", "import_kind", "value")]),
	("ImportSpecifier", &[EnumOr("importKind", "import_kind", "value")]),
	("ExportDeclaration", &[EnumOr("exportKind", "export_kind", "value")]),
	("ExportNamedDeclaration", &[EnumOr("exportKind", "export_kind", "value")]),
	("ExportDefaultDeclaration", &[EnumOr("exportKind", "export_kind", "value")]),
	("ExportAllDeclaration", &[EnumOr("exportKind", "export_kind", "value")]),
	("ExportSpecifier", &[EnumOr("exportKind", "export_kind", "value")]),
	("PropertySignature", &[Maybe("accessibility", "accessibility"), Bool("static", "is_static")]),
	("MethodSignature", &[Maybe("accessibility", "accessibility"), Bool("readonly", "readonly"), Bool("static", "is_static")]),
	("IndexSignature", &[Maybe("accessibility", "accessibility"), Bool("readonly", "readonly"), Bool("static", "is_static")]),
	("InterfaceDeclaration", &[Bool("declare", "declare")]),
	("TypeAliasDeclaration", &[Bool("declare", "declare")]),
	("EnumDeclaration", &[Bool("declare", "declare")]),
	("ModuleDeclaration", &[Bool("declare", "declare")]),
	("DeclareFunction", &[Maybe("returnType", "return_type"), Maybe("typeParameters", "type_parameters"), Bool("declare", "declare")]),
	("DeclareMethod", &[Maybe("returnType", "return_type")]),
	("ParameterProperty", &[Maybe("accessibility", "accessibility"), Bool("readonly", "readonly"), Bool("override", "is_override"), Bool("static", "is_static")]),
};

#[rustfmt::skip]
const PATTERN: &[Op<Path>] = &[Maybe("typeAnnotation", "type_annotation"), Maybe("decorators", "decorators"), Bool("optional", "optional")];
#[rustfmt::skip]
const FUNCTION: &[Op<Path>] = &[Maybe("returnType", "return_type"), Maybe("typeParameters", "type_parameters"), Bool("declare", "declare")];
#[rustfmt::skip]
const CLASS: &[Op<Path>] = &[Maybe("typeParameters", "type_parameters"), Maybe("superTypeArguments", "super_type_arguments"), Maybe("implements", "implements"), Maybe("decorators", "decorators"), Bool("declare", "declare"), Bool("abstract", "is_abstract")];

/// The same when erasing: decorators, a proposal JavaScript itself has.
pub const EXTRAS_ERASED: &[Op] = &[resolved(OptListKey("decorators", "decorators"), Extras::FIELDS)];

/// How each table's rows are spelled: the recipes of the answer's `scopes`, `bindings`,
/// `references` and `roots`.
pub const RECIPES: Table = recipes! { row:
	("scopes", &[Enum("kind", "kind"), Opt("parent", "parent"), Bool("topLevelAwait", "top_level_await")]),
	("bindings", &[Str("name", "name"), Enum("kind", "kind"), Int("scope", "scope"), Bool("write", "write")]),
	("references", &[Int("scope", "scope"), Opt("binding", "binding"), Bool("write", "write"), Bool("read", "read"), Bool("mutate", "mutate"), Bool("declares", "declares")]),
	("roots", &[Int("scope", "scope"), Pair("scopes", "scopes"), Pair("bindings", "bindings"), Pair("references", "references")]),
};

/// Each table's name, its record's size and fields, in the order of `RECIPES`.
pub const ROWS: &[(&str, usize, &[Field])] = &[
	("scopes", size_of::<Scope>(), Scope::FIELDS),
	("bindings", size_of::<Binding>(), Binding::FIELDS),
	("references", size_of::<Reference>(), Reference::FIELDS),
	("roots", size_of::<Root>(), Root::FIELDS),
];
