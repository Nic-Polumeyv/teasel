//! TypeScript in the scope analysis: type positions bind nothing, and the declarations with a
//! runtime value join the value space.

use super::ast::{Data, TsKind};
use crate::ast::{Ast, NodeId, NodeKind};
use crate::scopes::{Bind, Binder, BindingKind, Mode, ScopeKind};

impl Data {
	fn index(ast: &Ast<Self>, id: NodeId) -> Option<u32> {
		match ast.node(id).kind {
			NodeKind::Extension(index) => Some(index),
			_ => None,
		}
	}

	/// The identifier at the root of a qualified name.
	fn root(&self, ast: &Ast<Self>, mut id: NodeId) -> Option<NodeId> {
		loop {
			match ast.node(id).kind {
				NodeKind::Identifier { .. } => return Some(id),
				NodeKind::Extension(index) => match self.kind(index) {
					TsKind::QualifiedName { left, .. } => id = left,
					_ => return None,
				},
				_ => return None,
			}
		}
	}
}

impl Bind for Data {
	fn bind(&self, b: &mut Binder<Self>, id: NodeId, mode: Mode) {
		use TsKind::*;
		let Some(index) = Self::index(b.ast(), id) else { return };
		match self.kind(index) {
			AsExpression { expression, .. }
			| SatisfiesExpression { expression, .. }
			| NonNullExpression { expression }
			| TypeAssertion { expression, .. }
			| TypeCastExpression { expression, .. }
			| InstantiationExpression { expression, .. }
			| ParameterProperty { parameter: expression } => b.visit(expression, mode),
			Decorator { expression } | ExportAssignment { expression } => b.visit(expression, Mode::Expression),
			EnumDeclaration { id: name, members, .. } => {
				b.declare(name, BindingKind::Enum);
				for &member in b.ast().list(members).iter().flatten() {
					if let Some(EnumMember {
						initializer: Some(initializer),
						..
					}) = Self::index(b.ast(), member).map(|i| self.kind(i))
					{
						b.visit(initializer, Mode::Expression);
					}
				}
			}
			ModuleDeclaration { id: name, body, global } => {
				if !global {
					b.declare(name, BindingKind::Namespace);
				}
				if let Some(body) = body {
					b.enter(ScopeKind::Namespace, Some(id), false);
					b.visit(body, Mode::Expression);
					b.exit();
				}
			}
			ModuleBlock { body } => b.statements(body),
			ImportEqualsDeclaration {
				id: name,
				module_reference,
				..
			} => {
				b.declare(name, BindingKind::Import);
				if let Some(root) = self.root(b.ast(), module_reference) {
					b.reference(root, false, false);
				}
			}
			DeclareFunction { id: Some(name), .. } if self.extras(id).is_some_and(|e| e.declare) => {
				b.declare(name, BindingKind::Function);
			}
			_ => {}
		}
	}

	fn bind_extras(&self, b: &mut Binder<Self>, id: NodeId) {
		if let Some(decorators) = self.extras(id).and_then(|e| e.decorators) {
			for &decorator in b.ast().list(decorators).iter().flatten() {
				b.visit(decorator, Mode::Expression);
			}
		}
	}

	fn wrapped(&self, ast: &Ast<Self>, id: NodeId) -> Option<NodeId> {
		use TsKind::*;
		match self.kind(Self::index(ast, id)?) {
			AsExpression { expression, .. }
			| SatisfiesExpression { expression, .. }
			| NonNullExpression { expression }
			| TypeAssertion { expression, .. }
			| TypeCastExpression { expression, .. }
			| InstantiationExpression { expression, .. } => Some(expression),
			_ => None,
		}
	}
}
