//! How each kind of node, each table's rows and what TypeScript adds are spelled in ESTree: a
//! recipe of operations over the fields the layout describes, followed by the JSON writer and by
//! a front end building objects from the tree in place, so the two cannot drift.

use crate::layout::{Field, Ty, Variant};
use crate::scopes::{Binding, Reference, Root, Scope};

/// A field of the kind's record by name, `function.id` for one inside a record; a `Slot` once
/// resolved against the layout.
pub type Path = &'static str;

#[derive(Clone, Copy)]
pub enum Op<F: Copy + 'static = Path> {
	/// The node's `type`, first.
	Type(&'static str),
	/// The `type` is the enum field's name: a keyword type.
	TypeOf(F),
	Node(&'static str, F),
	/// Null when missing.
	Opt(&'static str, F),
	/// What TypeScript adds: undefined when missing, a list empty.
	Maybe(&'static str, F),
	List(&'static str, F),
	/// Left out when missing.
	OptListKey(&'static str, F),
	Bool(&'static str, F),
	/// An interned string.
	Str(&'static str, F),
	OptStrKey(&'static str, F),
	/// The enum field's name as a string.
	Enum(&'static str, F),
	/// A mapped type's modifier: `+` or `-` as a string, bare as `true`, undefined when missing.
	Modifier(&'static str, F),
	/// One of two names by a bool: the first when true.
	BoolNames(&'static str, F, &'static str, &'static str),
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
	Object(&'static str, &'static [Op<F>]),
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
	Names(&'static str, &'static str),
	Bool(bool),
	Text(&'static str),
	Ops(&'static [Op]),
}

impl Op {
	/// The operation as the layout tells it: its name, its key, its field and the rest.
	pub fn told(&self) -> (&'static str, Option<&'static str>, Option<Path>, Rest) {
		match *self {
			Type(t) => ("type", Some(t), None, Rest::Nothing),
			TypeOf(f) => ("typeof", None, Some(f), Rest::Nothing),
			Node(k, f) => ("node", Some(k), Some(f), Rest::Nothing),
			Opt(k, f) => ("opt", Some(k), Some(f), Rest::Nothing),
			Maybe(k, f) => ("maybe", Some(k), Some(f), Rest::Nothing),
			List(k, f) => ("list", Some(k), Some(f), Rest::Nothing),
			OptListKey(k, f) => ("optlistkey", Some(k), Some(f), Rest::Nothing),
			Bool(k, f) => ("bool", Some(k), Some(f), Rest::Nothing),
			Str(k, f) => ("str", Some(k), Some(f), Rest::Nothing),
			OptStrKey(k, f) => ("optstrkey", Some(k), Some(f), Rest::Nothing),
			Enum(k, f) => ("enum", Some(k), Some(f), Rest::Nothing),
			Modifier(k, f) => ("modifier", Some(k), Some(f), Rest::Nothing),
			BoolNames(k, f, yes, no) => ("boolnames", Some(k), Some(f), Rest::Names(yes, no)),
			Int(k, f) => ("int", Some(k), Some(f), Rest::Nothing),
			Pair(k, f) => ("pair", Some(k), Some(f), Rest::Nothing),
			Float(k, f) => ("float", Some(k), Some(f), Rest::Nothing),
			Raw => ("raw", None, None, Rest::Nothing),
			BigInt => ("bigint", None, None, Rest::Nothing),
			BigIntValue(k) => ("bigintvalue", Some(k), None, Rest::Nothing),
			RegExp(k, pattern, flags) => ("regexp", Some(k), Some(pattern), Rest::Text(flags)),
			ConstBool(k, value) => ("constbool", Some(k), None, Rest::Bool(value)),
			Null(k) => ("null", Some(k), None, Rest::Nothing),
			EmptyList(k) => ("emptylist", Some(k), None, Rest::Nothing),
			Object(k, inner) => ("object", Some(k), None, Rest::Ops(inner)),
			OtherName(k, f, binding) => ("othername", Some(k), Some(f), Rest::Text(binding)),
			Keep(name) => ("keep", Some(name), None, Rest::Nothing),
			KeepIf(name, f) => ("keepif", Some(name), Some(f), Rest::Nothing),
			Through(f) => ("through", None, Some(f), Rest::Nothing),
			EnumOr(k, f, other) => ("enumor", Some(k), Some(f), Rest::Text(other)),
		}
	}
}

/// Whether a JavaScript or TypeScript node has this `type`.
pub fn names_type(name: &str) -> bool {
	let spelled = |table: &[(&str, &[Op])]| {
		table
			.iter()
			.any(|(_, ops)| ops.iter().any(|op| matches!(op, Type(t) if *t == name)))
	};
	if spelled(TS) || crate::typescript::ast::Keyword::NAMES.contains(&name) {
		return true;
	}
	spelled(JS)
}

/// The JavaScript kinds, by the variant's name; `Extension` and `Host` have none, the writer
/// spells them itself.
#[rustfmt::skip]
pub const JS: &[(&str, &[Op])] = &[
	("Program", &[Type("Program"), List("body", "body"), BoolNames("sourceType", "module", "module", "script")]),
	("Identifier", &[Type("Identifier"), Str("name", "name")]),
	("PrivateIdentifier", &[Type("PrivateIdentifier"), Str("name", "name")]),
	("NumberLiteral", &[Type("Literal"), Float("value", "value"), Raw]),
	("BigIntLiteral", &[Type("Literal"), BigIntValue("value"), Raw, BigInt]),
	("StringLiteral", &[Type("Literal"), Str("value", "value"), Raw]),
	("BooleanLiteral", &[Type("Literal"), Bool("value", "value"), Raw]),
	("NullLiteral", &[Type("Literal"), Null("value"), Raw]),
	("RegExpLiteral", &[Type("Literal"), RegExp("value", "pattern", "flags"), Raw, Object("regex", &[Str("pattern", "pattern"), Str("flags", "flags")])]),
	("TemplateLiteral", &[Type("TemplateLiteral"), List("expressions", "expressions"), List("quasis", "quasis")]),
	("TemplateElement", &[Type("TemplateElement"), Object("value", &[Str("raw", "raw"), Opt("cooked", "cooked")]), Bool("tail", "tail")]),
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
	("ArrowFunctionExpression", &[Type("ArrowFunctionExpression"), Null("id"), Bool("expression", "expression"), ConstBool("generator", false), Bool("async", "is_async"), List("params", "params"), Node("body", "body")]),
	("FunctionExpression", &[Type("FunctionExpression"), Opt("id", "function.id"), ConstBool("expression", false), Bool("generator", "function.generator"), Bool("async", "function.is_async"), List("params", "function.params"), Node("body", "function.body")]),
	("FunctionDeclaration", &[Type("FunctionDeclaration"), Opt("id", "function.id"), ConstBool("expression", false), Bool("generator", "function.generator"), Bool("async", "function.is_async"), List("params", "function.params"), Node("body", "function.body")]),
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
			Maybe(k, f) => Maybe(
				k,
				slot(f, &[|t| matches!(t, Ty::OptNode | Ty::OptList | Ty::OptEnum(_))]),
			),
			List(k, f) => List(k, slot(f, &[|t| matches!(t, Ty::List)])),
			OptListKey(k, f) => OptListKey(k, slot(f, &[|t| matches!(t, Ty::OptList)])),
			Bool(k, f) => Bool(k, slot(f, &[|t| matches!(t, Ty::Bool)])),
			Str(k, f) => Str(k, slot(f, &[|t| matches!(t, Ty::Str)])),
			OptStrKey(k, f) => OptStrKey(k, slot(f, &[|t| matches!(t, Ty::OptStr)])),
			Enum(k, f) => Enum(k, slot(f, &[|t| matches!(t, Ty::Enum(_))])),
			Modifier(k, f) => Modifier(k, slot(f, &[|t| matches!(t, Ty::OptEnum(_))])),
			BoolNames(k, f, a, b) => BoolNames(k, slot(f, &[|t| matches!(t, Ty::Bool)]), a, b),
			Int(k, f) => Int(k, slot(f, &[|t| matches!(t, Ty::U32)])),
			Pair(k, f) => Pair(k, slot(f, &[|t| matches!(t, Ty::Pair)])),
			Float(k, f) => Float(k, slot(f, &[|t| matches!(t, Ty::U32)])),
			Raw => Raw,
			BigInt => BigInt,
			BigIntValue(k) => BigIntValue(k),
			RegExp(k, f, g) => RegExp(
				k,
				slot(f, &[|t| matches!(t, Ty::Str)]),
				slot(g, &[|t| matches!(t, Ty::Str)]),
			),
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
			KeepIf(name, f) => KeepIf(name, slot(f, &[|t| matches!(t, Ty::Enum([_, _]))])),
			Through(f) => Through(slot(f, &[|t| matches!(t, Ty::Node)])),
			EnumOr(k, f, other) => EnumOr(k, slot(f, &[|t| matches!(t, Ty::OptEnum(_))]), other),
		})
		.collect();
	Box::leak(resolved.into_boxed_slice())
}

/// The TypeScript kinds, by the variant's name.
#[rustfmt::skip]
pub const TS: &[(&str, &[Op])] = &[
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
	("MethodSignature", &[Type("TSMethodSignature"), Node("key", "key"), Bool("computed", "computed"), Bool("optional", "optional"), Enum("kind", "kind"), Maybe("typeParameters", "type_parameters"), List("parameters", "parameters"), Maybe("typeAnnotation", "type_annotation")]),
	("PropertySignature", &[Type("TSPropertySignature"), Node("key", "key"), Bool("computed", "computed"), Bool("optional", "optional"), Bool("readonly", "readonly"), Maybe("typeAnnotation", "type_annotation")]),
	("InterfaceDeclaration", &[Type("TSInterfaceDeclaration"), Node("id", "id"), Maybe("typeParameters", "type_parameters"), Maybe("extends", "extends"), Node("body", "body")]),
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
	("TypeCastExpression", &[Through("expression"), Type("TSTypeCastExpression"), Node("expression", "expression"), Maybe("typeAnnotation", "type_annotation")]),
	("InstantiationExpression", &[Through("expression"), Type("TSInstantiationExpression"), Node("expression", "expression"), Node("typeArguments", "type_arguments")]),
	("ParameterProperty", &[Keep("TSParameterProperty"), Through("parameter"), Type("TSParameterProperty"), Node("parameter", "parameter")]),
	("Decorator", &[Keep("Decorator"), Type("Decorator"), Node("expression", "expression")]),
];

/// What TypeScript adds to every node of a kind from its extras record, by the JavaScript kind's name.
#[rustfmt::skip]
pub const ADDS: &[(&str, &[Op])] = &[
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
];

/// The same by the TypeScript kind's name.
#[rustfmt::skip]
pub const TS_ADDS: &[(&str, &[Op])] = &[
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
];

#[rustfmt::skip]
const PATTERN: &[Op] = &[Maybe("typeAnnotation", "type_annotation"), Maybe("decorators", "decorators"), Bool("optional", "optional")];
#[rustfmt::skip]
const FUNCTION: &[Op] = &[Maybe("returnType", "return_type"), Maybe("typeParameters", "type_parameters"), Bool("declare", "declare")];
#[rustfmt::skip]
const CLASS: &[Op] = &[Maybe("typeParameters", "type_parameters"), Maybe("superTypeArguments", "super_type_arguments"), Maybe("implements", "implements"), Maybe("decorators", "decorators"), Bool("declare", "declare"), Bool("abstract", "is_abstract")];

/// The same when erasing: decorators, a proposal JavaScript itself has.
pub const EXTRAS_ERASED: &[Op] = &[OptListKey("decorators", "decorators")];

/// How each table's rows are spelled: the recipes of the answer's `scopes`, `bindings`,
/// `references` and `roots`.
#[rustfmt::skip]
pub const RECIPES: &[(&str, &[Op])] = &[
	("scopes", &[Enum("kind", "kind"), Opt("parent", "parent"), Bool("topLevelAwait", "top_level_await")]),
	("bindings", &[Str("name", "name"), Enum("kind", "kind"), Int("scope", "scope"), Bool("write", "write")]),
	("references", &[Int("scope", "scope"), Opt("binding", "binding"), Bool("write", "write"), Bool("read", "read"), Bool("mutate", "mutate"), Bool("declares", "declares")]),
	("roots", &[Int("scope", "scope"), Pair("scopes", "scopes"), Pair("bindings", "bindings"), Pair("references", "references")]),
];

/// Each table's name, its record's size and fields, in the order of `RECIPES`.
pub const ROWS: &[(&str, usize, &[Field])] = &[
	("scopes", size_of::<Scope>(), Scope::FIELDS),
	("bindings", size_of::<Binding>(), Binding::FIELDS),
	("references", size_of::<Reference>(), Reference::FIELDS),
	("roots", size_of::<Root>(), Root::FIELDS),
];
