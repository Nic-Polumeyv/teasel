//! The type grammar: everything after a `:` or inside `<...>`.

use super::ast::{Keyword as TsKeyword, Modifier, SignatureKind, TsKind};
use super::{Modifiers, TypeScript};
use crate::ast::{List, NodeId, NodeKind};
use crate::lexer::token::{Keyword, TokenKind};
use crate::parser::{ForInit, Parser, Result};

/// What closes a delimited list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ListKind {
	EnumMembers,
	TypeMembers,
	HeritageClause,
	TupleElements,
	TypeParametersOrArguments,
}

/// Which modifiers a type parameter list accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeParameterModifiers {
	None,
	InOut,
	Const,
}

const IN_OUT: &[&str] = &["in", "out"];
const ACCESSIBILITY_AND_CLASS: &[&str] = &[
	"public",
	"private",
	"protected",
	"readonly",
	"declare",
	"abstract",
	"override",
];

impl Parser<'_, TypeScript> {
	// Contexts

	pub(super) fn in_type<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
		let old = self.lexer.in_type;
		self.lexer.in_type = true;
		let result = f(self);
		self.lexer.in_type = old;
		result
	}

	fn allow_conditional_types<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
		let old = self.ext.disallow_conditional_types;
		self.ext.disallow_conditional_types = false;
		let result = f(self);
		self.ext.disallow_conditional_types = old;
		result
	}

	fn disallow_conditional_types<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
		let old = self.ext.disallow_conditional_types;
		self.ext.disallow_conditional_types = true;
		let result = f(self);
		self.ext.disallow_conditional_types = old;
		result
	}

	/// Runs `f` and puts the tokenizer back where it was, whatever `f` did.
	pub(super) fn lookahead<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
		let snapshot = self.snapshot();
		let result = f(self);
		self.restore(snapshot);
		result
	}

	/// `next(); parse_type()` in a type context: the caller's current token opens the type.
	pub(super) fn next_then_parse_type(&mut self) -> Result<NodeId> {
		self.in_type(|p| {
			p.next()?;
			p.parse_type()
		})
	}

	fn eat_then_parse_type(&mut self, kind: TokenKind) -> Result<Option<NodeId>> {
		if self.is(kind) {
			self.next_then_parse_type().map(Some)
		} else {
			Ok(None)
		}
	}

	fn eat_keyword_then_parse_type(&mut self, keyword: Keyword) -> Result<Option<NodeId>> {
		self.eat_then_parse_type(TokenKind::Keyword(keyword))
	}

	// Lists

	fn is_list_terminator(&self, kind: ListKind) -> bool {
		match kind {
			ListKind::EnumMembers | ListKind::TypeMembers => self.is(TokenKind::BraceR),
			ListKind::HeritageClause => self.is(TokenKind::BraceL),
			ListKind::TupleElements => self.is(TokenKind::BracketR),
			ListKind::TypeParametersOrArguments => self.is(TokenKind::Gt),
		}
	}

	pub(super) fn parse_delimited_list(
		&mut self,
		kind: ListKind,
		mut element: impl FnMut(&mut Self) -> Result<NodeId>,
	) -> Result<Vec<NodeId>> {
		let mut result = Vec::new();
		loop {
			if self.is_list_terminator(kind) {
				break;
			}
			result.push(element(self)?);
			if self.eat(TokenKind::Comma)? {
				continue;
			}
			if self.is_list_terminator(kind) {
				break;
			}
			self.expect(TokenKind::Comma)?;
		}
		Ok(result)
	}

	pub(super) fn parse_list(
		&mut self,
		kind: ListKind,
		mut element: impl FnMut(&mut Self) -> Result<NodeId>,
	) -> Result<Vec<NodeId>> {
		let mut result = Vec::new();
		while !self.is_list_terminator(kind) {
			result.push(element(self)?);
		}
		Ok(result)
	}

	// Annotations

	/// `: type`, or just the type when `eat_colon` is false. The node starts at `start`, which is
	/// the colon when there is one.
	pub(super) fn parse_type_annotation(&mut self, eat_colon: bool, start: Option<u32>) -> Result<NodeId> {
		let start = start.unwrap_or(self.tok.start);
		let type_annotation = self.in_type(|p| {
			if eat_colon {
				p.expect(TokenKind::Colon)?;
			}
			p.parse_type()
		})?;
		Ok(self.ts(TsKind::TypeAnnotation { type_annotation }, start))
	}

	pub(super) fn try_parse_type_annotation(&mut self) -> Result<Option<NodeId>> {
		if self.is(TokenKind::Colon) {
			self.parse_type_annotation(true, None).map(Some)
		} else {
			Ok(None)
		}
	}

	/// A return type after `return_token`, which may be a type predicate.
	pub(super) fn parse_type_or_type_predicate_annotation(&mut self, return_token: TokenKind) -> Result<NodeId> {
		self.in_type(|p| {
			let annotation_start = p.tok.start;
			p.expect(return_token)?;
			let start = p.tok.start;
			let asserts = p.try_parse(|p| p.parse_type_predicate_asserts())?.unwrap_or(false);
			if asserts && p.is_keyword(Keyword::This) {
				let predicate = p.parse_this_type_or_this_type_predicate()?;
				let predicate = match p.ts_kind(predicate) {
					Some(TsKind::ThisType) => p.ts(
						TsKind::TypePredicate {
							parameter_name: predicate,
							type_annotation: None,
							asserts: true,
						},
						start,
					),
					_ => {
						p.ast.node_mut(predicate).start = start;
						if let Some(TsKind::TypePredicate { asserts, .. }) = p.ts_kind_mut(predicate) {
							*asserts = true;
						}
						predicate
					}
				};
				return Ok(p.ts(
					TsKind::TypeAnnotation {
						type_annotation: predicate,
					},
					annotation_start,
				));
			}
			let variable = if p.is_ident() {
				p.try_parse(|p| p.parse_type_predicate_prefix())?
			} else {
				None
			};
			let Some(parameter_name) = variable else {
				if !asserts {
					return p.parse_type_annotation(false, Some(annotation_start));
				}
				let parameter_name = p.parse_ident(false)?;
				let predicate = p.ts(
					TsKind::TypePredicate {
						parameter_name,
						type_annotation: None,
						asserts,
					},
					start,
				);
				return Ok(p.ts(
					TsKind::TypeAnnotation {
						type_annotation: predicate,
					},
					annotation_start,
				));
			};
			let type_annotation = p.parse_type_annotation(false, None)?;
			let predicate = p.ts(
				TsKind::TypePredicate {
					parameter_name,
					type_annotation: Some(type_annotation),
					asserts,
				},
				start,
			);
			Ok(p.ts(
				TsKind::TypeAnnotation {
					type_annotation: predicate,
				},
				annotation_start,
			))
		})
	}

	/// Runs `f`, keeping its result only when it is `Some`; otherwise the tokenizer goes back.
	pub(super) fn try_parse<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<Option<T>>) -> Result<Option<T>> {
		let snapshot = self.snapshot();
		let result = f(self)?;
		if result.is_none() {
			self.restore(snapshot);
		}
		Ok(result)
	}

	fn parse_type_predicate_asserts(&mut self) -> Result<Option<bool>> {
		if !self.is_contextual("asserts") {
			return Ok(None);
		}
		self.next()?;
		if !self.is_ident() && !self.is_keyword(Keyword::This) {
			return Ok(None);
		}
		Ok(Some(true))
	}

	fn parse_type_predicate_prefix(&mut self) -> Result<Option<NodeId>> {
		let id = self.parse_ident(false)?;
		if self.is_contextual("is") && !self.tok.newline_before {
			self.next()?;
			return Ok(Some(id));
		}
		Ok(None)
	}

	fn parse_this_type_node(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		Ok(self.ts(TsKind::ThisType, start))
	}

	fn parse_this_type_or_this_type_predicate(&mut self) -> Result<NodeId> {
		let this = self.parse_this_type_node()?;
		if self.is_contextual("is") && !self.tok.newline_before {
			self.next()?;
			let type_annotation = self.parse_type_annotation(false, None)?;
			let start = self.start_of(this);
			return Ok(self.ts(
				TsKind::TypePredicate {
					parameter_name: this,
					type_annotation: Some(type_annotation),
					asserts: false,
				},
				start,
			));
		}
		Ok(this)
	}

	// Types

	pub(super) fn parse_type(&mut self) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_type_inner();
		self.leave();
		result
	}

	fn parse_type_inner(&mut self) -> Result<NodeId> {
		debug_assert!(self.lexer.in_type);
		let check_type = self.parse_non_conditional_type()?;
		if self.ext.disallow_conditional_types || self.tok.newline_before || !self.eat_keyword(Keyword::Extends)? {
			return Ok(check_type);
		}
		let extends_type = self.disallow_conditional_types(|p| p.parse_non_conditional_type())?;
		self.expect(TokenKind::Question)?;
		let true_type = self.allow_conditional_types(|p| p.parse_type())?;
		self.expect(TokenKind::Colon)?;
		let false_type = self.allow_conditional_types(|p| p.parse_type())?;
		let start = self.start_of(check_type);
		Ok(self.ts(
			TsKind::ConditionalType {
				check_type,
				extends_type,
				true_type,
				false_type,
			},
			start,
		))
	}

	fn parse_non_conditional_type(&mut self) -> Result<NodeId> {
		if self.is_start_of_function_type()? {
			return self.parse_function_or_constructor_type(false, false);
		}
		if self.is_keyword(Keyword::New) {
			return self.parse_function_or_constructor_type(true, false);
		}
		if self.is_contextual("abstract") && self.peek_token()?.kind == TokenKind::Keyword(Keyword::New) {
			return self.parse_function_or_constructor_type(true, true);
		}
		self.parse_union_type_or_higher()
	}

	fn is_start_of_function_type(&mut self) -> Result<bool> {
		if self.is(TokenKind::Lt) {
			return Ok(true);
		}
		if !self.is(TokenKind::ParenL) {
			return Ok(false);
		}
		self.lookahead(|p| p.is_unambiguously_start_of_function_type())
	}

	fn is_unambiguously_start_of_function_type(&mut self) -> Result<bool> {
		self.next()?;
		if self.is(TokenKind::ParenR) || self.is(TokenKind::Ellipsis) {
			return Ok(true);
		}
		if self.skip_parameter_start()? {
			if self.is(TokenKind::Colon)
				|| self.is(TokenKind::Comma)
				|| self.is(TokenKind::Question)
				|| self.is(TokenKind::Eq)
			{
				return Ok(true);
			}
			if self.is(TokenKind::ParenR) {
				self.next()?;
				if self.is(TokenKind::Arrow) {
					return Ok(true);
				}
			}
		}
		Ok(false)
	}

	fn skip_parameter_start(&mut self) -> Result<bool> {
		if self.is_ident() || self.is_keyword(Keyword::This) {
			self.next()?;
			return Ok(true);
		}
		if self.is(TokenKind::BraceL) {
			return Ok(self.attempt(|p| p.parse_obj(true, &mut None)).is_some());
		}
		if self.is(TokenKind::BracketL) {
			self.next()?;
			return Ok(self
				.attempt(|p| p.parse_binding_list(TokenKind::BracketR, true, true, false))
				.is_some());
		}
		Ok(false)
	}

	fn parse_function_or_constructor_type(&mut self, constructor: bool, is_abstract: bool) -> Result<NodeId> {
		let start = self.tok.start;
		if constructor {
			if is_abstract {
				self.next()?;
			}
			self.next()?;
		}
		let (type_parameters, parameters, type_annotation) =
			self.allow_conditional_types(|p| p.fill_signature(TokenKind::Arrow))?;
		let type_annotation = type_annotation.unwrap();
		let kind = if constructor {
			TsKind::ConstructorType {
				type_parameters,
				parameters,
				type_annotation,
				is_abstract,
			}
		} else {
			TsKind::FunctionType {
				type_parameters,
				parameters,
				type_annotation,
			}
		};
		Ok(self.ts(kind, start))
	}

	/// Type parameters, parameters and the return type after `return_token`, which is required
	/// when it is `=>`.
	pub(super) fn fill_signature(&mut self, return_token: TokenKind) -> Result<(Option<NodeId>, List, Option<NodeId>)> {
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::None)?;
		self.expect(TokenKind::ParenL)?;
		let parameters = self.parse_binding_list_for_signature()?;
		let type_annotation = if return_token == TokenKind::Arrow || self.is(return_token) {
			Some(self.parse_type_or_type_predicate_annotation(return_token)?)
		} else {
			None
		};
		Ok((type_parameters, parameters, type_annotation))
	}

	fn parse_binding_list_for_signature(&mut self) -> Result<List> {
		let params = self.parse_binding_list(TokenKind::ParenR, true, true, false)?;
		for param in params.iter().flatten() {
			if !matches!(
				self.kind(*param),
				NodeKind::Identifier { .. }
					| NodeKind::RestElement { .. }
					| NodeKind::ObjectPattern { .. }
					| NodeKind::ArrayPattern { .. }
			) {
				return self.error(
					self.start_of(*param),
					"Name in a signature must be an Identifier, ObjectPattern or ArrayPattern, instead got AssignmentPattern.",
				);
			}
		}
		Ok(self.list(&params))
	}

	fn parse_union_type_or_higher(&mut self) -> Result<NodeId> {
		self.parse_union_or_intersection_type(TokenKind::Pipe, |p| p.parse_intersection_type_or_higher())
	}

	fn parse_intersection_type_or_higher(&mut self) -> Result<NodeId> {
		self.parse_union_or_intersection_type(TokenKind::Amp, |p| p.parse_type_operator_or_higher())
	}

	fn parse_union_or_intersection_type(
		&mut self,
		operator: TokenKind,
		mut constituent: impl FnMut(&mut Self) -> Result<NodeId>,
	) -> Result<NodeId> {
		let start = self.tok.start;
		let has_leading_operator = self.eat(operator)?;
		let mut types = vec![constituent(self)?];
		while self.eat(operator)? {
			types.push(constituent(self)?);
		}
		if types.len() == 1 && !has_leading_operator {
			return Ok(types[0]);
		}
		let types = self.list_of(&types);
		let kind = if operator == TokenKind::Pipe {
			TsKind::UnionType { types }
		} else {
			TsKind::IntersectionType { types }
		};
		Ok(self.ts(kind, start))
	}

	fn parse_type_operator_or_higher(&mut self) -> Result<NodeId> {
		if let Some(name) = self.ident_name()
			&& !self.tok.escaped
		{
			match self.str(name) {
				"keyof" | "readonly" | "unique" => return self.parse_type_operator(),
				"infer" => return self.parse_infer_type(),
				_ => {}
			}
		}
		self.allow_conditional_types(|p| p.parse_array_type_or_higher())
	}

	fn parse_type_operator(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let operator = self.ident_name().unwrap();
		self.next()?;
		self.enter()?;
		let type_annotation = self.parse_type_operator_or_higher();
		self.leave();
		let type_annotation = type_annotation?;
		if self.str(operator) == "readonly"
			&& !matches!(
				self.ts_kind(type_annotation),
				Some(TsKind::TupleType { .. } | TsKind::ArrayType { .. })
			) {
			return self.error(
				start,
				"'readonly' type modifier is only permitted on array and tuple literal types.",
			);
		}
		Ok(self.ts(
			TsKind::TypeOperator {
				operator,
				type_annotation,
			},
			start,
		))
	}

	fn parse_infer_type(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.expect_contextual("infer")?;
		let parameter_start = self.tok.start;
		let name = self.parse_type_parameter_name()?;
		let constraint = self.try_parse(|p| p.parse_constraint_for_infer_type())?;
		let type_parameter = self.ts(
			TsKind::TypeParameter {
				name,
				constraint,
				default: None,
				is_in: false,
				is_out: false,
				is_const: false,
			},
			parameter_start,
		);
		Ok(self.ts(TsKind::InferType { type_parameter }, start))
	}

	fn parse_constraint_for_infer_type(&mut self) -> Result<Option<NodeId>> {
		if self.eat_keyword(Keyword::Extends)? {
			let constraint = self.disallow_conditional_types(|p| p.parse_type())?;
			if self.ext.disallow_conditional_types || !self.is(TokenKind::Question) {
				return Ok(Some(constraint));
			}
		}
		Ok(None)
	}

	fn parse_array_type_or_higher(&mut self) -> Result<NodeId> {
		let mut ty = self.parse_non_array_type()?;
		while !self.tok.newline_before && self.eat(TokenKind::BracketL)? {
			let start = self.start_of(ty);
			if self.is(TokenKind::BracketR) {
				self.next()?;
				ty = self.ts(TsKind::ArrayType { element_type: ty }, start);
			} else {
				let index_type = self.parse_type()?;
				self.expect(TokenKind::BracketR)?;
				ty = self.ts(
					TsKind::IndexedAccessType {
						object_type: ty,
						index_type,
					},
					start,
				);
			}
		}
		Ok(ty)
	}

	fn parse_non_array_type(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		match self.tok.kind {
			TokenKind::String(_)
			| TokenKind::Number(_)
			| TokenKind::BigInt
			| TokenKind::Keyword(Keyword::True)
			| TokenKind::Keyword(Keyword::False) => {
				let literal = self.parse_expr_atom(&mut None, ForInit::No, false)?;
				Ok(self.ts(TsKind::LiteralType { literal }, start))
			}
			TokenKind::Minus => {
				if !matches!(self.peek_token()?.kind, TokenKind::Number(_) | TokenKind::BigInt) {
					return self.unexpected();
				}
				let literal = self.parse_maybe_unary(&mut None, false, false, ForInit::No)?;
				Ok(self.ts(TsKind::LiteralType { literal }, start))
			}
			TokenKind::Keyword(Keyword::This) => self.parse_this_type_or_this_type_predicate(),
			TokenKind::Keyword(Keyword::Typeof) => self.parse_type_query(),
			TokenKind::Keyword(Keyword::Import) => self.parse_import_type(),
			TokenKind::BraceL => {
				if self.lookahead(|p| p.is_start_of_mapped_type())? {
					self.parse_mapped_type()
				} else {
					self.parse_type_literal()
				}
			}
			TokenKind::BracketL => self.parse_tuple_type(),
			TokenKind::ParenL => self.parse_parenthesized_type(),
			TokenKind::Backquote => {
				let literal = self.parse_template(false)?;
				Ok(self.ts(TsKind::LiteralType { literal }, start))
			}
			TokenKind::Ident(name) => {
				if let Some(keyword) = TsKeyword::from_name(self.str(name))
					&& self.peek_char().0 != Some('.')
				{
					self.next()?;
					return Ok(self.ts(TsKind::Keyword(keyword), start));
				}
				self.parse_type_reference()
			}
			TokenKind::Keyword(Keyword::Void) | TokenKind::Keyword(Keyword::Null) => {
				let keyword = if self.is_keyword(Keyword::Void) {
					TsKeyword::Void
				} else {
					TsKeyword::Null
				};
				if self.peek_char().0 != Some('.') {
					self.next()?;
					return Ok(self.ts(TsKind::Keyword(keyword), start));
				}
				self.parse_type_reference()
			}
			_ => self.unexpected(),
		}
	}

	pub(super) fn parse_type_reference(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let type_name = self.parse_entity_name(true)?;
		let type_arguments = if !self.tok.newline_before && self.is(TokenKind::Lt) {
			Some(self.parse_type_arguments()?)
		} else {
			None
		};
		Ok(self.ts(
			TsKind::TypeReference {
				type_name,
				type_arguments,
			},
			start,
		))
	}

	pub(super) fn parse_entity_name(&mut self, allow_reserved_words: bool) -> Result<NodeId> {
		let mut entity = self.parse_ident(allow_reserved_words)?;
		while self.eat(TokenKind::Dot)? {
			let start = self.start_of(entity);
			let right = self.parse_ident(allow_reserved_words)?;
			entity = self.ts(TsKind::QualifiedName { left: entity, right }, start);
		}
		Ok(entity)
	}

	fn parse_type_query(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.expect_keyword(Keyword::Typeof)?;
		let expr_name = if self.is_keyword(Keyword::Import) {
			self.parse_import_type()?
		} else {
			self.parse_entity_name(true)?
		};
		let type_arguments = if !self.tok.newline_before && self.is(TokenKind::Lt) {
			Some(self.parse_type_arguments()?)
		} else {
			None
		};
		Ok(self.ts(
			TsKind::TypeQuery {
				expr_name,
				type_arguments,
			},
			start,
		))
	}

	fn parse_import_type(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.expect_keyword(Keyword::Import)?;
		self.expect(TokenKind::ParenL)?;
		if !matches!(self.tok.kind, TokenKind::String(_)) {
			return self.error(self.tok.start, "Argument in a type import must be a string literal.");
		}
		let argument = self.parse_expr_atom(&mut None, ForInit::No, false)?;
		self.expect(TokenKind::ParenR)?;
		let qualifier = if self.eat(TokenKind::Dot)? {
			Some(self.parse_entity_name(true)?)
		} else {
			None
		};
		let type_arguments = if self.is(TokenKind::Lt) {
			Some(self.parse_type_arguments()?)
		} else {
			None
		};
		Ok(self.ts(
			TsKind::ImportType {
				argument,
				qualifier,
				type_arguments,
			},
			start,
		))
	}

	fn is_start_of_mapped_type(&mut self) -> Result<bool> {
		self.next()?;
		if self.eat(TokenKind::Plus)? || self.eat(TokenKind::Minus)? {
			return Ok(self.is_contextual("readonly"));
		}
		if self.is_contextual("readonly") {
			self.next()?;
		}
		if !self.is(TokenKind::BracketL) {
			return Ok(false);
		}
		self.next()?;
		if !self.is_ident() {
			return Ok(false);
		}
		self.next()?;
		Ok(self.is_keyword(Keyword::In))
	}

	fn parse_mapped_type(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.expect(TokenKind::BraceL)?;
		let readonly = if let Some(sign) = self.eat_sign()? {
			self.expect_contextual("readonly")?;
			Some(sign)
		} else if self.eat_contextual("readonly")? {
			Some(Modifier::True)
		} else {
			None
		};
		self.expect(TokenKind::BracketL)?;
		let parameter_start = self.tok.start;
		let name = self.parse_type_parameter_name()?;
		let constraint = self.in_type(|p| {
			p.expect_keyword(Keyword::In)?;
			p.parse_type()
		})?;
		let type_parameter = self.ts(
			TsKind::TypeParameter {
				name,
				constraint: Some(constraint),
				default: None,
				is_in: false,
				is_out: false,
				is_const: false,
			},
			parameter_start,
		);
		let name_type = if self.eat_contextual("as")? {
			Some(self.parse_type()?)
		} else {
			None
		};
		self.expect(TokenKind::BracketR)?;
		let optional = if let Some(sign) = self.eat_sign()? {
			self.expect(TokenKind::Question)?;
			Some(sign)
		} else if self.eat(TokenKind::Question)? {
			Some(Modifier::True)
		} else {
			None
		};
		let type_annotation = self.eat_then_parse_type(TokenKind::Colon)?;
		self.semicolon()?;
		self.expect(TokenKind::BraceR)?;
		Ok(self.ts(
			TsKind::MappedType {
				readonly,
				type_parameter,
				name_type,
				optional,
				type_annotation,
			},
			start,
		))
	}

	fn eat_sign(&mut self) -> Result<Option<Modifier>> {
		let sign = match self.tok.kind {
			TokenKind::Plus => Modifier::Plus,
			TokenKind::Minus => Modifier::Minus,
			_ => return Ok(None),
		};
		self.next()?;
		Ok(Some(sign))
	}

	fn parse_type_literal(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let members = self.parse_object_type_members()?;
		let members = self.list_of(&members);
		Ok(self.ts(TsKind::TypeLiteral { members }, start))
	}

	fn parse_tuple_type(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.expect(TokenKind::BracketL)?;
		let element_types = self.parse_delimited_list(ListKind::TupleElements, |p| p.parse_tuple_element_type())?;
		self.expect(TokenKind::BracketR)?;
		let mut seen_optional = false;
		for &element in &element_types {
			let kind = self.ts_kind(element);
			let optional = matches!(
				kind,
				Some(TsKind::OptionalType { .. } | TsKind::NamedTupleMember { optional: true, .. })
			);
			if seen_optional && !optional && !matches!(kind, Some(TsKind::RestType { .. })) {
				return self.error(
					self.start_of(element),
					"A required element cannot follow an optional element.",
				);
			}
			seen_optional |= optional;
		}
		let element_types = self.list_of(&element_types);
		Ok(self.ts(TsKind::TupleType { element_types }, start))
	}

	fn parse_tuple_element_type(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let rest = self.eat(TokenKind::Ellipsis)?;
		let mut ty = self.parse_type()?;
		let optional = self.eat(TokenKind::Question)?;
		if self.eat(TokenKind::Colon)? {
			let label = match self.ts_kind(ty) {
				Some(TsKind::TypeReference {
					type_name,
					type_arguments: None,
				}) if matches!(self.kind(type_name), NodeKind::Identifier { .. }) => type_name,
				_ => {
					return self.error(
						self.start_of(ty),
						"Tuple members must be labeled with a simple identifier.",
					);
				}
			};
			let element_type = self.parse_type()?;
			let label_start = self.start_of(ty);
			ty = self.ts(
				TsKind::NamedTupleMember {
					label,
					optional,
					element_type,
				},
				label_start,
			);
		} else if optional {
			let type_start = self.start_of(ty);
			ty = self.ts(TsKind::OptionalType { type_annotation: ty }, type_start);
		}
		if rest {
			ty = self.ts(TsKind::RestType { type_annotation: ty }, start);
		}
		Ok(ty)
	}

	fn parse_parenthesized_type(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.expect(TokenKind::ParenL)?;
		let type_annotation = self.parse_type()?;
		self.expect(TokenKind::ParenR)?;
		Ok(self.ts(TsKind::ParenthesizedType { type_annotation }, start))
	}

	// Type parameters and arguments

	fn parse_type_parameter_name(&mut self) -> Result<crate::interner::StrId> {
		let id = self.parse_ident(false)?;
		let NodeKind::Identifier { name } = self.kind(id) else {
			unreachable!()
		};
		Ok(name)
	}

	fn parse_type_parameter(&mut self, modifiers: TypeParameterModifiers) -> Result<NodeId> {
		let start = self.tok.start;
		let (allowed, disallowed): (&[&str], &[&str]) = match modifiers {
			TypeParameterModifiers::None => (&[], IN_OUT),
			TypeParameterModifiers::InOut => (IN_OUT, ACCESSIBILITY_AND_CLASS),
			TypeParameterModifiers::Const => (&["const"], IN_OUT),
		};
		let error = match modifiers {
			TypeParameterModifiers::InOut => "'{}' modifier cannot appear on a type parameter.",
			_ => "'{}' modifier can only appear on a type parameter of a class, interface or type alias.",
		};
		let parsed = self.parse_modifiers(allowed, disallowed, false, error)?;
		let name = self.parse_type_parameter_name()?;
		let constraint = self.eat_keyword_then_parse_type(Keyword::Extends)?;
		let default = self.eat_then_parse_type(TokenKind::Eq)?;
		Ok(self.ts(
			TsKind::TypeParameter {
				name,
				constraint,
				default,
				is_in: parsed.is_in,
				is_out: parsed.is_out,
				is_const: parsed.is_const,
			},
			start,
		))
	}

	pub(super) fn parse_type_parameters(&mut self, modifiers: TypeParameterModifiers) -> Result<NodeId> {
		let start = self.tok.start;
		if !self.is(TokenKind::Lt) {
			return self.unexpected();
		}
		self.next()?;
		let params = self.parse_delimited_list(ListKind::TypeParametersOrArguments, |p| {
			p.parse_type_parameter(modifiers)
		})?;
		// Unlike type arguments, checked after the `>`: the error position follows the plugin.
		self.expect(TokenKind::Gt)?;
		if params.is_empty() {
			return self.error(self.tok.start, "Type parameter list cannot be empty.");
		}
		let params = self.list_of(&params);
		Ok(self.ts(TsKind::TypeParameterDeclaration { params }, start))
	}

	pub(super) fn try_parse_type_parameters(&mut self, modifiers: TypeParameterModifiers) -> Result<Option<NodeId>> {
		if self.is(TokenKind::Lt) {
			self.parse_type_parameters(modifiers).map(Some)
		} else {
			Ok(None)
		}
	}

	pub(super) fn parse_type_arguments(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let params = self.in_type(|p| {
			p.expect(TokenKind::Lt)?;
			p.parse_delimited_list(ListKind::TypeParametersOrArguments, |p| p.parse_type())
		})?;
		if params.is_empty() {
			return self.error(self.tok.start, "Type argument list cannot be empty.");
		}
		self.expect(TokenKind::Gt)?;
		let params = self.list_of(&params);
		Ok(self.ts(TsKind::TypeParameterInstantiation { params }, start))
	}

	/// Type arguments where the `<` was read as an expression token, so `<<` needs splitting.
	pub(super) fn parse_type_arguments_in_expression(&mut self) -> Result<Option<NodeId>> {
		if self.is(TokenKind::LtLt) {
			self.split_lt();
		}
		if !self.is(TokenKind::Lt) {
			return Ok(None);
		}
		self.parse_type_arguments().map(Some)
	}

	pub(super) fn parse_heritage_clause(&mut self, token: &str) -> Result<Vec<NodeId>> {
		let start = self.tok.start;
		let list = self.parse_delimited_list(ListKind::HeritageClause, |p| {
			let start = p.tok.start;
			let expression = p.parse_entity_name(true)?;
			let type_arguments = if p.is(TokenKind::Lt) {
				Some(p.parse_type_arguments()?)
			} else {
				None
			};
			Ok(p.ts(
				TsKind::ExpressionWithTypeArguments {
					expression,
					type_arguments,
				},
				start,
			))
		})?;
		if list.is_empty() {
			return self.error(start, format!("'{token}' list cannot be empty."));
		}
		Ok(list)
	}

	// Type members

	pub(super) fn parse_object_type_members(&mut self) -> Result<Vec<NodeId>> {
		self.expect(TokenKind::BraceL)?;
		let members = self.parse_list(ListKind::TypeMembers, |p| p.parse_type_member())?;
		self.expect(TokenKind::BraceR)?;
		Ok(members)
	}

	pub(super) fn parse_type_member_semicolon(&mut self) -> Result<()> {
		if !self.eat(TokenKind::Comma)? && !self.is_line_terminator()? {
			self.expect(TokenKind::Semi)?;
		}
		Ok(())
	}

	pub(super) fn parse_type_member(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		if self.is(TokenKind::ParenL) || self.is(TokenKind::Lt) {
			return self.parse_signature_member(start, false);
		}
		if self.is_keyword(Keyword::New) {
			self.next()?;
			if self.is(TokenKind::ParenL) || self.is(TokenKind::Lt) {
				return self.parse_signature_member(start, true);
			}
			let name = self.intern("new");
			let key = self.add(NodeKind::Identifier { name }, start);
			return self.parse_property_or_method_signature(start, key, None, None, false);
		}
		let modifiers = self.parse_modifiers(
			&["readonly"],
			&[
				"declare",
				"abstract",
				"private",
				"protected",
				"public",
				"static",
				"override",
			],
			false,
			"'{}' modifier cannot appear on a type member.",
		)?;
		if let Some(signature) = self.try_parse_index_signature(start)? {
			self.extras_mut(signature).readonly = modifiers.extras.readonly;
			return Ok(signature);
		}
		let (mut key, mut computed) = self.parse_property_name()?;
		let mut kind = None;
		if !computed && (self.ident_is(key, "get") || self.ident_is(key, "set")) && self.token_can_follow_modifier() {
			kind = Some(if self.ident_is(key, "get") {
				SignatureKind::Get
			} else {
				SignatureKind::Set
			});
			(key, computed) = self.parse_property_name()?;
		}
		self.parse_property_or_method_signature(start, key, Some(computed), kind, modifiers.extras.readonly)
	}

	fn parse_signature_member(&mut self, start: u32, construct: bool) -> Result<NodeId> {
		let (type_parameters, parameters, type_annotation) = self.fill_signature(TokenKind::Colon)?;
		self.parse_type_member_semicolon()?;
		let kind = if construct {
			TsKind::ConstructSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			}
		} else {
			TsKind::CallSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			}
		};
		Ok(self.ts(kind, start))
	}

	fn parse_property_or_method_signature(
		&mut self,
		start: u32,
		key: NodeId,
		computed: Option<bool>,
		kind: Option<SignatureKind>,
		readonly: bool,
	) -> Result<NodeId> {
		let optional = self.eat(TokenKind::Question)?;
		if self.is(TokenKind::ParenL) || self.is(TokenKind::Lt) {
			if readonly {
				return self.error(
					start,
					"'readonly' modifier can only appear on a property declaration or index signature.",
				);
			}
			if kind.is_some() && self.is(TokenKind::Lt) {
				return self.error(self.tok.start, "An accessor cannot have type parameters.");
			}
			let (type_parameters, parameters, type_annotation) = self.fill_signature(TokenKind::Colon)?;
			self.parse_type_member_semicolon()?;
			let count = self.ast.list(parameters).len();
			match kind {
				Some(SignatureKind::Get) if count > 0 => {
					return self.error(self.tok.start, "A 'get' accesor must not have any formal parameters.");
				}
				Some(SignatureKind::Set) => {
					if count != 1 {
						return self.error(self.tok.start, "A 'get' accesor must not have any formal parameters.");
					}
					if let Some(type_annotation) = type_annotation {
						return self.error(
							self.start_of(type_annotation),
							"A 'set' accessor cannot have a return type annotation.",
						);
					}
				}
				_ => {}
			}
			return Ok(self.ts(
				TsKind::MethodSignature {
					key,
					computed: computed.unwrap_or(false),
					optional,
					kind: kind.unwrap_or(SignatureKind::Method),
					type_parameters,
					parameters,
					type_annotation,
				},
				start,
			));
		}
		let type_annotation = self.try_parse_type_annotation()?;
		self.parse_type_member_semicolon()?;
		Ok(self.ts(
			TsKind::PropertySignature {
				key,
				computed,
				optional,
				readonly,
				kind,
				type_annotation,
			},
			start,
		))
	}

	pub(super) fn try_parse_index_signature(&mut self, start: u32) -> Result<Option<NodeId>> {
		if !self.is(TokenKind::BracketL) || !self.lookahead(|p| p.is_unambiguously_index_signature())? {
			return Ok(None);
		}
		self.expect(TokenKind::BracketL)?;
		let id = self.parse_ident(false)?;
		let annotation = self.parse_type_annotation(true, None)?;
		self.extras_mut(id).type_annotation = Some(annotation);
		self.ast.node_mut(id).end = self.prev_end;
		self.expect(TokenKind::BracketR)?;
		let parameters = self.list_of(&[id]);
		let type_annotation = self.try_parse_type_annotation()?;
		self.parse_type_member_semicolon()?;
		Ok(Some(self.ts(
			TsKind::IndexSignature {
				parameters,
				type_annotation,
			},
			start,
		)))
	}

	fn is_unambiguously_index_signature(&mut self) -> Result<bool> {
		self.next()?;
		if self.is_ident() {
			self.next()?;
			return Ok(self.is(TokenKind::Colon));
		}
		Ok(false)
	}

	// Modifiers

	pub(super) fn token_can_follow_modifier(&self) -> bool {
		(self.is(TokenKind::BracketL)
			|| self.is(TokenKind::BraceL)
			|| self.is(TokenKind::Star)
			|| self.is(TokenKind::Ellipsis)
			|| matches!(self.tok.kind, TokenKind::PrivateName(_))
			|| self.is_literal_property_name())
			&& !self.tok.newline_before
	}

	/// Consumes the current identifier when it is one of the modifiers and a member name may
	/// follow it.
	fn parse_modifier(
		&mut self,
		modifiers: &[&str],
		stop_on_static_block: bool,
	) -> Result<Option<(&'static str, u32)>> {
		let word = match self.tok.kind {
			TokenKind::Ident(name) if !self.tok.escaped => self.str(name),
			TokenKind::Keyword(Keyword::In) => "in",
			TokenKind::Keyword(Keyword::Const) => "const",
			_ => return Ok(None),
		};
		let Some(&modifier) = MODIFIERS.iter().find(|m| **m == word) else {
			return Ok(None);
		};
		if !modifiers.contains(&modifier) {
			return Ok(None);
		}
		if stop_on_static_block && modifier == "static" && self.peek_char().0 == Some('{') {
			return Ok(None);
		}
		let start = self.tok.start;
		let snapshot = self.token_snapshot();
		self.next_liberal()?;
		if self.token_can_follow_modifier() {
			return Ok(Some((modifier, start)));
		}
		self.restore_tokens(snapshot);
		Ok(None)
	}

	pub(super) fn parse_modifiers(
		&mut self,
		allowed: &[&str],
		disallowed: &[&str],
		stop_on_static_block: bool,
		disallowed_error: &str,
	) -> Result<Modifiers> {
		let mut modifiers = Modifiers::default();
		let all: Vec<&str> = allowed.iter().chain(disallowed).copied().collect();
		while let Some((modifier, start)) = self.parse_modifier(&all, stop_on_static_block)? {
			self.check_modifier(&modifiers, modifier, start)?;
			modifiers.set(modifier);
			if disallowed.contains(&modifier) {
				return self.error(self.tok.start, disallowed_error.replace("{}", modifier));
			}
		}
		Ok(modifiers)
	}

	/// Duplicate, misordered and conflicting modifiers.
	fn check_modifier(&self, seen: &Modifiers, modifier: &str, start: u32) -> Result<()> {
		let order = |before: &str, after: &str| -> Result<()> {
			if modifier == before && seen.has(after) {
				return self.error(start, format!("'{before}' modifier must precede '{after}' modifier."));
			}
			Ok(())
		};
		let conflict = |a: &str, b: &str| -> Result<()> {
			if (seen.has(a) && modifier == b) || (seen.has(b) && modifier == a) {
				return self.error(start, format!("'{a}' modifier cannot be used with '{b}' modifier."));
			}
			Ok(())
		};
		match modifier {
			"public" | "private" | "protected" => {
				if seen.extras.accessibility.is_some() {
					return self.error(self.tok.start, "Accessibility modifier already seen.");
				}
				for after in ["override", "static", "readonly", "accessor"] {
					order(modifier, after)?;
				}
			}
			"in" | "out" => {
				if seen.has(modifier) {
					return self.error(self.tok.start, format!("Duplicate modifier: '{modifier}'."));
				}
				order("in", "out")?;
			}
			"accessor" => {
				if seen.has(modifier) {
					return self.error(self.tok.start, format!("Duplicate modifier: '{modifier}'."));
				}
				for other in ["readonly", "static", "override"] {
					conflict("accessor", other)?;
				}
			}
			"const" => {
				if seen.has(modifier) {
					return self.error(self.tok.start, format!("Duplicate modifier: '{modifier}'."));
				}
			}
			_ => {
				if seen.has(modifier) {
					return self.error(self.tok.start, format!("Duplicate modifier: '{modifier}'."));
				}
				order("static", "readonly")?;
				order("static", "override")?;
				order("override", "readonly")?;
				order("abstract", "override")?;
				conflict("declare", "override")?;
				conflict("static", "abstract")?;
			}
		}
		Ok(())
	}
}

const MODIFIERS: &[&str] = &[
	"declare",
	"private",
	"public",
	"protected",
	"accessor",
	"override",
	"abstract",
	"readonly",
	"static",
	"in",
	"out",
	"const",
];
