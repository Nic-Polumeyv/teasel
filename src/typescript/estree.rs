//! ESTree output for TypeScript nodes and the keys TypeScript adds to JavaScript nodes.

use super::ast::{Data, Extras, Kind, Modifier, TsKind};
use crate::ast::{NodeId, NodeKind};
use crate::estree::{Emit, Writer};

impl Emit for Data {
	fn node(&self, w: &mut Writer<Self>, id: NodeId, index: u32) {
		use TsKind::*;
		match self.kind(index) {
			TypeAnnotation { type_annotation } => {
				w.begin("TSTypeAnnotation", id);
				w.field("typeAnnotation", type_annotation);
			}
			Keyword(keyword) => w.begin(keyword.estree_type(), id),
			ThisType => w.begin("TSThisType", id),
			TypePredicate {
				parameter_name,
				type_annotation,
				asserts,
			} => {
				w.begin("TSTypePredicate", id);
				w.field("parameterName", parameter_name);
				w.opt("typeAnnotation", type_annotation);
				w.bool("asserts", asserts);
			}
			TypeReference {
				type_name,
				type_arguments,
			} => {
				w.begin("TSTypeReference", id);
				w.field("typeName", type_name);
				w.opt_key("typeArguments", type_arguments);
			}
			QualifiedName { left, right } => {
				w.begin("TSQualifiedName", id);
				w.field("left", left);
				w.field("right", right);
			}
			TypeParameterInstantiation { params } => {
				w.begin("TSTypeParameterInstantiation", id);
				w.list("params", params);
			}
			TypeParameterDeclaration { params } => {
				w.begin("TSTypeParameterDeclaration", id);
				w.list("params", params);
			}
			TypeParameter {
				name,
				constraint,
				default,
				is_in,
				is_out,
				is_const,
			} => {
				w.begin("TSTypeParameter", id);
				if is_in {
					w.bool("in", true);
				}
				if is_out {
					w.bool("out", true);
				}
				if is_const {
					w.bool("const", true);
				}
				w.interned("name", name);
				w.opt_key("constraint", constraint);
				w.opt_key("default", default);
			}
			FunctionType {
				type_parameters,
				parameters,
				type_annotation,
			} => {
				w.begin("TSFunctionType", id);
				w.opt_key("typeParameters", type_parameters);
				w.list("parameters", parameters);
				w.field("typeAnnotation", type_annotation);
			}
			ConstructorType {
				type_parameters,
				parameters,
				type_annotation,
				is_abstract,
			} => {
				w.begin("TSConstructorType", id);
				w.bool("abstract", is_abstract);
				w.opt_key("typeParameters", type_parameters);
				w.list("parameters", parameters);
				w.field("typeAnnotation", type_annotation);
			}
			UnionType { types } => {
				w.begin("TSUnionType", id);
				w.list("types", types);
			}
			IntersectionType { types } => {
				w.begin("TSIntersectionType", id);
				w.list("types", types);
			}
			TypeOperator {
				operator,
				type_annotation,
			} => {
				w.begin("TSTypeOperator", id);
				w.interned("operator", operator);
				w.field("typeAnnotation", type_annotation);
			}
			InferType { type_parameter } => {
				w.begin("TSInferType", id);
				w.field("typeParameter", type_parameter);
			}
			LiteralType { literal } => {
				w.begin("TSLiteralType", id);
				w.field("literal", literal);
			}
			ImportType {
				argument,
				qualifier,
				type_arguments,
			} => {
				w.begin("TSImportType", id);
				w.field("argument", argument);
				w.opt_key("qualifier", qualifier);
				w.opt_key("typeArguments", type_arguments);
			}
			TypeQuery {
				expr_name,
				type_arguments,
			} => {
				w.begin("TSTypeQuery", id);
				w.field("exprName", expr_name);
				w.opt_key("typeArguments", type_arguments);
			}
			MappedType {
				readonly,
				type_parameter,
				name_type,
				optional,
				type_annotation,
			} => {
				w.begin("TSMappedType", id);
				modifier(w, "readonly", readonly);
				w.field("typeParameter", type_parameter);
				w.opt("nameType", name_type);
				modifier(w, "optional", optional);
				w.opt_key("typeAnnotation", type_annotation);
			}
			TypeLiteral { members } => {
				w.begin("TSTypeLiteral", id);
				w.list("members", members);
			}
			NamedTupleMember {
				label,
				optional,
				element_type,
			} => {
				w.begin("TSNamedTupleMember", id);
				w.bool("optional", optional);
				w.field("label", label);
				w.field("elementType", element_type);
			}
			OptionalType { type_annotation } => {
				w.begin("TSOptionalType", id);
				w.field("typeAnnotation", type_annotation);
			}
			RestType { type_annotation } => {
				w.begin("TSRestType", id);
				w.field("typeAnnotation", type_annotation);
			}
			TupleType { element_types } => {
				w.begin("TSTupleType", id);
				w.list("elementTypes", element_types);
			}
			ParenthesizedType { type_annotation } => {
				w.begin("TSParenthesizedType", id);
				w.field("typeAnnotation", type_annotation);
			}
			ArrayType { element_type } => {
				w.begin("TSArrayType", id);
				w.field("elementType", element_type);
			}
			IndexedAccessType {
				object_type,
				index_type,
			} => {
				w.begin("TSIndexedAccessType", id);
				w.field("objectType", object_type);
				w.field("indexType", index_type);
			}
			ConditionalType {
				check_type,
				extends_type,
				true_type,
				false_type,
			} => {
				w.begin("TSConditionalType", id);
				w.field("checkType", check_type);
				w.field("extendsType", extends_type);
				w.field("trueType", true_type);
				w.field("falseType", false_type);
			}
			IndexSignature {
				parameters,
				type_annotation,
			} => {
				w.begin("TSIndexSignature", id);
				w.list("parameters", parameters);
				w.opt_key("typeAnnotation", type_annotation);
			}
			CallSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			} => {
				w.begin("TSCallSignatureDeclaration", id);
				w.opt_key("typeParameters", type_parameters);
				w.list("parameters", parameters);
				w.opt_key("typeAnnotation", type_annotation);
			}
			ConstructSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			} => {
				w.begin("TSConstructSignatureDeclaration", id);
				w.opt_key("typeParameters", type_parameters);
				w.list("parameters", parameters);
				w.opt_key("typeAnnotation", type_annotation);
			}
			MethodSignature {
				key,
				computed,
				optional,
				kind,
				type_parameters,
				parameters,
				type_annotation,
			} => {
				w.begin("TSMethodSignature", id);
				w.field("key", key);
				w.bool("computed", computed);
				if optional {
					w.bool("optional", true);
				}
				w.string("kind", kind.as_str());
				w.opt_key("typeParameters", type_parameters);
				w.list("parameters", parameters);
				w.opt_key("typeAnnotation", type_annotation);
			}
			PropertySignature {
				key,
				computed,
				optional,
				readonly,
				kind,
				type_annotation,
			} => {
				w.begin("TSPropertySignature", id);
				w.field("key", key);
				if let Some(computed) = computed {
					w.bool("computed", computed);
				}
				if optional {
					w.bool("optional", true);
				}
				if readonly {
					w.bool("readonly", true);
				}
				if let Some(kind) = kind {
					w.string("kind", kind.as_str());
				}
				w.opt_key("typeAnnotation", type_annotation);
			}
			InterfaceDeclaration {
				id: name,
				type_parameters,
				extends,
				body,
			} => {
				w.begin("TSInterfaceDeclaration", id);
				w.field("id", name);
				w.opt_key("typeParameters", type_parameters);
				if let Some(extends) = extends {
					w.list("extends", extends);
				}
				w.field("body", body);
			}
			InterfaceBody { body } => {
				w.begin("TSInterfaceBody", id);
				w.list("body", body);
			}
			ExpressionWithTypeArguments {
				expression,
				type_arguments,
			} => {
				w.begin("TSExpressionWithTypeArguments", id);
				w.field("expression", expression);
				w.opt_key("typeParameters", type_arguments);
			}
			EnumDeclaration {
				id: name,
				members,
				is_const,
			} => {
				w.begin("TSEnumDeclaration", id);
				if is_const {
					w.bool("const", true);
				}
				w.field("id", name);
				w.list("members", members);
			}
			EnumMember { id: name, initializer } => {
				w.begin("TSEnumMember", id);
				w.field("id", name);
				w.opt_key("initializer", initializer);
			}
			ModuleDeclaration { id: name, body, global } => {
				w.begin("TSModuleDeclaration", id);
				if global {
					w.bool("global", true);
				}
				w.field("id", name);
				w.opt_key("body", body);
			}
			ModuleBlock { body } => {
				w.begin("TSModuleBlock", id);
				w.list("body", body);
			}
			TypeAliasDeclaration {
				id: name,
				type_parameters,
				type_annotation,
			} => {
				w.begin("TSTypeAliasDeclaration", id);
				w.field("id", name);
				w.opt_key("typeParameters", type_parameters);
				w.field("typeAnnotation", type_annotation);
			}
			ImportEqualsDeclaration {
				id: name,
				module_reference,
				is_export,
				import_kind,
			} => {
				w.begin("TSImportEqualsDeclaration", id);
				w.string("importKind", import_kind.as_str());
				w.bool("isExport", is_export);
				w.field("id", name);
				w.field("moduleReference", module_reference);
			}
			ExternalModuleReference { expression } => {
				w.begin("TSExternalModuleReference", id);
				w.field("expression", expression);
			}
			ExportAssignment { expression } => {
				w.begin("TSExportAssignment", id);
				w.field("expression", expression);
			}
			NamespaceExportDeclaration { id: name } => {
				w.begin("TSNamespaceExportDeclaration", id);
				w.field("id", name);
			}
			DeclareFunction {
				id: name,
				params,
				is_async,
				generator,
			} => {
				w.begin("TSDeclareFunction", id);
				w.opt("id", name);
				w.bool("generator", generator);
				w.bool("async", is_async);
				w.bool("expression", false);
				w.list("params", params);
			}
			DeclareMethod {
				params,
				is_async,
				generator,
			} => {
				w.begin("TSDeclareMethod", id);
				w.opt("id", None);
				w.bool("generator", generator);
				w.bool("async", is_async);
				w.bool("expression", false);
				w.list("params", params);
			}
			AsExpression {
				expression,
				type_annotation,
			} => {
				w.begin("TSAsExpression", id);
				w.field("expression", expression);
				w.field("typeAnnotation", type_annotation);
			}
			SatisfiesExpression {
				expression,
				type_annotation,
			} => {
				w.begin("TSSatisfiesExpression", id);
				w.field("expression", expression);
				w.field("typeAnnotation", type_annotation);
			}
			NonNullExpression { expression } => {
				w.begin("TSNonNullExpression", id);
				w.field("expression", expression);
			}
			TypeAssertion {
				type_annotation,
				expression,
			} => {
				w.begin("TSTypeAssertion", id);
				w.field("typeAnnotation", type_annotation);
				w.field("expression", expression);
			}
			TypeCastExpression {
				expression,
				type_annotation,
			} => {
				w.begin("TSTypeCastExpression", id);
				w.field("expression", expression);
				w.field("typeAnnotation", type_annotation);
			}
			InstantiationExpression {
				expression,
				type_arguments,
			} => {
				w.begin("TSInstantiationExpression", id);
				w.field("expression", expression);
				w.field("typeArguments", type_arguments);
			}
			ParameterProperty { parameter } => {
				w.begin("TSParameterProperty", id);
				w.field("parameter", parameter);
			}
			Decorator { expression } => {
				w.begin("Decorator", id);
				w.field("expression", expression);
			}
		}
	}

	fn extras(&self, w: &mut Writer<Self>, id: NodeId) {
		let kind = w.kind(id);
		let extras = self.extras(id).copied().unwrap_or_default();
		let extension = matches!(kind, NodeKind::Extension(_));
		match kind {
			NodeKind::ImportDeclaration { .. } | NodeKind::ImportSpecifier { .. } => {
				w.string("importKind", extras.import_kind.unwrap_or(Kind::Value).as_str());
			}
			NodeKind::ExportNamedDeclaration { .. }
			| NodeKind::ExportDefaultDeclaration { .. }
			| NodeKind::ExportAllDeclaration { .. }
			| NodeKind::ExportSpecifier { .. } => {
				w.string("exportKind", extras.export_kind.unwrap_or(Kind::Value).as_str());
			}
			_ => {}
		}
		if extras == Extras::default() {
			return;
		}
		w.opt_key("typeAnnotation", extras.type_annotation);
		w.opt_key("returnType", extras.return_type);
		w.opt_key("typeParameters", extras.type_parameters);
		w.opt_key("typeArguments", extras.type_arguments);
		w.opt_key("superTypeParameters", extras.super_type_arguments);
		if let Some(implements) = extras.implements {
			w.list("implements", implements);
		}
		if let Some(decorators) = extras.decorators {
			w.list("decorators", decorators);
		}
		if let Some(accessibility) = extras.accessibility {
			w.string("accessibility", accessibility.as_str());
		}
		for (key, set) in [
			("optional", extras.optional),
			("definite", extras.definite),
			("declare", extras.declare),
			("abstract", extras.is_abstract),
			("readonly", extras.readonly),
			("override", extras.is_override),
			("accessor", extras.accessor),
			("static", extras.is_static && extension),
		] {
			if set {
				w.bool(key, true);
			}
		}
	}
}

fn modifier(w: &mut Writer<Data>, key: &str, value: Option<Modifier>) {
	match value {
		Some(Modifier::Plus) => w.string(key, "+"),
		Some(Modifier::Minus) => w.string(key, "-"),
		Some(Modifier::True) => w.bool(key, true),
		None => {}
	}
}
