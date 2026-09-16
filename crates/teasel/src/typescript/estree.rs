//! ESTree output for TypeScript nodes and the keys TypeScript adds to JavaScript nodes.

use super::ast::{Data, Extras, Kind, TsKind};
use crate::ast::{List, NodeId, NodeKind};
use crate::estree::{Emit, Sink, Writer};
use crate::names::c;
use crate::recipe::Op::{self, *};
use crate::recipe::Slot;

/// The TypeScript kinds, by the variant's name.
#[rustfmt::skip]
pub(crate) const TS: &[(&str, &[Op])] = &[
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
	("EnumDeclaration", &[Type(c!("TSEnumDeclaration")), BoolIf(c!("const"), "is_const"), Node(c!("id"), "id"), List(c!("members"), "members")]),
	("EnumMember", &[Type(c!("TSEnumMember")), Node(c!("id"), "id"), OptKey(c!("initializer"), "initializer")]),
	("ModuleDeclaration", &[Type(c!("TSModuleDeclaration")), BoolIf(c!("global"), "global"), Node(c!("id"), "id"), OptKey(c!("body"), "body")]),
	("ModuleBlock", &[Type(c!("TSModuleBlock")), List(c!("body"), "body")]),
	("TypeAliasDeclaration", &[Type(c!("TSTypeAliasDeclaration")), Node(c!("id"), "id"), OptKey(c!("typeParameters"), "type_parameters"), Node(c!("typeAnnotation"), "type_annotation")]),
	("ImportEqualsDeclaration", &[Type(c!("TSImportEqualsDeclaration")), Enum(c!("importKind"), "import_kind"), Bool(c!("isExport"), "is_export"), Node(c!("id"), "id"), Node(c!("moduleReference"), "module_reference")]),
	("ExternalModuleReference", &[Type(c!("TSExternalModuleReference")), Node(c!("expression"), "expression")]),
	("ExportAssignment", &[Type(c!("TSExportAssignment")), Node(c!("expression"), "expression")]),
	("NamespaceExportDeclaration", &[Type(c!("TSNamespaceExportDeclaration")), Node(c!("id"), "id")]),
	("DeclareFunction", &[Type(c!("TSDeclareFunction")), Opt(c!("id"), "id"), Bool(c!("generator"), "generator"), Bool(c!("async"), "is_async"), ConstBool(c!("expression"), false), List(c!("params"), "params")]),
	("DeclareMethod", &[Type(c!("TSDeclareMethod")), Null(c!("id")), Bool(c!("generator"), "generator"), Bool(c!("async"), "is_async"), ConstBool(c!("expression"), false), List(c!("params"), "params")]),
	("AsExpression", &[Type(c!("TSAsExpression")), Node(c!("expression"), "expression"), Node(c!("typeAnnotation"), "type_annotation")]),
	("SatisfiesExpression", &[Type(c!("TSSatisfiesExpression")), Node(c!("expression"), "expression"), Node(c!("typeAnnotation"), "type_annotation")]),
	("NonNullExpression", &[Type(c!("TSNonNullExpression")), Node(c!("expression"), "expression")]),
	("TypeAssertion", &[Type(c!("TSTypeAssertion")), Node(c!("typeAnnotation"), "type_annotation"), Node(c!("expression"), "expression")]),
	("TypeCastExpression", &[Type(c!("TSTypeCastExpression")), Node(c!("expression"), "expression"), Node(c!("typeAnnotation"), "type_annotation")]),
	("InstantiationExpression", &[Type(c!("TSInstantiationExpression")), Node(c!("expression"), "expression"), Node(c!("typeArguments"), "type_arguments")]),
	("ParameterProperty", &[Type(c!("TSParameterProperty")), Node(c!("parameter"), "parameter")]),
	("Decorator", &[Type(c!("Decorator")), Node(c!("expression"), "expression")]),
];

fn ts_recipes() -> &'static [&'static [Op<Slot>]] {
	static RESOLVED: std::sync::OnceLock<&'static [&'static [Op<Slot>]]> = std::sync::OnceLock::new();
	RESOLVED.get_or_init(|| crate::recipe::resolve(TS, super::ast::ts_layout::VARIANTS))
}

impl Data {
	/// Whether every statement of a list erases to nothing.
	fn all_erased<S: Sink>(&self, w: &Writer<Self, S>, list: List) -> bool {
		w.ast()
			.list(list)
			.iter()
			.all(|item| item.is_none_or(|id| self.erased(w, id)))
	}

	fn extras_of(&self, id: NodeId) -> Extras {
		if self.extras.is_empty() {
			return Extras::default();
		}
		self.extras(id).copied().unwrap_or_default()
	}
}

