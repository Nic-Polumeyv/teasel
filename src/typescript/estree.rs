//! ESTree output for TypeScript nodes and the keys TypeScript adds to JavaScript nodes.

use super::ast::{Data, Extras, Kind, Modifier, TsKind};
use crate::ast::{List, NodeId, NodeKind};
use crate::estree::{Emit, Sink, Writer};
use crate::names::{Name, c};

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
		let kind = self.kind(index);
		match kind {
			TypeAnnotation { type_annotation }
			| OptionalType { type_annotation }
			| RestType { type_annotation }
			| ParenthesizedType { type_annotation } => {
				w.begin(
					match kind {
						TypeAnnotation { .. } => c!("TSTypeAnnotation"),
						OptionalType { .. } => c!("TSOptionalType"),
						RestType { .. } => c!("TSRestType"),
						ParenthesizedType { .. } => c!("TSParenthesizedType"),
						_ => unreachable!(),
					},
					id,
				);
				w.field(c!("typeAnnotation"), type_annotation);
			}
			Keyword(keyword) => w.begin(keyword.estree_type(), id),
			ThisType => w.begin(c!("TSThisType"), id),
			TypePredicate {
				parameter_name,
				type_annotation,
				asserts,
			} => {
				w.begin(c!("TSTypePredicate"), id);
				w.field(c!("parameterName"), parameter_name);
				w.opt(c!("typeAnnotation"), type_annotation);
				w.bool(c!("asserts"), asserts);
			}
			TypeReference {
				type_name,
				type_arguments,
			} => {
				w.begin(c!("TSTypeReference"), id);
				w.field(c!("typeName"), type_name);
				w.opt_key(c!("typeArguments"), type_arguments);
			}
			QualifiedName { left, right } => {
				w.begin(c!("TSQualifiedName"), id);
				w.field(c!("left"), left);
				w.field(c!("right"), right);
			}
			TypeParameterInstantiation { params } | TypeParameterDeclaration { params } => {
				let instantiation = matches!(kind, TypeParameterInstantiation { .. });
				w.begin(
					if instantiation {
						c!("TSTypeParameterInstantiation")
					} else {
						c!("TSTypeParameterDeclaration")
					},
					id,
				);
				w.list(c!("params"), params);
			}
			TypeParameter {
				name,
				constraint,
				default,
				is_in,
				is_out,
				is_const,
			} => {
				w.begin(c!("TSTypeParameter"), id);
				if is_in {
					w.bool(c!("in"), true);
				}
				if is_out {
					w.bool(c!("out"), true);
				}
				if is_const {
					w.bool(c!("const"), true);
				}
				w.interned(c!("name"), name);
				w.opt_key(c!("constraint"), constraint);
				w.opt_key(c!("default"), default);
			}
			FunctionType {
				type_parameters,
				parameters,
				type_annotation,
			} => {
				w.begin(c!("TSFunctionType"), id);
				w.opt_key(c!("typeParameters"), type_parameters);
				w.list(c!("parameters"), parameters);
				w.field(c!("typeAnnotation"), type_annotation);
			}
			ConstructorType {
				type_parameters,
				parameters,
				type_annotation,
				is_abstract,
			} => {
				w.begin(c!("TSConstructorType"), id);
				w.bool(c!("abstract"), is_abstract);
				w.opt_key(c!("typeParameters"), type_parameters);
				w.list(c!("parameters"), parameters);
				w.field(c!("typeAnnotation"), type_annotation);
			}
			UnionType { types } | IntersectionType { types } => {
				let union = matches!(kind, UnionType { .. });
				w.begin(
					if union {
						c!("TSUnionType")
					} else {
						c!("TSIntersectionType")
					},
					id,
				);
				w.list(c!("types"), types);
			}
			TypeOperator {
				operator,
				type_annotation,
			} => {
				w.begin(c!("TSTypeOperator"), id);
				w.interned(c!("operator"), operator);
				w.field(c!("typeAnnotation"), type_annotation);
			}
			InferType { type_parameter } => {
				w.begin(c!("TSInferType"), id);
				w.field(c!("typeParameter"), type_parameter);
			}
			LiteralType { literal } => {
				w.begin(c!("TSLiteralType"), id);
				w.field(c!("literal"), literal);
			}
			ImportType {
				argument,
				qualifier,
				type_arguments,
			} => {
				w.begin(c!("TSImportType"), id);
				w.field(c!("argument"), argument);
				w.opt_key(c!("qualifier"), qualifier);
				w.opt_key(c!("typeArguments"), type_arguments);
			}
			TypeQuery {
				expr_name,
				type_arguments,
			} => {
				w.begin(c!("TSTypeQuery"), id);
				w.field(c!("exprName"), expr_name);
				w.opt_key(c!("typeArguments"), type_arguments);
			}
			MappedType {
				readonly,
				type_parameter,
				name_type,
				optional,
				type_annotation,
			} => {
				w.begin(c!("TSMappedType"), id);
				modifier(w, c!("readonly"), readonly);
				w.field(c!("typeParameter"), type_parameter);
				w.opt(c!("nameType"), name_type);
				modifier(w, c!("optional"), optional);
				w.opt_key(c!("typeAnnotation"), type_annotation);
			}
			TypeLiteral { members } => {
				w.begin(c!("TSTypeLiteral"), id);
				w.list(c!("members"), members);
			}
			NamedTupleMember {
				label,
				optional,
				element_type,
			} => {
				w.begin(c!("TSNamedTupleMember"), id);
				w.bool(c!("optional"), optional);
				w.field(c!("label"), label);
				w.field(c!("elementType"), element_type);
			}

			TupleType { element_types } => {
				w.begin(c!("TSTupleType"), id);
				w.list(c!("elementTypes"), element_types);
			}

			ArrayType { element_type } => {
				w.begin(c!("TSArrayType"), id);
				w.field(c!("elementType"), element_type);
			}
			IndexedAccessType {
				object_type,
				index_type,
			} => {
				w.begin(c!("TSIndexedAccessType"), id);
				w.field(c!("objectType"), object_type);
				w.field(c!("indexType"), index_type);
			}
			ConditionalType {
				check_type,
				extends_type,
				true_type,
				false_type,
			} => {
				w.begin(c!("TSConditionalType"), id);
				w.field(c!("checkType"), check_type);
				w.field(c!("extendsType"), extends_type);
				w.field(c!("trueType"), true_type);
				w.field(c!("falseType"), false_type);
			}
			IndexSignature {
				parameters,
				type_annotation,
			} => {
				w.begin(c!("TSIndexSignature"), id);
				w.list(c!("parameters"), parameters);
				w.opt_key(c!("typeAnnotation"), type_annotation);
			}
			CallSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			}
			| ConstructSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			} => {
				let call = matches!(kind, CallSignatureDeclaration { .. });
				w.begin(
					if call {
						c!("TSCallSignatureDeclaration")
					} else {
						c!("TSConstructSignatureDeclaration")
					},
					id,
				);
				w.opt_key(c!("typeParameters"), type_parameters);
				w.list(c!("parameters"), parameters);
				w.opt_key(c!("typeAnnotation"), type_annotation);
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
				w.begin(c!("TSMethodSignature"), id);
				w.field(c!("key"), key);
				w.bool(c!("computed"), computed);
				if optional {
					w.bool(c!("optional"), true);
				}
				w.string(c!("kind"), kind.name());
				w.opt_key(c!("typeParameters"), type_parameters);
				w.list(c!("parameters"), parameters);
				w.opt_key(c!("typeAnnotation"), type_annotation);
			}
			PropertySignature {
				key,
				computed,
				optional,
				readonly,
				kind,
				type_annotation,
			} => {
				w.begin(c!("TSPropertySignature"), id);
				w.field(c!("key"), key);
				if let Some(computed) = computed {
					w.bool(c!("computed"), computed);
				}
				if optional {
					w.bool(c!("optional"), true);
				}
				if readonly {
					w.bool(c!("readonly"), true);
				}
				if let Some(kind) = kind {
					w.string(c!("kind"), kind.name());
				}
				w.opt_key(c!("typeAnnotation"), type_annotation);
			}
			InterfaceDeclaration {
				id: name,
				type_parameters,
				extends,
				body,
			} => {
				w.begin(c!("TSInterfaceDeclaration"), id);
				w.field(c!("id"), name);
				w.opt_key(c!("typeParameters"), type_parameters);
				if let Some(extends) = extends {
					w.list(c!("extends"), extends);
				}
				w.field(c!("body"), body);
			}
			InterfaceBody { body } => {
				w.begin(c!("TSInterfaceBody"), id);
				w.list(c!("body"), body);
			}
			ExpressionWithTypeArguments {
				expression,
				type_arguments,
			} => {
				w.begin(c!("TSExpressionWithTypeArguments"), id);
				w.field(c!("expression"), expression);
				w.opt_key(c!("typeParameters"), type_arguments);
			}
			EnumDeclaration {
				id: name,
				members,
				is_const,
			} => {
				w.begin(c!("TSEnumDeclaration"), id);
				if is_const {
					w.bool(c!("const"), true);
				}
				w.field(c!("id"), name);
				w.list(c!("members"), members);
			}
			EnumMember { id: name, initializer } => {
				w.begin(c!("TSEnumMember"), id);
				w.field(c!("id"), name);
				w.opt_key(c!("initializer"), initializer);
			}
			ModuleDeclaration { id: name, body, global } => {
				w.begin(c!("TSModuleDeclaration"), id);
				if global {
					w.bool(c!("global"), true);
				}
				w.field(c!("id"), name);
				w.opt_key(c!("body"), body);
			}
			ModuleBlock { body } => {
				w.begin(c!("TSModuleBlock"), id);
				w.list(c!("body"), body);
			}
			TypeAliasDeclaration {
				id: name,
				type_parameters,
				type_annotation,
			} => {
				w.begin(c!("TSTypeAliasDeclaration"), id);
				w.field(c!("id"), name);
				w.opt_key(c!("typeParameters"), type_parameters);
				w.field(c!("typeAnnotation"), type_annotation);
			}
			ImportEqualsDeclaration {
				id: name,
				module_reference,
				is_export,
				import_kind,
			} => {
				w.begin(c!("TSImportEqualsDeclaration"), id);
				w.string(c!("importKind"), import_kind.name());
				w.bool(c!("isExport"), is_export);
				w.field(c!("id"), name);
				w.field(c!("moduleReference"), module_reference);
			}
			ExternalModuleReference { expression } => {
				w.begin(c!("TSExternalModuleReference"), id);
				w.field(c!("expression"), expression);
			}
			ExportAssignment { expression } => {
				w.begin(c!("TSExportAssignment"), id);
				w.field(c!("expression"), expression);
			}
			NamespaceExportDeclaration { id: name } => {
				w.begin(c!("TSNamespaceExportDeclaration"), id);
				w.field(c!("id"), name);
			}
			DeclareFunction {
				params,
				is_async,
				generator,
				..
			}
			| DeclareMethod {
				params,
				is_async,
				generator,
			} => {
				let name = match kind {
					DeclareFunction { id: name, .. } => {
						w.begin(c!("TSDeclareFunction"), id);
						name
					}
					_ => {
						w.begin(c!("TSDeclareMethod"), id);
						None
					}
				};
				w.opt(c!("id"), name);
				w.bool(c!("generator"), generator);
				w.bool(c!("async"), is_async);
				w.bool(c!("expression"), false);
				w.list(c!("params"), params);
			}
			AsExpression {
				expression,
				type_annotation,
			}
			| SatisfiesExpression {
				expression,
				type_annotation,
			}
			| TypeCastExpression {
				expression,
				type_annotation,
			} => {
				w.begin(
					match kind {
						AsExpression { .. } => c!("TSAsExpression"),
						SatisfiesExpression { .. } => c!("TSSatisfiesExpression"),
						TypeCastExpression { .. } => c!("TSTypeCastExpression"),
						_ => unreachable!(),
					},
					id,
				);
				w.field(c!("expression"), expression);
				w.field(c!("typeAnnotation"), type_annotation);
			}
			NonNullExpression { expression } => {
				w.begin(c!("TSNonNullExpression"), id);
				w.field(c!("expression"), expression);
			}
			TypeAssertion {
				type_annotation,
				expression,
			} => {
				w.begin(c!("TSTypeAssertion"), id);
				w.field(c!("typeAnnotation"), type_annotation);
				w.field(c!("expression"), expression);
			}

			InstantiationExpression {
				expression,
				type_arguments,
			} => {
				w.begin(c!("TSInstantiationExpression"), id);
				w.field(c!("expression"), expression);
				w.field(c!("typeArguments"), type_arguments);
			}
			ParameterProperty { parameter } => {
				w.begin(c!("TSParameterProperty"), id);
				w.field(c!("parameter"), parameter);
			}
			Decorator { expression } => {
				w.begin(c!("Decorator"), id);
				w.field(c!("expression"), expression);
			}
		}
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

fn modifier<S: Sink>(w: &mut Writer<Data, S>, key: Name, value: Option<Modifier>) {
	match value {
		Some(Modifier::Plus) => w.string(key, c!("+")),
		Some(Modifier::Minus) => w.string(key, c!("-")),
		Some(Modifier::True) => w.bool(key, true),
		None => {}
	}
}
