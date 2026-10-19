//! Declarations that only exist in TypeScript, and the ambient and type-only forms of the
//! JavaScript ones.

use super::ast::{Kind, TsKind};
use super::types::{ListKind, TypeParameterModifiers};
use super::{ClassFrame, TypeScript};
use crate::ast::{List, NodeId, NodeKind, VariableKind};
use crate::error::Code;
use crate::lexer::token::{Keyword, TokenKind};
use crate::parser::scope::{Binding, SCOPE_FUNCTION, SCOPE_TS_MODULE};
use crate::parser::statement::FUNC_STATEMENT;
use crate::parser::statement::{ClassKind, Exports, StatementPlace};
use crate::parser::{Context, ForInit, Parser, Result};

#[derive(Clone, Copy, Default)]
pub(super) struct Opened {
	declare: Option<u32>,
	is_abstract: Option<u32>,
	head: Option<Head>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Head {
	Abstract,
	Declare,
	Enum,
	Global,
	Interface,
	Module,
	Namespace,
	Type,
}

impl Parser<'_, TypeScript> {
	fn ambient<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
		let old = self.ext.ambient;
		self.ext.ambient = true;
		let result = f(self);
		self.ext.ambient = old;
		result
	}

	// Enums

	pub(super) fn parse_enum_declaration(&mut self, start: u32, is_const: bool, declare: bool) -> Result<NodeId> {
		self.expect_contextual("enum")?;
		let id = self.parse_ident(false)?;
		self.check_lval_simple(id, Binding::None, &mut None)?;
		self.declare_export_only(id);
		self.expect(TokenKind::BraceL)?;
		let members = self.parse_delimited_list(ListKind::EnumMembers, |p| p.parse_enum_member())?;
		self.expect(TokenKind::BraceR)?;
		let members = self.list_of(&members);
		let node = self.ts(TsKind::EnumDeclaration { id, members, is_const }, start);
		self.extras_mut(node).declare = declare;
		Ok(node)
	}

	fn parse_enum_member(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let id = if matches!(self.tok.kind, TokenKind::String(_)) {
			self.parse_expr_atom(&mut None, ForInit::No, false)?
		} else {
			self.parse_ident(true)?
		};
		let initializer = if self.eat(TokenKind::Eq)? {
			self.enter_scope(SCOPE_FUNCTION);
			let initializer = self.parse_maybe_assign(ForInit::No, &mut None);
			self.exit_scope();
			Some(initializer?)
		} else {
			None
		};
		Ok(self.ts(TsKind::EnumMember { id, initializer }, start))
	}

	// Namespaces

	/// A module block in its own module scope.
	fn parse_module_body(&mut self, namespace: bool) -> Result<NodeId> {
		self.enter_scope(SCOPE_TS_MODULE);
		let body = self.parse_module_block(namespace);
		self.exit_scope();
		body
	}

	/// A global block: its declarations are the module's to export.
	fn parse_global_body(&mut self) -> Result<NodeId> {
		self.enter_scope(SCOPE_TS_MODULE);
		self.ext.global_depth += 1;
		let body = self.parse_module_block(false);
		self.ext.global_depth -= 1;
		let declared: Vec<_> = self.current_scope().declared().collect();
		self.ext.globals.extend(declared);
		self.exit_scope();
		body
	}

	/// A statement an ambient body may hold: what declares, not what runs.
	fn is_declaration(&self, statement: NodeId) -> bool {
		matches!(
			self.kind(statement),
			NodeKind::VariableDeclaration { .. }
				| NodeKind::FunctionDeclaration { .. }
				| NodeKind::ClassDeclaration { .. }
				| NodeKind::ImportDeclaration { .. }
				| NodeKind::ExportDeclaration { .. }
				| NodeKind::ExportNamedDeclaration { .. }
				| NodeKind::ExportDefaultDeclaration { .. }
				| NodeKind::ExportAllDeclaration { .. }
				| NodeKind::Extension(_)
		)
	}

	fn parse_module_block(&mut self, namespace: bool) -> Result<NodeId> {
		let start = self.tok.start;
		self.enter_scope(0);
		self.ext.module_blocks += 1;
		let in_namespace = std::mem::replace(&mut self.ext.in_namespace, namespace);
		self.expect(TokenKind::BraceL)?;
		let mut body = Vec::new();
		let mut exports = Exports::default();
		while !self.is(TokenKind::BraceR) {
			let at = self.tok.start;
			if let Some(statement) = self.statement_recovered(|p| {
				p.parse_statement(Context::None, StatementPlace::TopLevel, Some(&mut exports))
			})? {
				if self.ext.ambient && !self.is_declaration(statement) {
					return self.error(at, Code::StatementInAmbient);
				}
				body.push(statement);
			}
			self.ensure_progress(at)?;
		}
		self.next()?;
		self.ext.module_blocks -= 1;
		self.ext.in_namespace = in_namespace;
		self.exit_scope();
		let body = self.list_of(&body);
		Ok(self.ts(TsKind::ModuleBlock { body }, start))
	}

	fn parse_ambient_external_module_declaration(&mut self, start: u32) -> Result<NodeId> {
		let global = self.is_contextual("global");
		let id = if global {
			self.parse_ident(false)?
		} else if matches!(self.tok.kind, TokenKind::String(_)) {
			if !self.ext.ambient {
				return self.error(self.tok.start, Code::QuotedModuleName);
			}
			self.parse_expr_atom(&mut None, ForInit::No, false)?
		} else {
			return self.unexpected();
		};
		let body = if !self.is(TokenKind::BraceL) {
			self.semicolon()?;
			None
		} else if global {
			Some(self.parse_global_body()?)
		} else {
			Some(self.parse_module_body(false)?)
		};
		Ok(self.ts(TsKind::ModuleDeclaration { id, body, global }, start))
	}

	fn parse_module_or_namespace_declaration(&mut self, start: u32, nested: bool) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_module_or_namespace_declaration_inner(start, nested);
		self.leave();
		result
	}

	fn parse_module_or_namespace_declaration_inner(&mut self, start: u32, nested: bool) -> Result<NodeId> {
		let id = self.parse_ident(false)?;
		if !nested {
			self.check_lval_simple(id, Binding::None, &mut None)?;
			self.declare_export_only(id);
		}
		let body = if self.eat(TokenKind::Dot)? {
			let inner_start = self.tok.start;
			self.parse_module_or_namespace_declaration(inner_start, true)?
		} else {
			self.parse_module_body(true)?
		};
		Ok(self.ts(
			TsKind::ModuleDeclaration {
				id,
				body: Some(body),
				global: false,
			},
			start,
		))
	}

	// Type aliases and interfaces

	fn parse_type_alias_declaration(&mut self, start: u32) -> Result<NodeId> {
		let id = self.parse_ident(false)?;
		self.check_lval_simple(id, Binding::None, &mut None)?;
		self.declare_type(id, true)?;
		let (type_parameters, type_annotation) = self.in_type(|p| {
			let type_parameters = p.try_parse_type_parameters(TypeParameterModifiers::InOut)?;
			p.expect(TokenKind::Eq)?;
			if p.is_contextual("intrinsic") && p.peek_token()?.kind != TokenKind::Dot {
				let keyword_start = p.tok.start;
				p.next()?;
				let keyword = p.ts(TsKind::Keyword(super::ast::Keyword::Intrinsic), keyword_start);
				return Ok((type_parameters, keyword));
			}
			Ok((type_parameters, p.parse_type()?))
		})?;
		self.semicolon()?;
		Ok(self.ts(
			TsKind::TypeAliasDeclaration {
				id,
				type_parameters,
				type_annotation,
			},
			start,
		))
	}

	pub(super) fn parse_interface_declaration(&mut self, start: u32) -> Result<NodeId> {
		if !self.is_ident() {
			return self.error(self.tok.start, Code::InterfaceWithoutName);
		}
		let id = self.parse_ident(false)?;
		self.check_lval_simple(id, Binding::None, &mut None)?;
		self.declare_type(id, false)?;
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::InOut)?;
		let extends = if self.eat_keyword(Keyword::Extends)? {
			let clause = self.parse_heritage_clause("extends")?;
			Some(self.list_of(&clause))
		} else {
			None
		};
		let body_start = self.tok.start;
		self.expect(TokenKind::BraceL)?;
		let members = self.in_type(|p| p.parse_list(ListKind::TypeMembers, |p| p.parse_type_member()))?;
		self.expect(TokenKind::BraceR)?;
		let members = self.list_of(&members);
		let body = self.ts(TsKind::InterfaceBody { body: members }, body_start);
		let node = self.ts(
			TsKind::InterfaceDeclaration {
				id,
				type_parameters,
				extends,
				body,
			},
			start,
		);
		Ok(node)
	}

	// Ambient declarations

	/// In an ambient context only a `const` may be initialized, and only with a literal, a template
	/// without substitutions, or a reference to an enum member.
	pub(super) fn check_ambient_initializer(&mut self, declarator: NodeId, kind: VariableKind) -> Result<()> {
		let NodeKind::VariableDeclarator { id, init: Some(init) } = self.kind(declarator) else {
			return Ok(());
		};
		let annotated = self.ext_data().extras(id).is_some_and(|e| e.type_annotation.is_some());
		if kind != VariableKind::Const || annotated {
			return self.error(self.start_of(init), Code::InitializerInAmbient);
		}
		let literal = match self.kind(init) {
			NodeKind::StringLiteral { .. }
			| NodeKind::NumberLiteral { .. }
			| NodeKind::BigIntLiteral
			| NodeKind::BooleanLiteral { .. } => true,
			NodeKind::UnaryExpression {
				operator: crate::ast::UnaryOperator::Minus,
				argument,
			} => matches!(
				self.kind(argument),
				NodeKind::NumberLiteral { .. } | NodeKind::BigIntLiteral
			),
			NodeKind::TemplateLiteral { expressions, .. } => expressions.len == 0,
			NodeKind::MemberExpression { .. } => self.is_possibly_literal_enum(init),
			_ => false,
		};
		if !literal {
			return self.error(self.start_of(init), Code::AmbientConstInitializer);
		}
		Ok(())
	}

	fn is_possibly_literal_enum(&self, id: NodeId) -> bool {
		let NodeKind::MemberExpression {
			object,
			property,
			computed,
			..
		} = self.kind(id)
		else {
			return false;
		};
		if computed
			&& !matches!(
				self.kind(property),
				NodeKind::StringLiteral { .. }
					| NodeKind::NumberLiteral { .. }
					| NodeKind::TemplateLiteral {
						expressions: List { len: 0, .. },
						..
					}
			) {
			return false;
		}
		self.is_uncomputed_member_chain(object)
	}

	fn is_uncomputed_member_chain(&self, id: NodeId) -> bool {
		match self.kind(id) {
			NodeKind::Identifier { .. } => true,
			NodeKind::MemberExpression {
				object,
				computed: false,
				..
			} => self.is_uncomputed_member_chain(object),
			_ => false,
		}
	}

	pub(super) fn head(&self) -> Option<Head> {
		let TokenKind::Ident(name) = self.tok.kind else {
			return None;
		};
		if self.tok.escaped {
			return None;
		}
		Some(match self.str(name) {
			"abstract" => Head::Abstract,
			"declare" => Head::Declare,
			"enum" => Head::Enum,
			"global" => Head::Global,
			"interface" => Head::Interface,
			"module" => Head::Module,
			"namespace" => Head::Namespace,
			"type" => Head::Type,
			_ => return None,
		})
	}

	// rewinding after a confirmed lookahead read the next token twice: +1.7% on lib.dom.d.ts
	pub(super) fn open_declaration(&mut self) -> Result<Option<Opened>> {
		let snapshot = self.token_snapshot();
		let mut opened = Opened::default();
		loop {
			let at = self.tok.start;
			let Some(head) = self.head() else {
				let javascript = matches!(
					self.tok.kind,
					TokenKind::Keyword(Keyword::Var | Keyword::Const | Keyword::Function | Keyword::Class)
				) || self.is_contextual("let");
				let modified = opened.declare.is_some() || opened.is_abstract.is_some();
				if javascript && (modified || self.is_keyword(Keyword::Const) && self.peek_is_contextual("enum")?) {
					return Ok(Some(opened));
				}
				break;
			};
			match head {
				Head::Enum => {
					opened.head = Some(head);
					return Ok(Some(opened));
				}
				Head::Global => {
					if self.peek_token()?.kind != TokenKind::BraceL {
						break;
					}
					opened.head = Some(head);
					return Ok(Some(opened));
				}
				_ => {}
			}
			self.next_liberal()?;
			if self.tok.newline_before {
				break;
			}
			let named = match head {
				Head::Declare if opened.declare.is_none() => {
					opened.declare = Some(at);
					continue;
				}
				Head::Abstract if opened.is_abstract.is_none() => {
					opened.is_abstract = Some(at);
					continue;
				}
				Head::Interface => true,
				Head::Type => self.is_ident(),
				Head::Namespace | Head::Module => self.is_ident() || matches!(self.tok.kind, TokenKind::String(_)),
				_ => false,
			};
			if !named {
				break;
			}
			opened.head = Some(head);
			return Ok(Some(opened));
		}
		self.restore_tokens(snapshot);
		Ok(None)
	}

	pub(super) fn parse_opened(&mut self, start: u32, opened: Opened) -> Result<NodeId> {
		let Some(at) = opened.declare else {
			return self.parse_opened_declaration(start, opened);
		};
		if self.ext.ambient {
			return self.error(at, Code::DeclareInAmbient);
		}
		let node = self.ambient(|p| p.parse_opened_declaration(start, opened))?;
		self.extras_mut(node).declare = true;
		Ok(node)
	}

	fn parse_opened_declaration(&mut self, start: u32, opened: Opened) -> Result<NodeId> {
		if opened.is_abstract.is_some() || self.is_keyword(Keyword::Class) {
			if opened.head.is_some() || !self.is_keyword(Keyword::Class) {
				return self.error(start, Code::AbstractPlacement);
			}
			self.ext.next_class = ClassFrame {
				is_abstract: opened.is_abstract.is_some(),
				declare: opened.declare.is_some(),
				start: Some(start),
				..ClassFrame::default()
			};
			return self.parse_class(ClassKind::Declaration);
		}
		let Some(head) = opened.head else {
			let const_enum = self.is_keyword(Keyword::Const) && self.peek_is_contextual("enum")?;
			return match self.tok.kind {
				TokenKind::Keyword(Keyword::Function) => {
					self.next()?;
					self.parse_function(start, FUNC_STATEMENT, false, ForInit::No)
				}
				TokenKind::Keyword(Keyword::Const) if const_enum => {
					self.next()?;
					self.parse_enum_declaration(start, true, false)
				}
				TokenKind::Keyword(Keyword::Const) => self.parse_var_statement(start, VariableKind::Const),
				TokenKind::Keyword(Keyword::Var) => self.parse_var_statement(start, VariableKind::Var),
				_ => self.parse_var_statement(start, VariableKind::Let),
			};
		};
		match head {
			Head::Enum => self.parse_enum_declaration(start, false, false),
			Head::Global => self.parse_ambient_external_module_declaration(start),
			Head::Interface => self.parse_interface_declaration(start),
			Head::Type => self.parse_type_alias_declaration(start),
			Head::Module if matches!(self.tok.kind, TokenKind::String(_)) => {
				self.parse_ambient_external_module_declaration(start)
			}
			_ => self.parse_module_or_namespace_declaration(start, false),
		}
	}

	// Modules

	pub(super) fn parse_import_equals_declaration(&mut self, start: u32, is_export: bool) -> Result<NodeId> {
		let id = self.parse_ident(false)?;
		self.check_lval_simple(id, Binding::Lexical, &mut None)?;
		self.expect(TokenKind::Eq)?;
		let import_kind = self.ext.outer_kind.unwrap_or(Kind::Value);
		let module_reference = if self.is_contextual("require") && self.peek_char().0 == Some('(') {
			let reference_start = self.tok.start;
			self.next()?;
			self.expect(TokenKind::ParenL)?;
			if !matches!(self.tok.kind, TokenKind::String(_)) {
				return self.unexpected();
			}
			let expression = self.parse_expr_atom(&mut None, ForInit::No, false)?;
			self.expect(TokenKind::ParenR)?;
			self.ts(TsKind::ExternalModuleReference { expression }, reference_start)
		} else {
			let reference = self.parse_entity_name(false)?;
			if import_kind == Kind::Type {
				return self.error(self.start_of(reference), Code::ImportTypeAlias);
			}
			reference
		};
		self.semicolon()?;
		self.ext.outer_kind = None;
		Ok(self.ts(
			TsKind::ImportEqualsDeclaration {
				id,
				module_reference,
				is_export,
				import_kind,
			},
			start,
		))
	}

	/// A named import or export specifier starting with `type`, which may be the modifier or the
	/// name itself.
	pub(super) fn parse_type_only_specifier(&mut self, is_import: bool) -> Result<NodeId> {
		let start = self.tok.start;
		let in_type_only = self.ext.outer_kind == Some(Kind::Type);
		let mut left = self.parse_module_export_name()?;
		let mut right = None;
		let mut has_type_specifier = false;
		let mut can_parse_as = true;
		if self.is_contextual("as") {
			let first_as = self.parse_ident(false)?;
			if self.is_contextual("as") {
				let second_as = self.parse_ident(false)?;
				if self.is_ident_or_keyword() {
					has_type_specifier = true;
					left = first_as;
					right = Some(self.parse_specifier_name(is_import)?);
				} else {
					right = Some(second_as);
				}
				can_parse_as = false;
			} else if self.is_ident_or_keyword() {
				can_parse_as = false;
				right = Some(self.parse_specifier_name(is_import)?);
			} else {
				has_type_specifier = true;
				left = first_as;
			}
		} else if self.is_ident_or_keyword() {
			has_type_specifier = true;
			left = if is_import {
				let local = self.parse_ident(true)?;
				if !self.is_contextual("as") {
					self.check_unreserved(local)?;
				}
				local
			} else {
				self.parse_module_export_name()?
			};
		}
		if has_type_specifier && in_type_only {
			return self.error(start, Code::TypeModifierInTypeImport);
		}
		if can_parse_as && self.eat_contextual("as")? {
			right = Some(self.parse_specifier_name(is_import)?);
		}
		let right = match right {
			Some(right) => right,
			None => self.copy_node(left),
		};
		let kind = if has_type_specifier { Kind::Type } else { Kind::Value };
		if is_import {
			self.check_lval_simple(
				right,
				if in_type_only || has_type_specifier {
					Binding::None
				} else {
					Binding::Lexical
				},
				&mut None,
			)?;
			if has_type_specifier {
				self.declare_export_only(right);
			}
		}
		let node = if is_import {
			let node = self.add(
				NodeKind::ImportSpecifier {
					imported: left,
					local: right,
				},
				start,
			);
			self.extras_mut(node).import_kind = Some(kind);
			node
		} else {
			let node = self.add(
				NodeKind::ExportSpecifier {
					local: left,
					exported: right,
				},
				start,
			);
			self.extras_mut(node).export_kind = Some(kind);
			node
		};
		Ok(node)
	}

	fn parse_specifier_name(&mut self, is_import: bool) -> Result<NodeId> {
		if is_import {
			self.parse_ident(false)
		} else {
			self.parse_module_export_name()
		}
	}
}