impl Emit for Data {
	fn erased<S: Sink>(&self, w: &Writer<Self, S>, id: NodeId) -> bool {
		use TsKind::*;
		let extras = self.extras_of(id);
		if extras.declare {
			return true;
		}
		match w.kind(id) {
			NodeKind::MethodDefinition { .. } | NodeKind::PropertyDefinition { .. } if extras.is_abstract => true,
			// an overload signature: a method without a body
			NodeKind::MethodDefinition { value, .. } => {
				matches!(self.ts_of(w.ast(), value), Some(DeclareMethod { .. }))
			}
			NodeKind::Extension(index) => match self.kind(index) {
				InterfaceDeclaration { .. }
				| TypeAliasDeclaration { .. }
				| DeclareFunction { .. }
				| IndexSignature { .. }
				| NamespaceExportDeclaration { .. } => true,
				ImportEqualsDeclaration { import_kind, .. } => import_kind == Kind::Type,
				ModuleDeclaration { body: None, .. } => true,
				ModuleDeclaration { body: Some(block), .. } => match self.ts_of(w.ast(), block) {
					Some(ModuleBlock { body }) => self.all_erased(w, body),
					_ => false,
				},
				_ => false,
			},
			NodeKind::ImportDeclaration { specifiers, .. } => {
				extras.import_kind == Some(Kind::Type) || (specifiers.len > 0 && self.all_erased(w, specifiers))
			}
			NodeKind::ImportSpecifier { .. } => extras.import_kind == Some(Kind::Type),
			NodeKind::ExportSpecifier { .. } | NodeKind::ExportAllDeclaration { .. } => {
				extras.export_kind == Some(Kind::Type)
			}
			// `export { type A }` keeps an `export {}`, as tsc keeps the file a module
			NodeKind::ExportDeclaration { declaration } => {
				extras.export_kind == Some(Kind::Type) || self.erased(w, declaration)
			}
			NodeKind::ExportNamedDeclaration { .. } => extras.export_kind == Some(Kind::Type),
			NodeKind::ExportDefaultDeclaration { declaration } => {
				extras.export_kind == Some(Kind::Type) || self.erased(w, declaration)
			}
			_ => false,
		}
	}

	fn node<S: Sink>(&self, w: &mut Writer<Self, S>, id: NodeId, index: u32) {
		use TsKind::*;
		if w.output.erase {
			match self.kind(index) {
				AsExpression { expression, .. }
				| SatisfiesExpression { expression, .. }
				| NonNullExpression { expression }
				| TypeAssertion { expression, .. }
				| TypeCastExpression { expression, .. }
				| InstantiationExpression { expression, .. } => {
					w.adopt(id);
					return w.node(expression);
				}
				ParameterProperty { parameter } => {
					w.keep(c!("TSParameterProperty"), id);
					return w.node(parameter);
				}
				EnumDeclaration { .. } => w.keep(c!("TSEnumDeclaration"), id),
				ModuleDeclaration { .. } => w.keep(c!("TSModuleDeclaration"), id),
				ExportAssignment { .. } => w.keep(c!("TSExportAssignment"), id),
				ImportEqualsDeclaration { .. } => w.keep(c!("TSImportEqualsDeclaration"), id),
				Decorator { .. } => w.keep(c!("Decorator"), id),
				_ => {}
			}
		}
		let base = &self.nodes[index as usize] as *const TsKind as *const u8;
		w.run(id, ts_recipes()[crate::estree::tag(base)], base);
		w.end();
	}

	fn extras<S: Sink>(&self, w: &mut Writer<Self, S>, id: NodeId) {
		let kind = w.kind(id);
		let extras = self.extras_of(id);
		let extension = matches!(kind, NodeKind::Extension(_));
		if w.output.erase {
			// proposals JavaScript itself has, which erasure keeps and lists: decorators and accessor fields
			if let Some(decorators) = extras.decorators {
				w.list(c!("decorators"), decorators);
			}
			if extras.accessor {
				w.bool(c!("accessor"), true);
				w.keep(c!("AccessorProperty"), id);
			}
			return;
		}
		match kind {
			NodeKind::ImportDeclaration { .. } | NodeKind::ImportSpecifier { .. } => {
				w.string(c!("importKind"), extras.import_kind.unwrap_or(Kind::Value).name());
			}
			NodeKind::ExportDeclaration { .. }
			| NodeKind::ExportNamedDeclaration { .. }
			| NodeKind::ExportDefaultDeclaration { .. }
			| NodeKind::ExportAllDeclaration { .. }
			| NodeKind::ExportSpecifier { .. } => {
				w.string(c!("exportKind"), extras.export_kind.unwrap_or(Kind::Value).name());
			}
			_ => {}
		}
		if extras == Extras::default() {
			return;
		}
		w.opt_key(c!("typeAnnotation"), extras.type_annotation);
		w.opt_key(c!("returnType"), extras.return_type);
		w.opt_key(c!("typeParameters"), extras.type_parameters);
		w.opt_key(c!("typeArguments"), extras.type_arguments);
		w.opt_key(c!("superTypeParameters"), extras.super_type_arguments);
		if let Some(implements) = extras.implements {
			w.list(c!("implements"), implements);
		}
		if let Some(decorators) = extras.decorators {
			w.list(c!("decorators"), decorators);
		}
		if let Some(accessibility) = extras.accessibility {
			w.string(c!("accessibility"), accessibility.name());
		}
		for (key, set) in [
			(c!("optional"), extras.optional),
			(c!("definite"), extras.definite),
			(c!("declare"), extras.declare),
			(c!("abstract"), extras.is_abstract),
			(c!("readonly"), extras.readonly),
			(c!("override"), extras.is_override),
			(c!("accessor"), extras.accessor),
			(c!("static"), extras.is_static && extension),
		] {
			if set {
				w.bool(key, true);
			}
		}
	}
}
