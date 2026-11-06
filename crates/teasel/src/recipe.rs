//! How each kind of node, each table's rows and what TypeScript adds are spelled in ESTree: a
//! recipe of operations over the fields the layout describes, read off a kind's row unless ESTree
//! says more, followed by the JSON writer and by the decoder building objects from the tree in
//! place, so the two cannot drift.

use crate::ast::node_layout::VARIANTS;
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
			Extra(k, f) => ("extra", Some(k), Some(f), Rest::Nothing),
			List(k, f) => ("list", Some(k), Some(f), Rest::Nothing),
			OptListKey(k, f) => ("optlistkey", Some(k), Some(f), Rest::Nothing),
			Params(k, f) => ("params", Some(k), Some(f), Rest::Nothing),
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
	kinds()
		.iter()
		.any(|(_, ops)| ops.iter().any(|op| matches!(op, Type(t) if *t == name)))
		|| crate::ast::Keyword::NAMES.contains(&name)
}

/// Every kind by the variant's name: its recipe in `OWN` where ESTree says more than the kind's
/// row, else what the row spells.
pub fn kinds() -> &'static [(&'static str, &'static [Op])] {
	static TABLE: std::sync::OnceLock<Vec<(&'static str, &'static [Op])>> = std::sync::OnceLock::new();
	TABLE.get_or_init(|| {
		VARIANTS
			.iter()
			.map(|variant| {
				let own = OWN.iter().find(|(name, _)| *name == variant.name);
				(variant.name, own.map_or_else(|| of_row(variant), |(_, ops)| *ops))
			})
			.collect()
	})
}

/// A kind's recipe read off its row: the name is the `type`, `TS` first for a kind TypeScript
/// adds; each field is a key of its name in camel case, `is_` dropped from the front, a record's
/// fields in its place; a missing node is `null`, or `undefined` in TypeScript, where a missing
/// list is empty and a missing enum `undefined` as well.
fn of_row(variant: &Variant) -> &'static [Op] {
	fn spell(ops: &mut Vec<Op>, fields: &[Field], at: &str, typescript: bool, kind: &str) {
		for field in fields {
			let path: Path = leak(format!("{at}{}", field.name));
			if let Ty::Struct(inner) = field.ty {
				spell(ops, inner, &format!("{path}."), typescript, kind);
				continue;
			}
			let key = leak(camel(field.name.strip_prefix("is_").unwrap_or(field.name)));
			ops.push(match (field.ty, typescript) {
				(Ty::Node, _) => Node(key, path),
				(Ty::OptNode, false) => Opt(key, path),
				(Ty::OptNode | Ty::OptList | Ty::OptEnum(_), true) => Maybe(key, path),
				(Ty::List, _) => List(key, path),
				(Ty::Str, _) => Str(key, path),
				(Ty::Bool, _) => Bool(key, path),
				(Ty::Enum(_), _) => Enum(key, path),
				_ => panic!("{kind}: {path} needs a recipe of its own"),
			});
		}
	}
	let name = variant.name;
	let ty = if variant.typescript {
		leak(format!("TS{name}"))
	} else {
		name
	};
	let mut ops = vec![Type(ty)];
	spell(&mut ops, variant.fields, "", variant.typescript, name);
	Box::leak(ops.into_boxed_slice())
}

fn leak(text: String) -> &'static str {
	Box::leak(text.into_boxed_str())
}

/// `type_annotation` as `typeAnnotation`.
fn camel(name: &str) -> String {
	let mut out = String::with_capacity(name.len());
	let mut up = false;
	for c in name.chars() {
		if c == '_' {
			up = true;
		} else {
			out.push(if up { c.to_ascii_uppercase() } else { c });
			up = false;
		}
	}
	out
}

