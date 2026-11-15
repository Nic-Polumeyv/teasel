//! How each kind of node, each table's rows and what TypeScript adds are spelled in ESTree: a
//! recipe of operations over the fields of a record, each field found by the compiler. The JSON
//! writer follows them, and the decoder building objects from the tree in place, so the two
//! cannot drift.

use crate::ast::Extras;
use crate::ast::node_layout::{Tag, VARIANTS};
use crate::layout::{Described, Ty};
use crate::scopes::{Binding, Reference, Root, Scope};

#[derive(Clone, Copy)]
pub enum Op {
	/// The node's `type`, first.
	Type(&'static str),
	/// The `type` is the enum field's name: a keyword type.
	TypeOf(Slot),
	Node(&'static str, Slot),
	/// Null when missing.
	Opt(&'static str, Slot),
	/// What TypeScript adds: undefined when missing, a list empty.
	Maybe(&'static str, Slot),
	/// A key TypeScript adds, read from the node's extras record where the kind's own has no room.
	Extra(&'static str, ExtraSlot),
	List(&'static str, Slot),
	/// Left out when missing.
	OptListKey(&'static str, Slot),
	/// A parameter list: erasing drops a leading `this`.
	Params(&'static str, Slot),
	Bool(&'static str, Slot),
	/// An interned string.
	Str(&'static str, Slot),
	OptStrKey(&'static str, Slot),
	/// The enum field's name as a string.
	Enum(&'static str, Slot),
	/// A mapped type's modifier: `+` or `-` as a string, bare as `true`, undefined when missing.
	Modifier(&'static str, Slot),
	/// One of two names by a bool: the first when true.
	BoolNames(&'static str, Slot, &'static [&'static str; 2]),
	/// A word as a number.
	Int(&'static str, Slot),
	/// Two words as a list of two numbers.
	Pair(&'static str, Slot),
	/// A number literal's value: the float, or null when not finite.
	Float(&'static str, Slot),
	/// `raw`: the node's source text.
	Raw,
	/// `bigint`: the literal's decimal digits.
	BigInt,
	/// A regular expression literal's value, null in JSON because JSON cannot hold one.
	RegExp(&'static str, Slot, Slot),
	/// A BigInt literal's value, null in JSON because JSON cannot hold one.
	BigIntValue(&'static str),
	ConstBool(&'static str, bool),
	Null(&'static str),
	EmptyList(&'static str),
	/// An object of its own under the key.
	Object(&'static str, &'static [Op]),
	/// A specifier's other name: names only when it is the binding node, the second field.
	OtherName(&'static str, Slot, Slot),
	/// Erasing lists the node under `typescript` by this name.
	Keep(&'static str),
	/// The same when the enum of two names is at the second.
	KeepIf(&'static str, Slot),
	/// Erasing, the node gives way to this child, which takes its facts over.
	Through(Slot),
	/// The enum field's name, or the given one when missing.
	EnumOr(&'static str, Slot, &'static str),
}

/// A field's place: its byte offset from the record's start and its type.
#[derive(Clone, Copy)]
pub struct Slot {
	pub at: usize,
	pub ty: &'static Ty,
}

/// A field's place in the extras record.
#[derive(Clone, Copy)]
pub struct ExtraSlot(pub Slot);

use Op::*;

/// A recipe for each kind, by tag.
pub type ByKind = [&'static [Op]; VARIANTS.len()];

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
			Extra(k, ExtraSlot(f)) => ("extra", k, Some(f), Rest::Nothing),
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
		.any(|ops| ops.iter().any(|op| matches!(op, Type(t) if *t == name)))
		|| crate::ast::Keyword::NAMES.contains(&name)
}

const fn ty_of<R, T: Described>(_: fn(&R) -> &T) -> &'static Ty {
	&T::TY
}

/// A field's place in `Record`, the record in scope.
macro_rules! at {
	($($field:tt).+) => {
		Slot {
			at: std::mem::offset_of!(Record, $($field).+),
			ty: ty_of(|record: &Record| &record.$($field).+),
		}
	};
}

macro_rules! extra {
	($field:ident) => {{
		type Record = Extras;
		ExtraSlot(at!($field))
	}};
}

/// A recipe over the fields of a record; a field of a type its operation cannot read does not compile.
macro_rules! of {
	($record:ty; $ops:expr) => {{
		#[allow(dead_code)]
		type Record = $record;
		fits($ops)
	}};
}

const fn fits(ops: &'static [Op]) -> &'static [Op] {
	let mut i = 0;
	while i < ops.len() {
		let fits = match ops[i] {
			Type(_) | Raw | BigInt | BigIntValue(_) | ConstBool(..) | Null(_) | EmptyList(_) | Keep(_) => true,
			Node(_, slot) | Through(slot) => matches!(slot.ty, Ty::Node),
			OtherName(_, slot, binding) => matches!((slot.ty, binding.ty), (Ty::Node, Ty::Node)),
			Opt(_, slot) => matches!(slot.ty, Ty::OptNode | Ty::OptStr | Ty::OptU32),
			Maybe(_, slot) | Extra(_, ExtraSlot(slot)) => matches!(slot.ty, Ty::OptNode | Ty::OptList | Ty::OptEnum(_)),
			List(_, slot) | Params(_, slot) => matches!(slot.ty, Ty::List),
			OptListKey(_, slot) => matches!(slot.ty, Ty::OptList),
			Bool(_, slot) | BoolNames(_, slot, _) => matches!(slot.ty, Ty::Bool),
			Str(_, slot) => matches!(slot.ty, Ty::Str),
			RegExp(_, pattern, flags) => matches!((pattern.ty, flags.ty), (Ty::Str, Ty::Str)),
			OptStrKey(_, slot) => matches!(slot.ty, Ty::OptStr),
			TypeOf(slot) | Enum(_, slot) => matches!(slot.ty, Ty::Enum(_)),
			KeepIf(_, slot) => matches!(slot.ty, Ty::Enum([_, _])),
			Modifier(_, slot) | EnumOr(_, slot, _) => matches!(slot.ty, Ty::OptEnum(_)),
			Int(_, slot) | Float(_, slot) => matches!(slot.ty, Ty::U32),
			Pair(_, slot) => matches!(slot.ty, Ty::Pair),
			Object(_, inner) => {
				fits(inner);
				true
			}
		};
		assert!(fits, "a field has the wrong type for its operation");
		i += 1;
	}
	ops
}

/// How many kinds are named, each once.
const fn once(tags: &[usize]) -> usize {
	let mut seen = [false; VARIANTS.len()];
	let mut i = 0;
	while i < tags.len() {
		assert!(!seen[tags[i]], "a kind has two recipes");
		seen[tags[i]] = true;
		i += 1;
	}
	tags.len()
}

/// A recipe by tag for each kind named and how many were named; the fields are those of the
/// kind's own record, or of the record named first.
macro_rules! by_kind {
	(@ $( $kind:ident: $ops:expr ),*) => {{
		let mut out: ByKind = [&[]; VARIANTS.len()];
		$( out[Tag::$kind as usize] = $ops; )*
		(out, once(&[$( Tag::$kind as usize ),*]))
	}};
	($record:ty; $( $kind:ident: $ops:expr ),* $(,)?) => {
		by_kind!(@ $( $kind: of!($record; $ops) ),*)
	};
	($( $kind:ident: $ops:expr ),* $(,)?) => {
		by_kind!(@ $( $kind: of!(crate::ast::node_layout::$kind::Record; $ops) ),*)
	};
}

/// Every kind's recipe; `Host` has none, the writer spells it itself.
pub static KINDS: ByKind = {
	let (kinds, named) = by_kind! {
		Program: &[Type("Program"), List("body", at!(body)), BoolNames("sourceType", at!(module), &["module", "script"])],
		Identifier: &[Type("Identifier"), Str("name", at!(name))],
		PrivateIdentifier: &[Type("PrivateIdentifier"), Str("name", at!(name))],
		NumberLiteral: &[Type("Literal"), Float("value", at!(value)), Raw],
		BigIntLiteral: &[Type("Literal"), BigIntValue("value"), Raw, BigInt],
		StringLiteral: &[Type("Literal"), Str("value", at!(value)), Raw],
		BooleanLiteral: &[Type("Literal"), Bool("value", at!(value)), Raw],
		NullLiteral: &[Type("Literal"), Null("value"), Raw],
		RegExpLiteral: &[Type("Literal"), RegExp("value", at!(pattern), at!(flags)), Raw, Object("regex", &[Str("pattern", at!(pattern)), Str("flags", at!(flags))])],
		TemplateLiteral: &[Type("TemplateLiteral"), List("expressions", at!(expressions)), List("quasis", at!(quasis))],
		TemplateElement: &[Type("TemplateElement"), Object("value", &[Str("raw", at!(raw)), Opt("cooked", at!(cooked))]), Bool("tail", at!(tail))],
		TaggedTemplateExpression: &[Type("TaggedTemplateExpression"), Node("tag", at!(tag)), Node("quasi", at!(quasi))],
		ThisExpression: &[Type("ThisExpression")],
		Super: &[Type("Super")],
		ArrayExpression: &[Type("ArrayExpression"), List("elements", at!(elements))],
		ObjectExpression: &[Type("ObjectExpression"), List("properties", at!(properties))],
		Property: &[Type("Property"), Bool("method", at!(method)), Bool("shorthand", at!(shorthand)), Bool("computed", at!(computed)), Node("key", at!(key)), Node("value", at!(value)), Enum("kind", at!(kind))],
		SpreadElement: &[Type("SpreadElement"), Node("argument", at!(argument))],
		UnaryExpression: &[Type("UnaryExpression"), Enum("operator", at!(operator)), ConstBool("prefix", true), Node("argument", at!(argument))],
		UpdateExpression: &[Type("UpdateExpression"), Enum("operator", at!(operator)), Bool("prefix", at!(prefix)), Node("argument", at!(argument))],
		BinaryExpression: &[Type("BinaryExpression"), Node("left", at!(left)), Enum("operator", at!(operator)), Node("right", at!(right))],
		LogicalExpression: &[Type("LogicalExpression"), Node("left", at!(left)), Enum("operator", at!(operator)), Node("right", at!(right))],
		AssignmentExpression: &[Type("AssignmentExpression"), Enum("operator", at!(operator)), Node("left", at!(left)), Node("right", at!(right))],
		ConditionalExpression: &[Type("ConditionalExpression"), Node("test", at!(test)), Node("consequent", at!(consequent)), Node("alternate", at!(alternate))],
		MemberExpression: &[Type("MemberExpression"), Node("object", at!(object)), Node("property", at!(property)), Bool("computed", at!(computed)), Bool("optional", at!(optional))],
		CallExpression: &[Type("CallExpression"), Node("callee", at!(callee)), List("arguments", at!(arguments)), Bool("optional", at!(optional))],
		ChainExpression: &[Type("ChainExpression"), Node("expression", at!(expression))],
		NewExpression: &[Type("NewExpression"), Node("callee", at!(callee)), List("arguments", at!(arguments))],
		SequenceExpression: &[Type("SequenceExpression"), List("expressions", at!(expressions))],
		ArrowFunctionExpression: &[Type("ArrowFunctionExpression"), Null("id"), Bool("expression", at!(expression)), ConstBool("generator", false), Bool("async", at!(is_async)), Params("params", at!(params)), Node("body", at!(body))],
		FunctionExpression: &[Type("FunctionExpression"), Opt("id", at!(function.id)), ConstBool("expression", false), Bool("generator", at!(function.generator)), Bool("async", at!(function.is_async)), Params("params", at!(function.params)), Node("body", at!(function.body))],
		FunctionDeclaration: &[Type("FunctionDeclaration"), Opt("id", at!(function.id)), ConstBool("expression", false), Bool("generator", at!(function.generator)), Bool("async", at!(function.is_async)), Params("params", at!(function.params)), Node("body", at!(function.body))],
		ClassExpression: &[Type("ClassExpression"), Opt("id", at!(class.id)), Opt("superClass", at!(class.super_class)), Node("body", at!(class.body))],
		ClassDeclaration: &[Type("ClassDeclaration"), Opt("id", at!(class.id)), Opt("superClass", at!(class.super_class)), Node("body", at!(class.body))],
		ClassBody: &[Type("ClassBody"), List("body", at!(body))],
		MethodDefinition: &[Type("MethodDefinition"), Bool("static", at!(is_static)), Bool("computed", at!(computed)), Node("key", at!(key)), Enum("kind", at!(kind)), Node("value", at!(value))],
		PropertyDefinition: &[TypeOf(at!(kind)), Bool("static", at!(is_static)), Bool("computed", at!(computed)), Node("key", at!(key)), Opt("value", at!(value)), KeepIf("AccessorProperty", at!(kind))],
		StaticBlock: &[Type("StaticBlock"), List("body", at!(body))],
		YieldExpression: &[Type("YieldExpression"), Bool("delegate", at!(delegate)), Opt("argument", at!(argument))],
		AwaitExpression: &[Type("AwaitExpression"), Node("argument", at!(argument))],
		MetaProperty: &[Type("MetaProperty"), Node("meta", at!(meta)), Node("property", at!(property))],
		ImportExpression: &[Type("ImportExpression"), Node("source", at!(source)), Opt("options", at!(options))],
		ObjectPattern: &[Type("ObjectPattern"), List("properties", at!(properties))],
		ArrayPattern: &[Type("ArrayPattern"), List("elements", at!(elements))],
		RestElement: &[Type("RestElement"), Node("argument", at!(argument))],
		AssignmentPattern: &[Type("AssignmentPattern"), Node("left", at!(left)), Node("right", at!(right))],
		ExpressionStatement: &[Type("ExpressionStatement"), Node("expression", at!(expression)), OptStrKey("directive", at!(directive))],
		BlockStatement: &[Type("BlockStatement"), List("body", at!(body))],
		EmptyStatement: &[Type("EmptyStatement")],
		DebuggerStatement: &[Type("DebuggerStatement")],
		WithStatement: &[Type("WithStatement"), Node("object", at!(object)), Node("body", at!(body))],
		ReturnStatement: &[Type("ReturnStatement"), Opt("argument", at!(argument))],
		LabeledStatement: &[Type("LabeledStatement"), Node("body", at!(body)), Node("label", at!(label))],
		BreakStatement: &[Type("BreakStatement"), Opt("label", at!(label))],
		ContinueStatement: &[Type("ContinueStatement"), Opt("label", at!(label))],
		IfStatement: &[Type("IfStatement"), Node("test", at!(test)), Node("consequent", at!(consequent)), Opt("alternate", at!(alternate))],
		SwitchStatement: &[Type("SwitchStatement"), Node("discriminant", at!(discriminant)), List("cases", at!(cases))],
		SwitchCase: &[Type("SwitchCase"), List("consequent", at!(consequent)), Opt("test", at!(test))],
		ThrowStatement: &[Type("ThrowStatement"), Node("argument", at!(argument))],
		TryStatement: &[Type("TryStatement"), Node("block", at!(block)), Opt("handler", at!(handler)), Opt("finalizer", at!(finalizer))],
		CatchClause: &[Type("CatchClause"), Opt("param", at!(param)), Node("body", at!(body))],
		WhileStatement: &[Type("WhileStatement"), Node("test", at!(test)), Node("body", at!(body))],
		DoWhileStatement: &[Type("DoWhileStatement"), Node("body", at!(body)), Node("test", at!(test))],
		ForStatement: &[Type("ForStatement"), Opt("init", at!(init)), Opt("test", at!(test)), Opt("update", at!(update)), Node("body", at!(body))],
		ForInStatement: &[Type("ForInStatement"), Node("left", at!(left)), Node("right", at!(right)), Node("body", at!(body))],
		ForOfStatement: &[Type("ForOfStatement"), Bool("await", at!(is_await)), Node("left", at!(left)), Node("right", at!(right)), Node("body", at!(body))],
		VariableDeclaration: &[Type("VariableDeclaration"), List("declarations", at!(declarations)), Enum("kind", at!(kind))],
		VariableDeclarator: &[Type("VariableDeclarator"), Node("id", at!(id)), Opt("init", at!(init))],
		ImportDeclaration: &[Type("ImportDeclaration"), List("specifiers", at!(specifiers)), Node("source", at!(source)), List("attributes", at!(attributes))],
		ImportSpecifier: &[Type("ImportSpecifier"), OtherName("imported", at!(imported), at!(local)), Node("local", at!(local))],
		ImportDefaultSpecifier: &[Type("ImportDefaultSpecifier"), Node("local", at!(local))],
		ImportNamespaceSpecifier: &[Type("ImportNamespaceSpecifier"), Node("local", at!(local))],
		ImportAttribute: &[Type("ImportAttribute"), Node("key", at!(key)), Node("value", at!(value))],
		ExportDeclaration: &[Type("ExportNamedDeclaration"), Node("declaration", at!(declaration)), EmptyList("specifiers"), Null("source"), EmptyList("attributes")],
		ExportNamedDeclaration: &[Type("ExportNamedDeclaration"), Null("declaration"), List("specifiers", at!(specifiers)), Opt("source", at!(source)), List("attributes", at!(attributes))],
		ExportSpecifier: &[Type("ExportSpecifier"), Node("local", at!(local)), OtherName("exported", at!(exported), at!(local))],
		ExportDefaultDeclaration: &[Type("ExportDefaultDeclaration"), Node("declaration", at!(declaration))],
		ExportAllDeclaration: &[Type("ExportAllDeclaration"), Opt("exported", at!(exported)), Node("source", at!(source)), List("attributes", at!(attributes))],
		Host: &[],
		TypeAnnotation: &[Type("TSTypeAnnotation"), Node("typeAnnotation", at!(type_annotation))],
		Keyword: &[TypeOf(at!(f0))],
		ThisType: &[Type("TSThisType")],
		TypePredicate: &[Type("TSTypePredicate"), Node("parameterName", at!(parameter_name)), Opt("typeAnnotation", at!(type_annotation)), Bool("asserts", at!(asserts))],
		TypeReference: &[Type("TSTypeReference"), Node("typeName", at!(type_name)), Maybe("typeArguments", at!(type_arguments))],
		QualifiedName: &[Type("TSQualifiedName"), Node("left", at!(left)), Node("right", at!(right))],
		TypeParameterInstantiation: &[Type("TSTypeParameterInstantiation"), List("params", at!(params))],
		TypeParameterDeclaration: &[Type("TSTypeParameterDeclaration"), List("params", at!(params))],
		TypeParameter: &[Type("TSTypeParameter"), Bool("in", at!(is_in)), Bool("out", at!(is_out)), Bool("const", at!(is_const)), Str("name", at!(name)), Maybe("constraint", at!(constraint)), Maybe("default", at!(default))],
		FunctionType: &[Type("TSFunctionType"), Maybe("typeParameters", at!(type_parameters)), List("parameters", at!(parameters)), Node("typeAnnotation", at!(type_annotation))],
		ConstructorType: &[Type("TSConstructorType"), Bool("abstract", at!(is_abstract)), Maybe("typeParameters", at!(type_parameters)), List("parameters", at!(parameters)), Node("typeAnnotation", at!(type_annotation))],
		UnionType: &[Type("TSUnionType"), List("types", at!(types))],
		IntersectionType: &[Type("TSIntersectionType"), List("types", at!(types))],
		TypeOperator: &[Type("TSTypeOperator"), Str("operator", at!(operator)), Node("typeAnnotation", at!(type_annotation))],
		InferType: &[Type("TSInferType"), Node("typeParameter", at!(type_parameter))],
		LiteralType: &[Type("TSLiteralType"), Node("literal", at!(literal))],
		ImportType: &[Type("TSImportType"), Node("argument", at!(argument)), Opt("qualifier", at!(qualifier)), Opt("typeArguments", at!(type_arguments))],
		TypeQuery: &[Type("TSTypeQuery"), Node("exprName", at!(expr_name)), Maybe("typeArguments", at!(type_arguments))],
		MappedType: &[Type("TSMappedType"), Modifier("readonly", at!(readonly)), Node("typeParameter", at!(type_parameter)), Opt("nameType", at!(name_type)), Modifier("optional", at!(optional)), Maybe("typeAnnotation", at!(type_annotation))],
		TypeLiteral: &[Type("TSTypeLiteral"), List("members", at!(members))],
		NamedTupleMember: &[Type("TSNamedTupleMember"), Bool("optional", at!(optional)), Node("label", at!(label)), Node("elementType", at!(element_type))],
		OptionalType: &[Type("TSOptionalType"), Node("typeAnnotation", at!(type_annotation))],
		RestType: &[Type("TSRestType"), Node("typeAnnotation", at!(type_annotation))],
		TupleType: &[Type("TSTupleType"), List("elementTypes", at!(element_types))],
		ParenthesizedType: &[Type("TSParenthesizedType"), Node("typeAnnotation", at!(type_annotation))],
		ArrayType: &[Type("TSArrayType"), Node("elementType", at!(element_type))],
		IndexedAccessType: &[Type("TSIndexedAccessType"), Node("objectType", at!(object_type)), Node("indexType", at!(index_type))],
		ConditionalType: &[Type("TSConditionalType"), Node("checkType", at!(check_type)), Node("extendsType", at!(extends_type)), Node("trueType", at!(true_type)), Node("falseType", at!(false_type))],
		IndexSignature: &[Type("TSIndexSignature"), List("parameters", at!(parameters)), Maybe("typeAnnotation", at!(type_annotation))],
		CallSignatureDeclaration: &[Type("TSCallSignatureDeclaration"), Maybe("typeParameters", at!(type_parameters)), List("parameters", at!(parameters)), Maybe("typeAnnotation", at!(type_annotation))],
		ConstructSignatureDeclaration: &[Type("TSConstructSignatureDeclaration"), Maybe("typeParameters", at!(type_parameters)), List("parameters", at!(parameters)), Maybe("typeAnnotation", at!(type_annotation))],
		MethodSignature: &[Type("TSMethodSignature"), Node("key", at!(key)), Bool("computed", at!(computed)), Bool("optional", at!(optional)), Enum("kind", at!(kind)), Extra("typeParameters", extra!(type_parameters)), List("parameters", at!(parameters)), Maybe("typeAnnotation", at!(type_annotation))],
		PropertySignature: &[Type("TSPropertySignature"), Node("key", at!(key)), Bool("computed", at!(computed)), Bool("optional", at!(optional)), Bool("readonly", at!(readonly)), Maybe("typeAnnotation", at!(type_annotation))],
		InterfaceDeclaration: &[Type("TSInterfaceDeclaration"), Node("id", at!(id)), Maybe("typeParameters", at!(type_parameters)), List("extends", at!(extends)), Node("body", at!(body))],
		InterfaceBody: &[Type("TSInterfaceBody"), List("body", at!(body))],
		Heritage: &[TypeOf(at!(clause)), Node("expression", at!(expression)), Maybe("typeArguments", at!(type_arguments))],
		EnumDeclaration: &[Keep("TSEnumDeclaration"), Type("TSEnumDeclaration"), Bool("const", at!(is_const)), Node("id", at!(id)), List("members", at!(members))],
		EnumMember: &[Type("TSEnumMember"), Node("id", at!(id)), Maybe("initializer", at!(initializer))],
		ModuleDeclaration: &[Keep("TSModuleDeclaration"), Type("TSModuleDeclaration"), Bool("global", at!(global)), Node("id", at!(id)), Maybe("body", at!(body))],
		ModuleBlock: &[Type("TSModuleBlock"), List("body", at!(body))],
		TypeAliasDeclaration: &[Type("TSTypeAliasDeclaration"), Node("id", at!(id)), Maybe("typeParameters", at!(type_parameters)), Node("typeAnnotation", at!(type_annotation))],
		ImportEqualsDeclaration: &[Keep("TSImportEqualsDeclaration"), Type("TSImportEqualsDeclaration"), Enum("importKind", at!(import_kind)), Bool("isExport", at!(is_export)), Node("id", at!(id)), Node("moduleReference", at!(module_reference))],
		ExternalModuleReference: &[Type("TSExternalModuleReference"), Node("expression", at!(expression))],
		ExportAssignment: &[Keep("TSExportAssignment"), Type("TSExportAssignment"), Node("expression", at!(expression))],
		NamespaceExportDeclaration: &[Type("TSNamespaceExportDeclaration"), Node("id", at!(id))],
		DeclareFunction: &[Type("TSDeclareFunction"), Opt("id", at!(id)), Bool("generator", at!(generator)), Bool("async", at!(is_async)), ConstBool("expression", false), List("params", at!(params))],
		DeclareMethod: &[Type("TSDeclareMethod"), Null("id"), Bool("generator", at!(generator)), Bool("async", at!(is_async)), ConstBool("expression", false), List("params", at!(params))],
		AsExpression: &[Through(at!(expression)), Type("TSAsExpression"), Node("expression", at!(expression)), Node("typeAnnotation", at!(type_annotation))],
		SatisfiesExpression: &[Through(at!(expression)), Type("TSSatisfiesExpression"), Node("expression", at!(expression)), Node("typeAnnotation", at!(type_annotation))],
		NonNullExpression: &[Through(at!(expression)), Type("TSNonNullExpression"), Node("expression", at!(expression))],
		TypeAssertion: &[Through(at!(expression)), Type("TSTypeAssertion"), Node("typeAnnotation", at!(type_annotation)), Node("expression", at!(expression))],
		InstantiationExpression: &[Through(at!(expression)), Type("TSInstantiationExpression"), Node("expression", at!(expression)), Node("typeArguments", at!(type_arguments))],
		ParameterProperty: &[Keep("TSParameterProperty"), Through(at!(parameter)), Type("TSParameterProperty"), Node("parameter", at!(parameter))],
		Decorator: &[Keep("Decorator"), Type("Decorator"), Node("expression", at!(expression))],
	};
	assert!(named == VARIANTS.len(), "every kind has one recipe");
	kinds
};

/// What TypeScript adds to every node of a kind from its extras record.
pub static ADDS: ByKind = by_kind! { Extras;
	Identifier: PATTERN,
	ArrayPattern: PATTERN,
	ObjectPattern: PATTERN,
	AssignmentPattern: PATTERN,
	RestElement: PATTERN,
	ArrowFunctionExpression: &[Maybe("returnType", at!(return_type)), Maybe("typeParameters", at!(type_parameters))],
	FunctionDeclaration: FUNCTION,
	FunctionExpression: FUNCTION,
	ClassDeclaration: CLASS,
	ClassExpression: CLASS,
	MethodDefinition: &[Maybe("typeParameters", at!(type_parameters)), Maybe("decorators", at!(decorators)), Maybe("accessibility", at!(accessibility)), Bool("optional", at!(optional)), Bool("abstract", at!(is_abstract)), Bool("override", at!(is_override))],
	PropertyDefinition: &[Maybe("typeAnnotation", at!(type_annotation)), Maybe("decorators", at!(decorators)), Maybe("accessibility", at!(accessibility)), Bool("optional", at!(optional)), Bool("definite", at!(definite)), Bool("declare", at!(declare)), Bool("abstract", at!(is_abstract)), Bool("readonly", at!(readonly)), Bool("override", at!(is_override))],
	Property: &[Bool("optional", at!(optional))],
	CallExpression: &[Maybe("typeArguments", at!(type_arguments))],
	NewExpression: &[Maybe("typeArguments", at!(type_arguments))],
	TaggedTemplateExpression: &[Maybe("typeArguments", at!(type_arguments))],
	VariableDeclaration: &[Bool("declare", at!(declare))],
	VariableDeclarator: &[Bool("definite", at!(definite))],
	ImportDeclaration: &[EnumOr("importKind", at!(import_kind), "value")],
	ImportSpecifier: &[EnumOr("importKind", at!(import_kind), "value")],
	ExportDeclaration: &[EnumOr("exportKind", at!(export_kind), "value")],
	ExportNamedDeclaration: &[EnumOr("exportKind", at!(export_kind), "value")],
	ExportDefaultDeclaration: &[EnumOr("exportKind", at!(export_kind), "value")],
	ExportAllDeclaration: &[EnumOr("exportKind", at!(export_kind), "value")],
	ExportSpecifier: &[EnumOr("exportKind", at!(export_kind), "value")],
	PropertySignature: &[Maybe("accessibility", at!(accessibility)), Bool("static", at!(is_static))],
	MethodSignature: &[Maybe("accessibility", at!(accessibility)), Bool("readonly", at!(readonly)), Bool("static", at!(is_static))],
	IndexSignature: &[Maybe("accessibility", at!(accessibility)), Bool("readonly", at!(readonly)), Bool("static", at!(is_static))],
	InterfaceDeclaration: &[Bool("declare", at!(declare))],
	TypeAliasDeclaration: &[Bool("declare", at!(declare))],
	EnumDeclaration: &[Bool("declare", at!(declare))],
	ModuleDeclaration: &[Bool("declare", at!(declare))],
	DeclareFunction: &[Maybe("returnType", at!(return_type)), Maybe("typeParameters", at!(type_parameters)), Bool("declare", at!(declare))],
	DeclareMethod: &[Maybe("returnType", at!(return_type))],
	ParameterProperty: &[Maybe("accessibility", at!(accessibility)), Bool("readonly", at!(readonly)), Bool("override", at!(is_override)), Bool("static", at!(is_static))],
}
.0;

const PATTERN: &[Op] = of!(Extras; &[Maybe("typeAnnotation", at!(type_annotation)), Maybe("decorators", at!(decorators)), Bool("optional", at!(optional))]);
const FUNCTION: &[Op] = of!(Extras; &[Maybe("returnType", at!(return_type)), Maybe("typeParameters", at!(type_parameters)), Bool("declare", at!(declare))]);
const CLASS: &[Op] = of!(Extras; &[Maybe("typeParameters", at!(type_parameters)), Maybe("superTypeArguments", at!(super_type_arguments)), Maybe("implements", at!(implements)), Maybe("decorators", at!(decorators)), Bool("declare", at!(declare)), Bool("abstract", at!(is_abstract))]);

/// The same when erasing: decorators, a proposal JavaScript itself has.
pub const EXTRAS_ERASED: &[Op] = of!(Extras; &[OptListKey("decorators", at!(decorators))]);

/// Each table of the answer, its `scopes`, `bindings`, `references` and `roots`: its name, its
/// record's size and how its rows are spelled.
#[rustfmt::skip]
pub const ROWS: &[(&str, usize, &[Op])] = &[
	("scopes", size_of::<Scope>(), of!(Scope; &[Enum("kind", at!(kind)), Opt("parent", at!(parent)), Bool("topLevelAwait", at!(top_level_await))])),
	("bindings", size_of::<Binding>(), of!(Binding; &[Str("name", at!(name)), Enum("kind", at!(kind)), Int("scope", at!(scope)), Bool("write", at!(write))])),
	("references", size_of::<Reference>(), of!(Reference; &[Int("scope", at!(scope)), Opt("binding", at!(binding)), Bool("write", at!(write)), Bool("read", at!(read)), Bool("mutate", at!(mutate)), Bool("declares", at!(declares))])),
	("roots", size_of::<Root>(), of!(Root; &[Int("scope", at!(scope)), Pair("scopes", at!(scopes)), Pair("bindings", at!(bindings)), Pair("references", at!(references))])),
];
