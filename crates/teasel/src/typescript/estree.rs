//! ESTree output for TypeScript nodes and the keys TypeScript adds to JavaScript nodes.

use super::ast::{Data, Extras, Kind, TsKind};
use crate::ast::{Ast, List, NodeId, NodeKind};
use crate::estree::{Emit, Writer};
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
#[rustfmt::skip]
pub(crate) const ADDS: &[(&str, &[Op])] = &[
	("ImportDeclaration", &[EnumOr(c!("importKind"), "import_kind", c!("value"))]),
	("ImportSpecifier", &[EnumOr(c!("importKind"), "import_kind", c!("value"))]),
	("ExportDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportNamedDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportDefaultDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportAllDeclaration", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
	("ExportSpecifier", &[EnumOr(c!("exportKind"), "export_kind", c!("value"))]),
];

/// The keys a node's extras add, after the kind's own.
#[rustfmt::skip]
pub(crate) const EXTRAS: &[Op] = &[
	OptKey(c!("typeAnnotation"), "type_annotation"), OptKey(c!("returnType"), "return_type"), OptKey(c!("typeParameters"), "type_parameters"),
	OptKey(c!("typeArguments"), "type_arguments"), OptKey(c!("superTypeParameters"), "super_type_arguments"),
	OptListKey(c!("implements"), "implements"), OptListKey(c!("decorators"), "decorators"), OptEnumKey(c!("accessibility"), "accessibility"),
	BoolIf(c!("optional"), "optional"), BoolIf(c!("definite"), "definite"), BoolIf(c!("declare"), "declare"), BoolIf(c!("abstract"), "is_abstract"),
	BoolIf(c!("readonly"), "readonly"), BoolIf(c!("override"), "is_override"), BoolIf(c!("accessor"), "accessor"), BoolIf(c!("static"), "is_static"),
];

/// The same when erasing: the proposals JavaScript itself has, decorators and accessor fields.
pub(crate) const EXTRAS_ERASED: &[Op] = &[
	OptListKey(c!("decorators"), "decorators"),
	BoolIf(c!("accessor"), "accessor"),
	KeepIf(c!("AccessorProperty"), "accessor"),
];

struct Recipes {
	kinds: &'static [&'static [Op<Slot>]],
	/// By the JavaScript kind's tag.
	adds: Vec<&'static [Op<Slot>]>,
	extras: &'static [Op<Slot>],
	erased: &'static [Op<Slot>],
}

fn recipes() -> &'static Recipes {
	static RESOLVED: std::sync::OnceLock<Recipes> = std::sync::OnceLock::new();
	RESOLVED.get_or_init(|| Recipes {
		kinds: crate::recipe::resolve(TS, super::ast::ts_layout::VARIANTS),
		adds: crate::ast::node_layout::VARIANTS
			.iter()
			.map(|variant| match ADDS.iter().find(|(name, _)| *name == variant.name) {
				Some((name, ops)) => crate::recipe::resolve_ops(ops, Extras::FIELDS, name),
				None => &[][..],
			})
			.collect(),
		extras: crate::recipe::resolve_ops(EXTRAS, Extras::FIELDS, "Extras"),
		erased: crate::recipe::resolve_ops(EXTRAS_ERASED, Extras::FIELDS, "Extras"),
	})
}

impl Data {
	/// Whether every statement of a list erases to nothing.
	fn all_erased(&self, ast: &Ast<Self>, list: List) -> bool {
		ast.list(list)
			.iter()
			.all(|item| item.is_none_or(|id| self.erased(ast, id)))
	}

	fn extras_of(&self, id: NodeId) -> Extras {
		self.extras(id).copied().unwrap_or_default()
	}
}

impl Emit for Data {
	fn erased(&self, ast: &Ast<Self>, id: NodeId) -> bool {
		use TsKind::*;
		let extras = self.extras_of(id);
		if extras.declare {
			return true;
		}
		match ast.node(id).kind {
			NodeKind::MethodDefinition { .. } | NodeKind::PropertyDefinition { .. } if extras.is_abstract => true,
			// an overload signature: a method without a body
			NodeKind::MethodDefinition { value, .. } => {
				matches!(self.ts_of(ast, value), Some(DeclareMethod { .. }))
			}
			NodeKind::Extension(index) => match self.kind(index) {
				InterfaceDeclaration { .. }
				| TypeAliasDeclaration { .. }
				| DeclareFunction { .. }
				| IndexSignature { .. }
				| NamespaceExportDeclaration { .. } => true,
				ImportEqualsDeclaration { import_kind, .. } => import_kind == Kind::Type,
				ModuleDeclaration { body: None, .. } => true,
				ModuleDeclaration { body: Some(block), .. } => match self.ts_of(ast, block) {
					Some(ModuleBlock { body }) => self.all_erased(ast, body),
					_ => false,
				},
				_ => false,
			},
			NodeKind::ImportDeclaration { specifiers, .. } => {
				extras.import_kind == Some(Kind::Type) || (specifiers.len > 0 && self.all_erased(ast, specifiers))
			}
			NodeKind::ImportSpecifier { .. } => extras.import_kind == Some(Kind::Type),
			NodeKind::ExportSpecifier { .. } | NodeKind::ExportAllDeclaration { .. } => {
				extras.export_kind == Some(Kind::Type)
			}
			// `export { type A }` keeps an `export {}`, as tsc keeps the file a module
			NodeKind::ExportDeclaration { declaration } => {
				extras.export_kind == Some(Kind::Type) || self.erased(ast, declaration)
			}
			NodeKind::ExportNamedDeclaration { .. } => extras.export_kind == Some(Kind::Type),
			NodeKind::ExportDefaultDeclaration { declaration } => {
				extras.export_kind == Some(Kind::Type) || self.erased(ast, declaration)
			}
			_ => false,
		}
	}

	fn node(&self, w: &mut Writer<Self>, id: NodeId, index: u32) {
		let base = &self.nodes[index as usize] as *const TsKind as *const u8;
		if w.run(id, recipes().kinds[crate::estree::tag(base)], base) {
			w.end();
		}
	}

	fn extras(&self, w: &mut Writer<Self>, id: NodeId) {
		let recipes = recipes();
		let extras = self.extras_of(id);
		let base = &extras as *const Extras as *const u8;
		if w.output.erase {
			w.run(id, recipes.erased, base);
			return;
		}
		let kind = w.kind(id);
		w.run(
			id,
			recipes.adds[crate::estree::tag(&kind as *const NodeKind as *const u8)],
			base,
		);
		w.run(id, recipes.extras, base);
	}
}