/// The kinds ESTree spells past their rows: a `Literal`, a constant key, a key renamed or left
/// out, what erasing keeps or sees through, a `type` by an enum, a field of the extras record;
/// `Host` has no recipe, the writer spells it itself.
#[rustfmt::skip]
const OWN: &[(&str, &[Op])] = &[
	("Program", &[Type("Program"), List("body", "body"), BoolNames("sourceType", "module", "module", "script")]),
	("NumberLiteral", &[Type("Literal"), Float("value", "value"), Raw]),
	("BigIntLiteral", &[Type("Literal"), BigIntValue("value"), Raw, BigInt]),
	("StringLiteral", &[Type("Literal"), Str("value", "value"), Raw]),
	("BooleanLiteral", &[Type("Literal"), Bool("value", "value"), Raw]),
	("NullLiteral", &[Type("Literal"), Null("value"), Raw]),
	("RegExpLiteral", &[Type("Literal"), RegExp("value", "pattern", "flags"), Raw, Object("regex", &[Str("pattern", "pattern"), Str("flags", "flags")])]),
	("TemplateElement", &[Type("TemplateElement"), Object("value", &[Str("raw", "raw"), Opt("cooked", "cooked")]), Bool("tail", "tail")]),
	("UnaryExpression", &[Type("UnaryExpression"), Enum("operator", "operator"), ConstBool("prefix", true), Node("argument", "argument")]),
	("ArrowFunctionExpression", &[Type("ArrowFunctionExpression"), Null("id"), Bool("expression", "expression"), ConstBool("generator", false), Bool("async", "is_async"), Params("params", "params"), Node("body", "body")]),
	("FunctionExpression", &[Type("FunctionExpression"), Opt("id", "function.id"), ConstBool("expression", false), Bool("generator", "function.generator"), Bool("async", "function.is_async"), Params("params", "function.params"), Node("body", "function.body")]),
	("FunctionDeclaration", &[Type("FunctionDeclaration"), Opt("id", "function.id"), ConstBool("expression", false), Bool("generator", "function.generator"), Bool("async", "function.is_async"), Params("params", "function.params"), Node("body", "function.body")]),
	("PropertyDefinition", &[TypeOf("kind"), Bool("static", "is_static"), Bool("computed", "computed"), Node("key", "key"), Opt("value", "value"), KeepIf("AccessorProperty", "kind")]),
	("ExpressionStatement", &[Type("ExpressionStatement"), Node("expression", "expression"), OptStrKey("directive", "directive")]),
	("ImportSpecifier", &[Type("ImportSpecifier"), OtherName("imported", "imported", "local"), Node("local", "local")]),
	("ExportDeclaration", &[Type("ExportNamedDeclaration"), Node("declaration", "declaration"), EmptyList("specifiers"), Null("source"), EmptyList("attributes")]),
	("ExportNamedDeclaration", &[Type("ExportNamedDeclaration"), Null("declaration"), List("specifiers", "specifiers"), Opt("source", "source"), List("attributes", "attributes")]),
	("ExportSpecifier", &[Type("ExportSpecifier"), Node("local", "local"), OtherName("exported", "exported", "local")]),
	("Host", &[]),
	("Keyword", &[TypeOf("0")]),
	("TypePredicate", &[Type("TSTypePredicate"), Node("parameterName", "parameter_name"), Opt("typeAnnotation", "type_annotation"), Bool("asserts", "asserts")]),
	("ImportType", &[Type("TSImportType"), Node("argument", "argument"), Opt("qualifier", "qualifier"), Opt("typeArguments", "type_arguments")]),
	("MappedType", &[Type("TSMappedType"), Modifier("readonly", "readonly"), Node("typeParameter", "type_parameter"), Opt("nameType", "name_type"), Modifier("optional", "optional"), Maybe("typeAnnotation", "type_annotation")]),
	("MethodSignature", &[Type("TSMethodSignature"), Node("key", "key"), Bool("computed", "computed"), Bool("optional", "optional"), Enum("kind", "kind"), Extra("typeParameters", "type_parameters"), List("parameters", "parameters"), Maybe("typeAnnotation", "type_annotation")]),
	("Heritage", &[TypeOf("clause"), Node("expression", "expression"), Maybe("typeArguments", "type_arguments")]),
	("EnumDeclaration", &[Keep("TSEnumDeclaration"), Type("TSEnumDeclaration"), Bool("const", "is_const"), Node("id", "id"), List("members", "members")]),
	("ModuleDeclaration", &[Keep("TSModuleDeclaration"), Type("TSModuleDeclaration"), Bool("global", "global"), Node("id", "id"), Maybe("body", "body")]),
	("ImportEqualsDeclaration", &[Keep("TSImportEqualsDeclaration"), Type("TSImportEqualsDeclaration"), Enum("importKind", "import_kind"), Bool("isExport", "is_export"), Node("id", "id"), Node("moduleReference", "module_reference")]),
	("ExportAssignment", &[Keep("TSExportAssignment"), Type("TSExportAssignment"), Node("expression", "expression")]),
	("DeclareFunction", &[Type("TSDeclareFunction"), Opt("id", "id"), Bool("generator", "generator"), Bool("async", "is_async"), ConstBool("expression", false), List("params", "params")]),
	("DeclareMethod", &[Type("TSDeclareMethod"), Null("id"), Bool("generator", "generator"), Bool("async", "is_async"), ConstBool("expression", false), List("params", "params")]),
	("AsExpression", &[Through("expression"), Type("TSAsExpression"), Node("expression", "expression"), Node("typeAnnotation", "type_annotation")]),
	("SatisfiesExpression", &[Through("expression"), Type("TSSatisfiesExpression"), Node("expression", "expression"), Node("typeAnnotation", "type_annotation")]),
	("NonNullExpression", &[Through("expression"), Type("TSNonNullExpression"), Node("expression", "expression")]),
	("TypeAssertion", &[Through("expression"), Type("TSTypeAssertion"), Node("typeAnnotation", "type_annotation"), Node("expression", "expression")]),
	("InstantiationExpression", &[Through("expression"), Type("TSInstantiationExpression"), Node("expression", "expression"), Node("typeArguments", "type_arguments")]),
	("ParameterProperty", &[Keep("TSParameterProperty"), Through("parameter"), Type("TSParameterProperty"), Node("parameter", "parameter")]),
	("Decorator", &[Keep("Decorator"), Type("Decorator"), Node("expression", "expression")]),
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
	let slot_in = |fields: &[Field], path: Path, wanted: &[fn(&Ty) -> bool]| -> Slot {
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
	let slot = |path: Path, wanted: &[fn(&Ty) -> bool]| slot_in(fields, path, wanted);
	let maybe: &[fn(&Ty) -> bool] = &[|t| matches!(t, Ty::OptNode | Ty::OptList | Ty::OptEnum(_))];
	let resolved: Vec<Op<Slot>> = ops
		.iter()
		.map(|op| match *op {
			Type(name) => Type(name),
			TypeOf(f) => TypeOf(slot(f, &[|t| matches!(t, Ty::Enum(_))])),
			Node(k, f) => Node(k, slot(f, &[|t| matches!(t, Ty::Node)])),
			Opt(k, f) => Opt(k, slot(f, &[|t| matches!(t, Ty::OptNode | Ty::OptStr | Ty::OptU32)])),
			Maybe(k, f) => Maybe(k, slot(f, maybe)),
			Extra(k, f) => Extra(k, slot_in(crate::ast::Extras::FIELDS, f, maybe)),
			List(k, f) => List(k, slot(f, &[|t| matches!(t, Ty::List)])),
			OptListKey(k, f) => OptListKey(k, slot(f, &[|t| matches!(t, Ty::OptList)])),
			Params(k, f) => Params(k, slot(f, &[|t| matches!(t, Ty::List)])),
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

/// What TypeScript adds to every node of a kind from its extras record, by the kind's name.
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
