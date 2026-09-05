//! Declarations that only exist in TypeScript, and the ambient and type-only forms of the
//! JavaScript ones.

use super::ast::{Kind, TsKind};
use super::types::{ListKind, TypeParameterModifiers};
use super::{ClassFrame, TypeScript};
use crate::ast::{NodeId, NodeKind, VariableKind};
use crate::lexer::token::{Keyword, TokenKind};
use crate::parser::class::ClassKind;
use crate::parser::scope::Binding;
use crate::parser::statement::FUNC_STATEMENT;
use crate::parser::{Context, ForInit, Parser, Result};
use std::collections::HashSet;

/// The scope flags acorn-typescript gives module blocks; the inner one is also the class field
/// initializer flag, so `arguments` is rejected inside namespaces the same way.
const SCOPE_TS_MODULE: u32 = 1024;
const SCOPE_TS_OTHER: u32 = 512;

/// The identifiers that open a declaration when what follows allows it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Word {
	Abstract,
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
			Some(self.parse_maybe_assign(ForInit::No, &mut None)?)
		} else {
			None
		};
		Ok(self.ts(TsKind::EnumMember { id, initializer }, start))
	}

	// Namespaces

	fn parse_module_block(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.enter_scope(SCOPE_TS_OTHER);
		self.ext.module_blocks += 1;
		self.expect(TokenKind::BraceL)?;
		let mut body = Vec::new();
		let mut exports = HashSet::new();
		while !self.is(TokenKind::BraceR) {
			body.push(self.parse_statement(Context::None, true, Some(&mut exports))?);
		}
		self.next()?;
		self.ext.module_blocks -= 1;
		self.exit_scope();
		let body = self.list_of(&body);
		Ok(self.ts(TsKind::ModuleBlock { body }, start))
	}

	fn parse_ambient_external_module_declaration(&mut self, start: u32) -> Result<NodeId> {
		let global = self.is_contextual("global");
		let id = if global {
			self.parse_ident(false)?
		} else if matches!(self.tok.kind, TokenKind::String(_)) {
			self.parse_expr_atom(&mut None, ForInit::No, false)?
		} else {
			return self.unexpected();
		};
		let body = if self.is(TokenKind::BraceL) {
			self.enter_scope(SCOPE_TS_MODULE);
			let body = self.parse_module_block()?;
			self.exit_scope();
			Some(body)
		} else {
			self.semicolon()?;
			None
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
			self.enter_scope(SCOPE_TS_MODULE);
			let body = self.parse_module_block()?;
			self.exit_scope();
			body
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

	fn parse_global_declaration(&mut self, start: u32, id: NodeId) -> Result<NodeId> {
		self.enter_scope(SCOPE_TS_MODULE);
		let body = self.parse_module_block()?;
		self.exit_scope();
		Ok(self.ts(
			TsKind::ModuleDeclaration {
				id,
				body: Some(body),
				global: true,
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

	/// `None` when a line break follows `interface`, which then is a plain identifier.
	pub(super) fn parse_interface_declaration(
		&mut self,
		start: u32,
		declare: bool,
		is_abstract: bool,
	) -> Result<Option<NodeId>> {
		if self.has_following_line_break() {
			return Ok(None);
		}
		self.expect_contextual("interface")?;
		if !self.is_ident() {
			return self.error(
				self.tok.start,
				"'interface' declarations must be followed by an identifier.",
			);
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
		let extras = self.extras_mut(node);
		extras.declare = declare;
		extras.is_abstract = is_abstract;
		Ok(Some(node))
	}

	fn parse_abstract_declaration(&mut self, start: u32) -> Result<Option<NodeId>> {
		if self.is_keyword(Keyword::Class) {
			self.ext.next_class = ClassFrame {
				is_abstract: true,
				start: Some(start),
				..ClassFrame::default()
			};
			return self.parse_class(ClassKind::Declaration).map(Some);
		}
		if self.is_contextual("interface") {
			if !self.has_following_line_break() {
				return self.parse_interface_declaration(start, false, true);
			}
			return Ok(None);
		}
		self.unexpected_at(start)
	}

	// Ambient declarations

	/// After `declare`, which is the current statement's first identifier.
	pub(super) fn try_parse_declare(&mut self, start: u32) -> Result<Option<NodeId>> {
		if self.is_line_terminator()? {
			return Ok(None);
		}
		self.ambient(|p| {
			let const_enum = p.is_keyword(Keyword::Const) && p.peek_is_contextual("enum")?;
			let node = match p.tok.kind {
				TokenKind::Keyword(Keyword::Function) => {
					p.next()?;
					p.parse_function(start, FUNC_STATEMENT, false, ForInit::No)?
				}
				TokenKind::Keyword(Keyword::Class) => {
					p.ext.next_class = ClassFrame {
						declare: true,
						start: Some(start),
						..ClassFrame::default()
					};
					p.parse_class(ClassKind::Declaration)?
				}
				TokenKind::Keyword(Keyword::Const) if const_enum => {
					p.next()?;
					p.parse_enum_declaration(start, true, true)?
				}
				TokenKind::Keyword(Keyword::Const) | TokenKind::Keyword(Keyword::Var) => {
					let kind = if p.is_keyword(Keyword::Const) {
						VariableKind::Const
					} else {
						VariableKind::Var
					};
					p.parse_var_statement(start, kind)?
				}
				TokenKind::Ident(name) => match p.str(name) {
					"let" => p.parse_var_statement(start, VariableKind::Let)?,
					"enum" => p.parse_enum_declaration(start, false, true)?,
					"global" => p.parse_ambient_external_module_declaration(start)?,
					"interface" => match p.parse_interface_declaration(start, true, false)? {
						Some(node) => node,
						None => return Ok(None),
					},
					_ => match p.parse_declaration(start, name, true)? {
						Some(node) => node,
						None => return Ok(None),
					},
				},
				_ => return Ok(None),
			};
			p.extras_mut(node).declare = true;
			Ok(Some(node))
		})
	}

	/// In an ambient context only a `const` may be initialized, and only with a string or numeric
	/// literal, a template without substitutions, or a reference to an enum member.
	pub(super) fn check_ambient_initializer(&mut self, declarator: NodeId, kind: VariableKind) -> Result<()> {
		let NodeKind::VariableDeclarator { id, init: Some(init) } = self.kind(declarator) else {
			return Ok(());
		};
		let annotated = self.ext_data().extras(id).is_some_and(|e| e.type_annotation.is_some());
		if kind != VariableKind::Const || annotated {
			return self.error(self.start_of(init), "Initializers are not allowed in ambient contexts.");
		}
		let literal = match self.kind(init) {
			NodeKind::StringLiteral { .. } | NodeKind::NumberLiteral { .. } | NodeKind::BigIntLiteral => true,
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
			return self.error(
				self.start_of(init),
				"A 'const' initializer in an ambient context must be a string or numeric literal or literal enum reference.",
			);
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
			&& !matches!(self.kind(property), NodeKind::TemplateLiteral { expressions, .. } if expressions.len == 0)
		{
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

	/// The declarations introduced by an identifier: `abstract`, `module`, `namespace`, `type`.
	/// With `next`, the identifier is still the current token.
	pub(super) fn parse_declaration(
		&mut self,
		start: u32,
		name: crate::interner::StrId,
		next: bool,
	) -> Result<Option<NodeId>> {
		let word = match self.str(name) {
			"abstract" => Word::Abstract,
			"module" => Word::Module,
			"namespace" => Word::Namespace,
			"type" => Word::Type,
			_ => return Ok(None),
		};
		if !self.check_line_terminator(next)? {
			return Ok(None);
		}
		match word {
			Word::Abstract if self.is_keyword(Keyword::Class) || self.is_ident() => {
				self.parse_abstract_declaration(start)
			}
			Word::Module if matches!(self.tok.kind, TokenKind::String(_)) => {
				self.parse_ambient_external_module_declaration(start).map(Some)
			}
			Word::Module | Word::Namespace if self.is_ident() => {
				self.parse_module_or_namespace_declaration(start, false).map(Some)
			}
			Word::Type if self.is_ident() => self.parse_type_alias_declaration(start).map(Some),
			_ => Ok(None),
		}
	}

	fn check_line_terminator(&mut self, next: bool) -> Result<bool> {
		if next {
			if self.has_following_line_break() {
				return Ok(false);
			}
			self.next()?;
			return Ok(true);
		}
		Ok(!self.is_line_terminator()?)
	}

	/// An expression statement that turned out to be a bare identifier.
	pub(super) fn parse_declaration_statement(&mut self, start: u32, expression: NodeId) -> Result<Option<NodeId>> {
		let NodeKind::Identifier { name } = self.kind(expression) else {
			unreachable!()
		};
		match self.str(name) {
			"declare" => self.try_parse_declare(start),
			"global" => {
				if self.is(TokenKind::BraceL) {
					return self.parse_global_declaration(start, expression).map(Some);
				}
				Ok(None)
			}
			_ => self.parse_declaration(start, name, false),
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
				return self.error(self.start_of(reference), "An import alias can not use 'import type'.");
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
			return self.error(
				start,
				if is_import {
					"The 'type' modifier cannot be used on a named import when 'import type' is used on its import statement."
				} else {
					"The 'type' modifier cannot be used on a named export when 'export type' is used on its export statement."
				},
			);
		}
		if can_parse_as && self.eat_contextual("as")? {
			right = Some(self.parse_specifier_name(is_import)?);
		}
		let right = match right {
			Some(right) => right,
			None => self.copy_node(left),
		};
		if is_import {
			self.check_lval_simple(right, Binding::Lexical, &mut None)?;
		}
		let kind = if has_type_specifier { Kind::Type } else { Kind::Value };
		let node = if is_import {
			self.add(
				NodeKind::ImportSpecifier {
					imported: left,
					local: right,
				},
				start,
			)
		} else {
			self.add(
				NodeKind::ExportSpecifier {
					local: left,
					exported: right,
				},
				start,
			)
		};
		let extras = self.extras_mut(node);
		if is_import {
			extras.import_kind = Some(kind);
		} else {
			extras.export_kind = Some(kind);
		}
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
