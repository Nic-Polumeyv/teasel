//! The children of TypeScript nodes, and the ones TypeScript adds to JavaScript nodes.

use super::ast::{Data, TsKind};
use crate::ast::{Ast, List, NodeId, NodeKind, Walk};

impl Walk for Data {
	fn children(&self, ast: &Ast<Self>, id: NodeId, out: &mut Vec<NodeId>) {
		let list = |list: Option<List>, out: &mut Vec<NodeId>| {
			if let Some(list) = list {
				out.extend(ast.list(list).iter().flatten());
			}
		};
		let extras = self.extras(id).copied().unwrap_or_default();
		match ast.node(id).kind {
			NodeKind::Extension(index) => {
				use TsKind::*;
				match self.kind(index) {
					TypeAnnotation { type_annotation }
					| TypeOperator { type_annotation, .. }
					| OptionalType { type_annotation }
					| RestType { type_annotation }
					| ParenthesizedType { type_annotation } => out.push(type_annotation),
					Keyword(_) | ThisType => {}
					TypePredicate {
						parameter_name,
						type_annotation,
						..
					} => {
						out.push(parameter_name);
						out.extend(type_annotation);
					}
					TypeReference {
						type_name,
						type_arguments,
					} => {
						out.push(type_name);
						out.extend(type_arguments);
					}
					QualifiedName { left, right } => out.extend([left, right]),
					TypeParameterInstantiation { params } | TypeParameterDeclaration { params } => {
						list(Some(params), out)
					}
					TypeParameter {
						constraint, default, ..
					} => {
						out.extend(constraint);
						out.extend(default);
					}
					FunctionType {
						type_parameters,
						parameters,
						type_annotation,
					}
					| ConstructorType {
						type_parameters,
						parameters,
						type_annotation,
						..
					} => {
						out.extend(type_parameters);
						list(Some(parameters), out);
						out.push(type_annotation);
					}
					UnionType { types } | IntersectionType { types } => list(Some(types), out),
					InferType { type_parameter } => out.push(type_parameter),
					LiteralType { literal } => out.push(literal),
					ImportType {
						argument,
						qualifier,
						type_arguments,
					} => {
						out.push(argument);
						out.extend(qualifier);
						out.extend(type_arguments);
					}
					TypeQuery {
						expr_name,
						type_arguments,
					} => {
						out.push(expr_name);
						out.extend(type_arguments);
					}
					MappedType {
						type_parameter,
						name_type,
						type_annotation,
						..
					} => {
						out.push(type_parameter);
						out.extend(name_type);
						out.extend(type_annotation);
					}
					TypeLiteral { members } => list(Some(members), out),
					NamedTupleMember {
						label, element_type, ..
					} => out.extend([label, element_type]),
					TupleType { element_types } => list(Some(element_types), out),
					ArrayType { element_type } => out.push(element_type),
					IndexedAccessType {
						object_type,
						index_type,
					} => out.extend([object_type, index_type]),
					ConditionalType {
						check_type,
						extends_type,
						true_type,
						false_type,
					} => out.extend([check_type, extends_type, true_type, false_type]),
					IndexSignature {
						parameters,
						type_annotation,
					} => {
						list(Some(parameters), out);
						out.extend(type_annotation);
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
						out.extend(type_parameters);
						list(Some(parameters), out);
						out.extend(type_annotation);
					}
					MethodSignature {
						key,
						type_parameters,
						parameters,
						type_annotation,
						..
					} => {
						out.push(key);
						out.extend(type_parameters);
						list(Some(parameters), out);
						out.extend(type_annotation);
					}
					PropertySignature {
						key, type_annotation, ..
					} => {
						out.push(key);
						out.extend(type_annotation);
					}
					InterfaceDeclaration {
						id,
						type_parameters,
						extends,
						body,
					} => {
						out.push(id);
						out.extend(type_parameters);
						list(extends, out);
						out.push(body);
					}
					InterfaceBody { body } | ModuleBlock { body } => list(Some(body), out),
					ExpressionWithTypeArguments {
						expression,
						type_arguments,
					} => {
						out.push(expression);
						out.extend(type_arguments);
					}
					EnumDeclaration { id, members, .. } => {
						out.push(id);
						list(Some(members), out);
					}
					EnumMember { id, initializer } => {
						out.push(id);
						out.extend(initializer);
					}
					ModuleDeclaration { id, body, .. } => {
						out.push(id);
						out.extend(body);
					}
					TypeAliasDeclaration {
						id,
						type_parameters,
						type_annotation,
					} => {
						out.push(id);
						out.extend(type_parameters);
						out.push(type_annotation);
					}
					ImportEqualsDeclaration {
						id, module_reference, ..
					} => out.extend([id, module_reference]),
					ExternalModuleReference { expression }
					| ExportAssignment { expression }
					| NonNullExpression { expression }
					| Decorator { expression } => out.push(expression),
					NamespaceExportDeclaration { id } => out.push(id),
					DeclareFunction { id, params, .. } => {
						out.extend(id);
						out.extend(extras.type_parameters);
						list(Some(params), out);
						out.extend(extras.return_type);
					}
					DeclareMethod { params, .. } => {
						list(Some(params), out);
						out.extend(extras.return_type);
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
					} => out.extend([expression, type_annotation]),
					TypeAssertion {
						type_annotation,
						expression,
					} => out.extend([type_annotation, expression]),
					InstantiationExpression {
						expression,
						type_arguments,
					} => out.extend([expression, type_arguments]),
					ParameterProperty { parameter } => out.push(parameter),
				}
			}
			_ => {
				ast.plain_children(id, out);
				out.extend(extras.type_annotation);
				out.extend(extras.return_type);
				out.extend(extras.type_parameters);
				out.extend(extras.type_arguments);
				out.extend(extras.super_type_arguments);
				list(extras.implements, out);
				list(extras.decorators, out);
			}
		}
	}
}
