use super::pattern::Binding;
use super::scope::{SCOPE_ARROW, SCOPE_DIRECT_SUPER, SCOPE_SUPER, function_flags};
use super::{DestructuringErrors, Errors, Extension, FunctionKind, Parser, Result};
use crate::ast::{
	AssignmentOperator, BinaryOperator, Function, List, LogicalOperator, NodeId, NodeKind, PropertyKind, UnaryOperator,
	UpdateOperator,
};
use crate::lexer::token::{Keyword, TokenKind};

/// Whether the expression is the init of a `for` statement, where `in` is not an operator.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForInit {
	No,
	Yes,
	Await,
}

impl ForInit {
	fn no_in(self) -> bool {
		self != ForInit::No
	}
}

fn binary_precedence(kind: TokenKind) -> Option<(u8, BinaryOperator)> {
	use BinaryOperator as B;
	use TokenKind as T;
	Some(match kind {
		T::Pipe => (3, B::BitOr),
		T::Caret => (4, B::BitXor),
		T::Amp => (5, B::BitAnd),
		T::EqEq => (6, B::Eq),
		T::BangEq => (6, B::NotEq),
		T::EqEqEq => (6, B::StrictEq),
		T::BangEqEq => (6, B::StrictNotEq),
		T::Lt => (7, B::Lt),
		T::LtEq => (7, B::LtEq),
		T::Gt => (7, B::Gt),
		T::GtEq => (7, B::GtEq),
		T::Keyword(Keyword::In) => (7, B::In),
		T::Keyword(Keyword::Instanceof) => (7, B::Instanceof),
		T::LtLt => (8, B::Shl),
		T::GtGt => (8, B::Shr),
		T::GtGtGt => (8, B::UShr),
		T::Plus => (9, B::Add),
		T::Minus => (9, B::Sub),
		T::Star => (10, B::Mul),
		T::Slash => (10, B::Div),
		T::Percent => (10, B::Mod),
		_ => return None,
	})
}

fn assignment_operator(kind: TokenKind) -> Option<AssignmentOperator> {
	use AssignmentOperator as A;
	use TokenKind as T;
	Some(match kind {
		T::Eq => A::Assign,
		T::PlusEq => A::Add,
		T::MinusEq => A::Sub,
		T::StarEq => A::Mul,
		T::SlashEq => A::Div,
		T::PercentEq => A::Mod,
		T::StarStarEq => A::Exp,
		T::LtLtEq => A::Shl,
		T::GtGtEq => A::Shr,
		T::GtGtGtEq => A::UShr,
		T::PipeEq => A::BitOr,
		T::CaretEq => A::BitXor,
		T::AmpEq => A::BitAnd,
		T::PipePipeEq => A::Or,
		T::AmpAmpEq => A::And,
		T::QuestionQuestionEq => A::Nullish,
		_ => return None,
	})
}

fn unary_operator(kind: TokenKind) -> Option<UnaryOperator> {
	use TokenKind as T;
	Some(match kind {
		T::Minus => UnaryOperator::Minus,
		T::Plus => UnaryOperator::Plus,
		T::Bang => UnaryOperator::Not,
		T::Tilde => UnaryOperator::BitNot,
		T::Keyword(Keyword::Typeof) => UnaryOperator::Typeof,
		T::Keyword(Keyword::Void) => UnaryOperator::Void,
		T::Keyword(Keyword::Delete) => UnaryOperator::Delete,
		_ => return None,
	})
}

pub(crate) fn starts_expression(kind: TokenKind) -> bool {
	use TokenKind as T;
	match kind {
		T::Ident(_) | T::PrivateName(_) | T::Number(_) | T::BigInt | T::String(_) | T::RegExp { .. } => true,
		T::ParenL | T::BracketL | T::BraceL | T::Backquote | T::Slash | T::SlashEq => true,
		T::Bang | T::Tilde | T::Plus | T::Minus | T::PlusPlus | T::MinusMinus => true,
		T::Keyword(k) => {
			matches!(
				k,
				Keyword::This
					| Keyword::Super
					| Keyword::Function
					| Keyword::Class
					| Keyword::New | Keyword::Typeof
					| Keyword::Void | Keyword::Delete
					| Keyword::Import
					| Keyword::Null | Keyword::True
					| Keyword::False
			)
		}
		_ => false,
	}
}

impl<E: Extension> Parser<'_, E> {
	pub(crate) fn parse_expression(&mut self, for_init: bool, errors: &mut Errors) -> Result<NodeId> {
		let for_init = if for_init { ForInit::Yes } else { ForInit::No };
		self.parse_sequence(for_init, errors)
	}

	pub(crate) fn parse_sequence(&mut self, for_init: ForInit, errors: &mut Errors) -> Result<NodeId> {
		let start = self.tok.start;
		let expr = self.parse_maybe_assign(for_init, errors)?;
		if self.is(TokenKind::Comma) {
			let mut expressions = vec![expr];
			while self.eat(TokenKind::Comma)? {
				expressions.push(self.parse_maybe_assign(for_init, errors)?);
			}
			let expressions = self.list_of(&expressions);
			return Ok(self.add(NodeKind::SequenceExpression { expressions }, start));
		}
		Ok(expr)
	}

	pub(crate) fn parse_maybe_assign(&mut self, for_init: ForInit, errors: &mut Errors) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_maybe_assign_inner(for_init, errors, false);
		self.leave();
		result
	}

	/// An assignment expression that is an item of a parenthesized list, which an extension may
	/// read differently.
	pub(crate) fn parse_paren_item(&mut self, for_init: ForInit, errors: &mut Errors) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_maybe_assign_inner(for_init, errors, true);
		self.leave();
		result
	}

	fn parse_maybe_assign_inner(&mut self, for_init: ForInit, errors: &mut Errors, item: bool) -> Result<NodeId> {
		if let Some(expr) = E::maybe_assign(self, for_init, errors)? {
			return Ok(expr);
		}
		if self.is_contextual("yield") && self.in_generator() {
			return self.parse_yield(for_init);
		}
		let own_errors = errors.is_none();
		let (old_paren_assign, old_trailing_comma, old_double_proto) = match errors {
			Some(e) => {
				let old = (e.parenthesized_assign, e.trailing_comma, e.double_proto);
				e.parenthesized_assign = None;
				e.trailing_comma = None;
				old
			}
			None => {
				*errors = Some(DestructuringErrors::default());
				(None, None, None)
			}
		};
		let start = self.tok.start;
		if self.is(TokenKind::ParenL) || matches!(self.tok.kind, TokenKind::Ident(_)) {
			self.potential_arrow_at = start;
			self.potential_arrow_in_for_await = for_init == ForInit::Await;
		}
		let mut left = self.parse_maybe_conditional(for_init, errors)?;
		if item {
			left = E::paren_item(self, left)?;
		}
		if let Some(operator) = assignment_operator(self.tok.kind) {
			let is_eq = operator == AssignmentOperator::Assign;
			if is_eq {
				left = self.make_pattern(left, false, errors)?;
			}
			let e = errors.as_mut().unwrap();
			if !own_errors {
				e.parenthesized_assign = None;
				e.trailing_comma = None;
				e.double_proto = None;
			}
			if e.shorthand_assign.is_some_and(|p| p >= self.start_of(left)) {
				e.shorthand_assign = None;
			}
			if is_eq {
				self.check_lval_pattern(left, Binding::None, &mut None)?;
			} else {
				self.check_lval_simple(left, Binding::None, &mut None)?;
			}
			self.next()?;
			let right = self.parse_maybe_assign(for_init, &mut None)?;
			if old_double_proto.is_some() {
				errors.as_mut().unwrap().double_proto = old_double_proto;
			}
			if own_errors {
				*errors = None;
			}
			return Ok(self.add(NodeKind::AssignmentExpression { operator, left, right }, start));
		}
		if own_errors {
			self.check_expression_errors(errors, true)?;
			*errors = None;
		} else {
			let e = errors.as_mut().unwrap();
			if old_paren_assign.is_some() {
				e.parenthesized_assign = old_paren_assign;
			}
			if old_trailing_comma.is_some() {
				e.trailing_comma = old_trailing_comma;
			}
		}
		Ok(left)
	}

	fn parse_maybe_conditional(&mut self, for_init: ForInit, errors: &mut Errors) -> Result<NodeId> {
		let start = self.tok.start;
		let expr = self.parse_expr_ops(for_init, errors)?;
		if self.check_expression_errors(errors, false)? {
			return Ok(expr);
		}
		if self.is_arrow_at(expr, start) {
			return Ok(expr);
		}
		if self.is(TokenKind::Question) {
			if let Some(expr) = E::conditional(self, expr, start, for_init)? {
				return Ok(expr);
			}
			return self.parse_conditional(expr, start, for_init);
		}
		Ok(expr)
	}

	/// The `? consequent : alternate` after a test.
	pub(crate) fn parse_conditional(&mut self, test: NodeId, start: u32, for_init: ForInit) -> Result<NodeId> {
		self.next()?;
		let consequent = self.parse_maybe_assign(ForInit::No, &mut None)?;
		self.expect(TokenKind::Colon)?;
		let alternate = self.parse_maybe_assign(for_init, &mut None)?;
		Ok(self.add(
			NodeKind::ConditionalExpression {
				test,
				consequent,
				alternate,
			},
			start,
		))
	}

	fn parse_expr_ops(&mut self, for_init: ForInit, errors: &mut Errors) -> Result<NodeId> {
		let start = self.tok.start;
		let expr = self.parse_maybe_unary(errors, false, false, for_init)?;
		if self.check_expression_errors(errors, false)? {
			return Ok(expr);
		}
		if self.is_arrow_at(expr, start) {
			return Ok(expr);
		}
		self.parse_expr_op(expr, start, -1, for_init)
	}

	fn is_arrow_at(&self, expr: NodeId, start: u32) -> bool {
		self.start_of(expr) == start && matches!(self.kind(expr), NodeKind::ArrowFunctionExpression { .. })
	}

	fn parse_expr_op(&mut self, mut left: NodeId, left_start: u32, min_prec: i8, for_init: ForInit) -> Result<NodeId> {
		loop {
			if let Some(expr) = E::expr_op(self, left, left_start, min_prec)? {
				left = expr;
				continue;
			}
			let (prec, op): (i8, Op) = match self.tok.kind {
				TokenKind::QuestionQuestion => (1, Op::Coalesce),
				TokenKind::PipePipe => (1, Op::Logical(LogicalOperator::Or)),
				TokenKind::AmpAmp => (2, Op::Logical(LogicalOperator::And)),
				kind => match binary_precedence(kind) {
					Some((prec, op)) => (prec as i8, Op::Binary(op)),
					None => return Ok(left),
				},
			};
			if for_init.no_in() && self.is_keyword(Keyword::In) {
				return Ok(left);
			}
			if prec <= min_prec {
				return Ok(left);
			}
			let logical = matches!(op, Op::Logical(_));
			let coalesce = matches!(op, Op::Coalesce);
			let prec = if coalesce { 2 } else { prec };
			self.next()?;
			let right_start = self.tok.start;
			let unary = self.parse_maybe_unary(&mut None, false, false, for_init)?;
			self.enter()?;
			let right = self.parse_expr_op(unary, right_start, if coalesce { prec - 1 } else { prec }, for_init);
			self.leave();
			let right = right?;
			left = self.build_binary(left_start, left, right, op)?;
			if (logical && self.is(TokenKind::QuestionQuestion))
				|| (coalesce && (self.is(TokenKind::PipePipe) || self.is(TokenKind::AmpAmp)))
			{
				return self.error(
					self.tok.start,
					"Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses",
				);
			}
		}
	}

	fn build_binary(&mut self, start: u32, left: NodeId, right: NodeId, op: Op) -> Result<NodeId> {
		if matches!(self.kind(right), NodeKind::PrivateIdentifier { .. }) {
			return self.error(
				self.start_of(right),
				"Private identifier can only be left side of binary expression",
			);
		}
		let kind = match op {
			Op::Binary(operator) => NodeKind::BinaryExpression { operator, left, right },
			Op::Logical(operator) => NodeKind::LogicalExpression { operator, left, right },
			Op::Coalesce => NodeKind::LogicalExpression {
				operator: LogicalOperator::Nullish,
				left,
				right,
			},
		};
		Ok(self.add(kind, start))
	}

	pub(crate) fn parse_maybe_unary(
		&mut self,
		errors: &mut Errors,
		saw_unary: bool,
		inc_dec: bool,
		for_init: ForInit,
	) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_maybe_unary_inner(errors, saw_unary, inc_dec, for_init);
		self.leave();
		result
	}

	fn parse_maybe_unary_inner(
		&mut self,
		errors: &mut Errors,
		mut saw_unary: bool,
		inc_dec: bool,
		for_init: ForInit,
	) -> Result<NodeId> {
		if let Some(expr) = E::unary(self, for_init)? {
			return Ok(expr);
		}
		let start = self.tok.start;
		let expr;
		if self.is_contextual("await") && self.can_await() {
			expr = self.parse_await(for_init)?;
			saw_unary = true;
		} else if let Some(operator) = unary_operator(self.tok.kind) {
			self.next()?;
			let argument = self.parse_maybe_unary(&mut None, true, false, for_init)?;
			self.check_expression_errors(errors, true)?;
			if self.strict && operator == UnaryOperator::Delete && self.is_local_variable_access(argument) {
				return self.error(start, "Deleting local variable in strict mode");
			}
			if operator == UnaryOperator::Delete && self.is_private_field_access(argument) {
				return self.error(start, "Private fields can not be deleted");
			}
			saw_unary = true;
			expr = self.add(NodeKind::UnaryExpression { operator, argument }, start);
		} else if self.is(TokenKind::PlusPlus) || self.is(TokenKind::MinusMinus) {
			let operator = if self.is(TokenKind::PlusPlus) {
				UpdateOperator::Increment
			} else {
				UpdateOperator::Decrement
			};
			self.next()?;
			let argument = self.parse_maybe_unary(&mut None, true, true, for_init)?;
			self.check_expression_errors(errors, true)?;
			self.check_lval_simple(argument, Binding::None, &mut None)?;
			expr = self.add(
				NodeKind::UpdateExpression {
					operator,
					prefix: true,
					argument,
				},
				start,
			);
		} else if !saw_unary && matches!(self.tok.kind, TokenKind::PrivateName(_)) {
			if for_init.no_in() || self.private_names.is_empty() {
				return self.unexpected();
			}
			expr = self.parse_private_ident()?;
			if !self.is_keyword(Keyword::In) {
				return self.unexpected();
			}
		} else {
			let mut e = self.parse_expr_subscripts(errors, for_init)?;
			if self.check_expression_errors(errors, false)? {
				return Ok(e);
			}
			while (self.is(TokenKind::PlusPlus) || self.is(TokenKind::MinusMinus)) && !self.can_insert_semicolon() {
				let operator = if self.is(TokenKind::PlusPlus) {
					UpdateOperator::Increment
				} else {
					UpdateOperator::Decrement
				};
				self.check_lval_simple(e, Binding::None, &mut None)?;
				self.next()?;
				e = self.add(
					NodeKind::UpdateExpression {
						operator,
						prefix: false,
						argument: e,
					},
					start,
				);
			}
			expr = e;
		}

		if !inc_dec && self.is(TokenKind::StarStar) && !self.is_arrow_at(expr, start) {
			if saw_unary {
				return self.unexpected();
			}
			self.next()?;
			let right = self.parse_maybe_unary(&mut None, false, false, for_init)?;
			return self.build_binary(start, expr, right, Op::Binary(BinaryOperator::Exp));
		}
		Ok(expr)
	}

	fn is_local_variable_access(&self, id: NodeId) -> bool {
		match self.kind(id) {
			NodeKind::Identifier { .. } => true,
			NodeKind::ParenthesizedExpression { expression } => self.is_local_variable_access(expression),
			_ => false,
		}
	}

	fn is_private_field_access(&self, id: NodeId) -> bool {
		match self.kind(id) {
			NodeKind::MemberExpression { property, .. } => {
				matches!(self.kind(property), NodeKind::PrivateIdentifier { .. })
			}
			NodeKind::ChainExpression { expression } => self.is_private_field_access(expression),
			NodeKind::ParenthesizedExpression { expression } => self.is_private_field_access(expression),
			_ => false,
		}
	}

	pub(crate) fn parse_expr_subscripts(&mut self, errors: &mut Errors, for_init: ForInit) -> Result<NodeId> {
		let start = self.tok.start;
		let expr = self.parse_expr_atom(errors, for_init, false)?;
		if matches!(self.kind(expr), NodeKind::ArrowFunctionExpression { .. })
			&& self.source().as_bytes().get(self.prev_end as usize - 1) != Some(&b')')
		{
			return Ok(expr);
		}
		let result = self.parse_subscripts(expr, start, false, for_init)?;
		if let Some(e) = errors
			&& matches!(self.kind(result), NodeKind::MemberExpression { .. })
		{
			let rs = self.start_of(result);
			if e.parenthesized_assign.is_some_and(|p| p >= rs) {
				e.parenthesized_assign = None;
			}
			if e.parenthesized_bind.is_some_and(|p| p >= rs) {
				e.parenthesized_bind = None;
			}
			if e.trailing_comma.is_some_and(|p| p >= rs) {
				e.trailing_comma = None;
			}
		}
		Ok(result)
	}

	fn parse_subscripts(&mut self, mut base: NodeId, start: u32, no_calls: bool, for_init: ForInit) -> Result<NodeId> {
		let maybe_async_arrow = match self.kind(base) {
			NodeKind::Identifier { name } => {
				self.str(name) == "async"
					&& self.prev_end == self.end_of(base)
					&& !self.can_insert_semicolon()
					&& self.end_of(base) - self.start_of(base) == 5
					&& self.potential_arrow_at == self.start_of(base)
			}
			_ => false,
		};
		let mut optional_chained = false;
		loop {
			let (element, optional) =
				self.parse_subscript(base, start, no_calls, maybe_async_arrow, optional_chained, for_init)?;
			if optional {
				optional_chained = true;
			}
			if element == base || matches!(self.kind(element), NodeKind::ArrowFunctionExpression { .. }) {
				if optional_chained {
					return Ok(self.add(NodeKind::ChainExpression { expression: element }, start));
				}
				return Ok(element);
			}
			base = element;
		}
	}

	fn parse_subscript(
		&mut self,
		base: NodeId,
		start: u32,
		no_calls: bool,
		maybe_async_arrow: bool,
		optional_chained: bool,
		for_init: ForInit,
	) -> Result<(NodeId, bool)> {
		if let Some(result) = E::subscript(
			self,
			base,
			start,
			no_calls,
			maybe_async_arrow,
			optional_chained,
			for_init,
		)? {
			return Ok(result);
		}
		let optional = self.eat(TokenKind::QuestionDot)?;
		if no_calls && optional {
			return self.error(
				self.prev_end - 2,
				"Optional chaining cannot appear in the callee of new expressions",
			);
		}
		let computed = self.eat(TokenKind::BracketL)?;
		if computed
			|| (optional && !self.is(TokenKind::ParenL) && !self.is(TokenKind::Backquote))
			|| self.eat(TokenKind::Dot)?
		{
			let property = if computed {
				let property = self.parse_expression(false, &mut None)?;
				self.expect(TokenKind::BracketR)?;
				property
			} else if matches!(self.tok.kind, TokenKind::PrivateName(_)) && !matches!(self.kind(base), NodeKind::Super)
			{
				self.parse_private_ident()?
			} else {
				self.parse_ident(true)?
			};
			let node = self.add(
				NodeKind::MemberExpression {
					object: base,
					property,
					computed,
					optional,
				},
				start,
			);
			return Ok((node, optional));
		}
		if !no_calls && self.eat(TokenKind::ParenL)? {
			let mut errors = Some(DestructuringErrors::default());
			let (old_yield, old_await, old_await_ident) = (self.yield_pos, self.await_pos, self.await_ident_pos);
			self.yield_pos = 0;
			self.await_pos = 0;
			self.await_ident_pos = 0;
			let args = self.parse_expr_list(TokenKind::ParenR, true, false, &mut errors)?;
			if maybe_async_arrow && !optional && E::should_parse_async_arrow(self)? {
				self.check_pattern_errors(&errors, false)?;
				self.check_yield_await_in_default_params()?;
				if self.await_ident_pos > 0 {
					return self.error(
						self.await_ident_pos,
						"Cannot use 'await' as identifier inside an async function",
					);
				}
				self.yield_pos = old_yield;
				self.await_pos = old_await;
				self.await_ident_pos = old_await_ident;
				let arrow = self.parse_arrow_expression(start, args, true, for_init)?;
				return Ok((arrow, false));
			}
			self.check_expression_errors(&errors, true)?;
			if old_yield != 0 {
				self.yield_pos = old_yield;
			}
			if old_await != 0 {
				self.await_pos = old_await;
			}
			if old_await_ident != 0 {
				self.await_ident_pos = old_await_ident;
			}
			let arguments = self.list(&args);
			let node = self.add(
				NodeKind::CallExpression {
					callee: base,
					arguments,
					optional,
				},
				start,
			);
			return Ok((node, optional));
		}
		if self.is(TokenKind::Backquote) {
			if optional || optional_chained {
				return self.error(
					self.tok.start,
					"Optional chaining cannot appear in the tag of tagged template expressions",
				);
			}
			let quasi = self.parse_template(true)?;
			let node = self.add(NodeKind::TaggedTemplateExpression { tag: base, quasi }, start);
			return Ok((node, false));
		}
		Ok((base, optional))
	}

	pub(crate) fn parse_expr_atom(&mut self, errors: &mut Errors, for_init: ForInit, for_new: bool) -> Result<NodeId> {
		if self.is(TokenKind::Slash) || self.is(TokenKind::SlashEq) {
			self.tok = self.lexer.read_regex(self.tok)?;
		}
		let start = self.tok.start;
		let can_be_arrow = self.potential_arrow_at == start;
		match self.tok.kind {
			TokenKind::Keyword(Keyword::Super) => {
				if !self.allow_super() {
					return self.error(start, "'super' keyword outside a method");
				}
				self.next()?;
				if self.is(TokenKind::ParenL) && !self.allow_direct_super() {
					return self.error(start, "super() call outside constructor of a subclass");
				}
				if !self.is(TokenKind::Dot) && !self.is(TokenKind::BracketL) && !self.is(TokenKind::ParenL) {
					return self.unexpected();
				}
				Ok(self.add(NodeKind::Super, start))
			}
			TokenKind::Keyword(Keyword::This) => {
				self.next()?;
				Ok(self.add(NodeKind::ThisExpression, start))
			}
			TokenKind::Ident(name) => {
				let escaped = self.tok.escaped;
				let id = self.parse_ident(false)?;
				let is_async = !escaped && self.str(name) == "async";
				if is_async && !self.can_insert_semicolon() && self.eat_keyword(Keyword::Function)? {
					return self.parse_function(start, 0, true, for_init);
				}
				if can_be_arrow && !self.can_insert_semicolon() {
					if self.eat(TokenKind::Arrow)? {
						return self.parse_arrow_expression(start, vec![Some(id)], false, for_init);
					}
					if is_async
						&& matches!(self.tok.kind, TokenKind::Ident(_))
						&& (!self.potential_arrow_in_for_await || !self.is_contextual("of"))
					{
						let param = self.parse_ident(false)?;
						if self.can_insert_semicolon() || !self.eat(TokenKind::Arrow)? {
							return self.unexpected();
						}
						return self.parse_arrow_expression(start, vec![Some(param)], true, for_init);
					}
				}
				Ok(id)
			}
			TokenKind::RegExp { pattern, flags } => {
				self.next()?;
				Ok(self.add(NodeKind::RegExpLiteral { pattern, flags }, start))
			}
			TokenKind::Number(value) => {
				self.next()?;
				Ok(self.add(NodeKind::NumberLiteral { value }, start))
			}
			TokenKind::BigInt => {
				self.next()?;
				Ok(self.add(NodeKind::BigIntLiteral, start))
			}
			TokenKind::String(value) => {
				self.next()?;
				Ok(self.add(NodeKind::StringLiteral { value }, start))
			}
			TokenKind::Keyword(Keyword::Null) => {
				self.next()?;
				Ok(self.add(NodeKind::NullLiteral, start))
			}
			TokenKind::Keyword(Keyword::True) => {
				self.next()?;
				Ok(self.add(NodeKind::BooleanLiteral { value: true }, start))
			}
			TokenKind::Keyword(Keyword::False) => {
				self.next()?;
				Ok(self.add(NodeKind::BooleanLiteral { value: false }, start))
			}
			TokenKind::ParenL => {
				let expr = self.parse_paren_and_distinguish_expression(can_be_arrow, for_init)?;
				if let Some(e) = errors {
					if e.parenthesized_assign.is_none() && !self.is_simple_assign_target(expr) {
						e.parenthesized_assign = Some(start);
					}
					if e.parenthesized_bind.is_none() {
						e.parenthesized_bind = Some(start);
					}
				}
				Ok(expr)
			}
			TokenKind::BracketL => {
				self.next()?;
				let elements = self.parse_expr_list(TokenKind::BracketR, true, true, errors)?;
				let elements = self.list(&elements);
				Ok(self.add(NodeKind::ArrayExpression { elements }, start))
			}
			TokenKind::BraceL => self.parse_obj(false, errors),
			TokenKind::Keyword(Keyword::Function) => {
				self.next()?;
				self.parse_function(start, 0, false, ForInit::No)
			}
			TokenKind::Keyword(Keyword::Class) => self.parse_class(super::class::ClassKind::Expression),
			TokenKind::Keyword(Keyword::New) => self.parse_new(),
			TokenKind::Backquote => self.parse_template(false),
			TokenKind::Keyword(Keyword::Import) => self.parse_expr_import(for_new),
			_ => self.unexpected(),
		}
	}

	fn parse_expr_import(&mut self, for_new: bool) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		if self.is(TokenKind::ParenL) && !for_new {
			return self.parse_dynamic_import(start);
		}
		if self.is(TokenKind::Dot) {
			let name = self.intern("import");
			let meta = self.add_with_end(NodeKind::Identifier { name }, start, start + 6);
			return self.parse_import_meta(start, meta);
		}
		self.unexpected()
	}

	fn parse_dynamic_import(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		let source = self.parse_maybe_assign(ForInit::No, &mut None)?;
		let mut options = None;
		if !self.eat(TokenKind::ParenR)? {
			self.expect(TokenKind::Comma)?;
			if !self.after_trailing_comma(TokenKind::ParenR, false)? {
				options = Some(self.parse_maybe_assign(ForInit::No, &mut None)?);
				if !self.eat(TokenKind::ParenR)? {
					self.expect(TokenKind::Comma)?;
					if !self.after_trailing_comma(TokenKind::ParenR, false)? {
						return self.unexpected();
					}
				}
			}
		}
		Ok(self.add(NodeKind::ImportExpression { source, options }, start))
	}

	fn parse_import_meta(&mut self, start: u32, meta: NodeId) -> Result<NodeId> {
		self.next()?;
		let escaped = self.tok.escaped;
		let property = self.parse_ident(true)?;
		if !self.ident_is(property, "meta") {
			return self.error(
				self.start_of(property),
				"The only valid meta property for import is 'import.meta'",
			);
		}
		if escaped {
			return self.error(start, "'import.meta' must not contain escaped characters");
		}
		if !self.options.module {
			return self.error(start, "Cannot use 'import.meta' outside a module");
		}
		Ok(self.add(NodeKind::MetaProperty { meta, property }, start))
	}

	pub(crate) fn ident_is(&self, id: NodeId, name: &str) -> bool {
		matches!(self.kind(id), NodeKind::Identifier { name: n } if self.str(n) == name)
	}

	fn parse_paren_and_distinguish_expression(&mut self, can_be_arrow: bool, for_init: ForInit) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		let (old_yield, old_await) = (self.yield_pos, self.await_pos);
		self.yield_pos = 0;
		self.await_pos = 0;
		let paren = self.parse_paren_items()?;

		if can_be_arrow && E::should_parse_arrow(self, &paren.items)? && self.eat(TokenKind::Arrow)? {
			self.check_pattern_errors(&paren.errors, false)?;
			self.check_yield_await_in_default_params()?;
			self.yield_pos = old_yield;
			self.await_pos = old_await;
			return self.parse_arrow_expression(start, paren.items, false, for_init);
		}

		if paren.items.is_empty() || paren.last_is_comma {
			return self.unexpected_at(self.prev_end - 1);
		}
		if let Some(pos) = paren.spread_start {
			return self.unexpected_at(pos);
		}
		self.check_expression_errors(&paren.errors, true)?;
		if old_yield != 0 {
			self.yield_pos = old_yield;
		}
		if old_await != 0 {
			self.await_pos = old_await;
		}

		let value = if paren.items.len() > 1 {
			let expressions = self.list(&paren.items);
			self.add_with_end(
				NodeKind::SequenceExpression { expressions },
				paren.inner_start,
				paren.inner_end,
			)
		} else {
			paren.items[0].unwrap()
		};
		if self.options.preserve_parens {
			return Ok(self.add(NodeKind::ParenthesizedExpression { expression: value }, start));
		}
		Ok(value)
	}

	/// Parses the comma-separated expressions after an opening paren through the closing one,
	/// which may turn out to be arrow function parameters.
	pub(crate) fn parse_paren_items(&mut self) -> Result<ParenItems> {
		let inner_start = self.tok.start;
		let mut items: Vec<Option<NodeId>> = Vec::new();
		let mut first = true;
		let mut last_is_comma = false;
		let mut spread_start = None;
		let mut errors = Some(DestructuringErrors::default());
		while !self.is(TokenKind::ParenR) {
			if first {
				first = false;
			} else {
				self.expect(TokenKind::Comma)?;
			}
			if self.after_trailing_comma(TokenKind::ParenR, true)? {
				last_is_comma = true;
				break;
			}
			if self.is(TokenKind::Ellipsis) {
				spread_start = Some(self.tok.start);
				let rest = self.parse_rest_binding()?;
				let rest = E::paren_item(self, rest)?;
				items.push(Some(rest));
				if self.is(TokenKind::Comma) {
					return self.error(self.tok.start, "Comma is not permitted after the rest element");
				}
				break;
			}
			items.push(Some(self.parse_paren_item(ForInit::No, &mut errors)?));
		}
		let inner_end = self.prev_end;
		self.expect(TokenKind::ParenR)?;
		Ok(ParenItems {
			items,
			errors,
			inner_start,
			inner_end,
			last_is_comma,
			spread_start,
		})
	}

	fn parse_new(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let meta = self.parse_ident(true)?;
		if self.eat(TokenKind::Dot)? {
			let escaped = self.tok.escaped;
			let property = self.parse_ident(true)?;
			if !self.ident_is(property, "target") {
				return self.error(
					self.start_of(property),
					"The only valid meta property for new is 'new.target'",
				);
			}
			if escaped {
				return self.error(start, "'new.target' must not contain escaped characters");
			}
			if !self.allow_new_target() {
				return self.error(
					start,
					"'new.target' can only be used in functions and class static block",
				);
			}
			return Ok(self.add(NodeKind::MetaProperty { meta, property }, start));
		}
		let callee_start = self.tok.start;
		let atom = self.parse_expr_atom(&mut None, ForInit::No, true)?;
		let callee = self.parse_subscripts(atom, callee_start, true, ForInit::No)?;
		if matches!(self.kind(callee), NodeKind::Super) {
			return self.error(self.start_of(callee), "Invalid use of 'super'");
		}
		let arguments = if self.eat(TokenKind::ParenL)? {
			let args = self.parse_expr_list(TokenKind::ParenR, true, false, &mut None)?;
			self.list(&args)
		} else {
			List::EMPTY
		};
		let node = self.add(NodeKind::NewExpression { callee, arguments }, start);
		E::new_expression(self, node);
		Ok(node)
	}

	pub(crate) fn parse_template(&mut self, is_tagged: bool) -> Result<NodeId> {
		let start = self.tok.start;
		let mut quasis = Vec::new();
		let mut expressions = Vec::new();
		loop {
			let chunk = self.lexer.read_template()?;
			let TokenKind::Template { cooked, raw, tail } = chunk.kind else {
				unreachable!()
			};
			if cooked.is_none() && !is_tagged {
				return self.error(chunk.start, "Bad escape sequence in untagged template literal");
			}
			quasis.push(self.add_with_end(NodeKind::TemplateElement { cooked, raw, tail }, chunk.start, chunk.end));
			if tail {
				self.prev_end = chunk.end + 1;
				self.tok = self.lexer.next_token()?;
				break;
			}
			self.prev_end = chunk.end + 2;
			self.tok = self.lexer.next_token()?;
			let expression = match E::template_expression(self)? {
				Some(expression) => expression,
				None => self.parse_expression(false, &mut None)?,
			};
			expressions.push(expression);
			if !self.is(TokenKind::BraceR) {
				return self.unexpected();
			}
		}
		let quasis = self.list_of(&quasis);
		let expressions = self.list_of(&expressions);
		Ok(self.add(NodeKind::TemplateLiteral { quasis, expressions }, start))
	}

	pub(crate) fn parse_obj(&mut self, is_pattern: bool, errors: &mut Errors) -> Result<NodeId> {
		let start = self.tok.start;
		let mut properties = Vec::new();
		let mut first = true;
		let mut has_proto = false;
		self.next()?;
		while !self.eat(TokenKind::BraceR)? {
			if !first {
				self.expect(TokenKind::Comma)?;
				if self.after_trailing_comma(TokenKind::BraceR, false)? {
					break;
				}
			} else {
				first = false;
			}
			let prop = self.parse_property(is_pattern, errors)?;
			if !is_pattern {
				self.check_prop_clash(prop, &mut has_proto, errors)?;
			}
			properties.push(prop);
		}
		let properties = self.list_of(&properties);
		let kind = if is_pattern {
			NodeKind::ObjectPattern { properties }
		} else {
			NodeKind::ObjectExpression { properties }
		};
		Ok(self.add(kind, start))
	}

	fn check_prop_clash(&self, prop: NodeId, has_proto: &mut bool, errors: &mut Errors) -> Result<()> {
		let NodeKind::Property {
			key,
			kind,
			computed,
			method,
			shorthand,
			..
		} = self.kind(prop)
		else {
			return Ok(());
		};
		if computed || method || shorthand || kind != PropertyKind::Init {
			return Ok(());
		}
		let is_proto = match self.kind(key) {
			NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } => self.str(name) == "__proto__",
			_ => false,
		};
		if is_proto {
			if *has_proto {
				match errors {
					Some(e) => {
						if e.double_proto.is_none() {
							e.double_proto = Some(self.start_of(key));
						}
					}
					None => return self.error(self.start_of(key), "Redefinition of __proto__ property"),
				}
			}
			*has_proto = true;
		}
		Ok(())
	}

	fn parse_property(&mut self, is_pattern: bool, errors: &mut Errors) -> Result<NodeId> {
		let start = self.tok.start;
		if self.eat(TokenKind::Ellipsis)? {
			if is_pattern {
				let argument = self.parse_ident(false)?;
				if self.is(TokenKind::Comma) {
					return self.error(self.tok.start, "Comma is not permitted after the rest element");
				}
				return Ok(self.add(NodeKind::RestElement { argument }, start));
			}
			let argument = self.parse_maybe_assign(ForInit::No, errors)?;
			if self.is(TokenKind::Comma)
				&& let Some(e) = errors
				&& e.trailing_comma.is_none()
			{
				e.trailing_comma = Some(self.tok.start);
			}
			return Ok(self.add(NodeKind::SpreadElement { argument }, start));
		}
		let mut generator = false;
		if !is_pattern {
			generator = self.eat(TokenKind::Star)?;
		}
		let escaped = self.tok.escaped;
		let (mut key, mut computed) = self.parse_property_name()?;
		let mut is_async = false;
		if !is_pattern && !escaped && !generator && !computed && self.is_async_prop(key) {
			is_async = true;
			generator = self.eat(TokenKind::Star)?;
			(key, computed) = self.parse_property_name()?;
		}
		self.parse_property_value(start, key, computed, is_pattern, generator, is_async, errors, escaped)
	}

	fn is_async_prop(&self, key: NodeId) -> bool {
		self.ident_is(key, "async")
			&& (matches!(
				self.tok.kind,
				TokenKind::Ident(_)
					| TokenKind::Number(_)
					| TokenKind::BigInt
					| TokenKind::String(_)
					| TokenKind::BracketL
					| TokenKind::Keyword(_)
					| TokenKind::Star
			) && !self.tok.newline_before)
	}

	#[allow(clippy::too_many_arguments)]
	fn parse_property_value(
		&mut self,
		start: u32,
		key: NodeId,
		computed: bool,
		is_pattern: bool,
		generator: bool,
		is_async: bool,
		errors: &mut Errors,
		escaped: bool,
	) -> Result<NodeId> {
		if let Some(prop) = E::property_value(self, start, key, computed, is_pattern, generator, is_async)? {
			return Ok(prop);
		}
		if (generator || is_async) && self.is(TokenKind::Colon) {
			return self.unexpected();
		}
		let mut kind = PropertyKind::Init;
		let mut method = false;
		let mut shorthand = false;
		let value;
		if self.eat(TokenKind::Colon)? {
			value = if is_pattern {
				let s = self.tok.start;
				self.parse_maybe_default(s, None)?
			} else {
				self.parse_maybe_assign(ForInit::No, errors)?
			};
		} else if self.is(TokenKind::ParenL) {
			if is_pattern {
				return self.unexpected();
			}
			method = true;
			value = self.parse_method(generator, is_async, false, false)?;
		} else if !is_pattern
			&& !escaped
			&& !computed
			&& (self.ident_is(key, "get") || self.ident_is(key, "set"))
			&& !self.is(TokenKind::Comma)
			&& !self.is(TokenKind::BraceR)
			&& !self.is(TokenKind::Eq)
		{
			if generator || is_async {
				return self.unexpected();
			}
			kind = if self.ident_is(key, "get") {
				PropertyKind::Get
			} else {
				PropertyKind::Set
			};
			let (accessor_key, accessor_computed) = self.parse_property_name()?;
			let func = self.parse_method(false, false, false, false)?;
			self.check_accessor_params(func, kind == PropertyKind::Get)?;
			return Ok(self.add(
				NodeKind::Property {
					key: accessor_key,
					value: func,
					kind,
					computed: accessor_computed,
					method,
					shorthand,
				},
				start,
			));
		} else if !computed && matches!(self.kind(key), NodeKind::Identifier { .. }) {
			if generator || is_async {
				return self.unexpected();
			}
			self.check_unreserved(key)?;
			if self.ident_is(key, "await") && self.await_ident_pos == 0 {
				self.await_ident_pos = start;
			}
			let copy = self.copy_node(key);
			if is_pattern {
				value = self.parse_maybe_default(start, Some(copy))?;
			} else if self.is(TokenKind::Eq) && errors.is_some() {
				let e = errors.as_mut().unwrap();
				if e.shorthand_assign.is_none() {
					e.shorthand_assign = Some(self.tok.start);
				}
				value = self.parse_maybe_default(start, Some(copy))?;
			} else {
				value = copy;
			}
			shorthand = true;
		} else {
			return self.unexpected();
		}
		Ok(self.add(
			NodeKind::Property {
				key,
				value,
				kind,
				computed,
				method,
				shorthand,
			},
			start,
		))
	}

	pub(crate) fn check_accessor_params(&self, func: NodeId, is_getter: bool) -> Result<()> {
		let params = match self.kind(func) {
			NodeKind::FunctionExpression { function } => function.params,
			_ => match E::function_params(self, func) {
				Some(params) => params,
				None => return Ok(()),
			},
		};
		let params = self.ast.list(params);
		let expected = if is_getter { 0 } else { 1 };
		if params.len() != expected {
			let start = self.start_of(func);
			return if is_getter {
				self.error(start, "getter should have no params")
			} else {
				self.error(start, "setter should have exactly one param")
			};
		}
		if !is_getter {
			let param = params[0].unwrap();
			if matches!(self.kind(param), NodeKind::RestElement { .. }) {
				return self.error(self.start_of(param), "Setter cannot use rest params");
			}
		}
		Ok(())
	}

	pub(crate) fn copy_node(&mut self, id: NodeId) -> NodeId {
		let node = *self.ast.node(id);
		self.ast.add(node.kind, node.start, node.end)
	}

	pub(crate) fn parse_property_name(&mut self) -> Result<(NodeId, bool)> {
		if self.eat(TokenKind::BracketL)? {
			let key = self.parse_maybe_assign(ForInit::No, &mut None)?;
			self.expect(TokenKind::BracketR)?;
			return Ok((key, true));
		}
		let key = match self.tok.kind {
			TokenKind::Number(_) | TokenKind::BigInt | TokenKind::String(_) => {
				self.parse_expr_atom(&mut None, ForInit::No, false)?
			}
			_ => self.parse_ident(true)?,
		};
		Ok((key, false))
	}

	pub(crate) fn parse_method(
		&mut self,
		generator: bool,
		is_async: bool,
		allow_direct_super: bool,
		in_class: bool,
	) -> Result<NodeId> {
		let start = self.tok.start;
		let (old_yield, old_await, old_await_ident) = (self.yield_pos, self.await_pos, self.await_ident_pos);
		self.yield_pos = 0;
		self.await_pos = 0;
		self.await_ident_pos = 0;
		self.enter_scope(
			function_flags(is_async, generator) | SCOPE_SUPER | if allow_direct_super { SCOPE_DIRECT_SUPER } else { 0 },
		);
		let kind = FunctionKind::Method { in_class };
		E::function_start(self, kind)?;
		self.expect(TokenKind::ParenL)?;
		let params = self.parse_binding_list(TokenKind::ParenR, false, true, in_class)?;
		self.check_yield_await_in_default_params()?;
		let params = self.list(&params);
		let node = match E::function_body(self, start, None, params, is_async, generator, kind)? {
			Some(node) => {
				self.exit_scope();
				node
			}
			None => {
				let (body, _) = self.parse_function_body(start, None, params, false, true, ForInit::No)?;
				let function = Function {
					id: None,
					params,
					body,
					is_async,
					generator,
				};
				self.add(NodeKind::FunctionExpression { function }, start)
			}
		};
		self.yield_pos = old_yield;
		self.await_pos = old_await;
		self.await_ident_pos = old_await_ident;
		E::function_end(self, node);
		Ok(node)
	}

	pub(crate) fn parse_arrow_expression(
		&mut self,
		start: u32,
		params: Vec<Option<NodeId>>,
		is_async: bool,
		for_init: ForInit,
	) -> Result<NodeId> {
		let (old_yield, old_await, old_await_ident) = (self.yield_pos, self.await_pos, self.await_ident_pos);
		self.enter_scope(function_flags(is_async, false) | SCOPE_ARROW);
		E::function_start(self, FunctionKind::Arrow)?;
		self.yield_pos = 0;
		self.await_pos = 0;
		self.await_ident_pos = 0;
		let params = self.make_patterns(params, true)?;
		let params = self.list(&params);
		let (body, expression) = self.parse_function_body(start, None, params, true, false, for_init)?;
		self.yield_pos = old_yield;
		self.await_pos = old_await;
		self.await_ident_pos = old_await_ident;
		let node = self.add(
			NodeKind::ArrowFunctionExpression {
				params,
				body,
				expression,
				is_async,
			},
			start,
		);
		E::function_end(self, node);
		Ok(node)
	}

	/// Parses a function body after the parameters, returning the body and whether it is an expression.
	pub(crate) fn parse_function_body(
		&mut self,
		start: u32,
		id: Option<NodeId>,
		params: List,
		is_arrow: bool,
		is_method: bool,
		for_init: ForInit,
	) -> Result<(NodeId, bool)> {
		let is_expression = is_arrow && !self.is(TokenKind::BraceL);
		let old_strict = self.strict;
		let result;
		if is_expression {
			let body = self.parse_maybe_assign(for_init, &mut None)?;
			self.check_params(params, false)?;
			result = (body, true);
		} else {
			let simple = self.is_simple_param_list(params);
			let mut use_strict = false;
			if !old_strict || !simple {
				use_strict = self.strict_directive(self.tok.end);
				if use_strict && !simple {
					return self.error(
						start,
						"Illegal 'use strict' directive in function with non-simple parameter list",
					);
				}
			}
			let old_labels = std::mem::take(&mut self.labels);
			if use_strict {
				self.set_strict(true);
			}
			self.check_params(params, !old_strict && !use_strict && !is_arrow && !is_method && simple)?;
			if self.strict
				&& let Some(id) = id
			{
				self.check_lval_simple(id, Binding::Outside, &mut None)?;
			}
			let body = self.parse_block(false, use_strict && !old_strict)?;
			let NodeKind::BlockStatement { body: statements } = self.kind(body) else {
				unreachable!()
			};
			self.adapt_directive_prologue(statements);
			self.labels = old_labels;
			result = (body, false);
		}
		self.exit_scope();
		self.set_strict(old_strict);
		Ok(result)
	}

	fn is_simple_param_list(&self, params: List) -> bool {
		self.ast
			.list(params)
			.iter()
			.all(|p| matches!(self.kind(p.unwrap()), NodeKind::Identifier { .. }))
	}

	pub(crate) fn check_params(&mut self, params: List, allow_duplicates: bool) -> Result<()> {
		let mut names = if allow_duplicates { None } else { Some(Vec::new()) };
		for i in 0..params.len {
			let param = self.ast.lists[(params.start + i) as usize].unwrap();
			self.check_lval_inner_pattern(param, Binding::Var, &mut names)?;
		}
		Ok(())
	}

	pub(crate) fn parse_expr_list(
		&mut self,
		close: TokenKind,
		allow_trailing_comma: bool,
		allow_empty: bool,
		errors: &mut Errors,
	) -> Result<Vec<Option<NodeId>>> {
		let mut elements = Vec::new();
		let mut first = true;
		while !self.eat(close)? {
			if !first {
				self.expect(TokenKind::Comma)?;
				if allow_trailing_comma && self.after_trailing_comma(close, false)? {
					break;
				}
			} else {
				first = false;
			}
			let element = if allow_empty && self.is(TokenKind::Comma) {
				None
			} else if self.is(TokenKind::Ellipsis) {
				let spread = self.parse_spread(errors)?;
				E::spread(self, spread)?;
				if self.is(TokenKind::Comma)
					&& let Some(e) = errors
					&& e.trailing_comma.is_none()
				{
					e.trailing_comma = Some(self.tok.start);
				}
				Some(spread)
			} else {
				Some(self.parse_paren_item(ForInit::No, errors)?)
			};
			elements.push(element);
		}
		Ok(elements)
	}

	fn parse_spread(&mut self, errors: &mut Errors) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		let argument = self.parse_maybe_assign(ForInit::No, errors)?;
		Ok(self.add(NodeKind::SpreadElement { argument }, start))
	}

	pub(crate) fn check_unreserved(&self, id: NodeId) -> Result<()> {
		let NodeKind::Identifier { name } = self.kind(id) else {
			return Ok(());
		};
		let start = self.start_of(id);
		let name = self.str(name);
		if self.in_generator() && name == "yield" {
			return self.error(start, "Cannot use 'yield' as identifier inside a generator");
		}
		if self.in_async() && name == "await" {
			return self.error(start, "Cannot use 'await' as identifier inside an async function");
		}
		if self.in_class_field_init() && name == "arguments" {
			return self.error(start, "Cannot use 'arguments' in class field initializer");
		}
		if self.in_class_static_block() && (name == "arguments" || name == "await") {
			return self.error(start, format!("Cannot use {name} in class static initialization block"));
		}
		if Keyword::from_word(name).is_some() {
			return self.error(start, format!("Unexpected keyword '{name}'"));
		}
		if self.is_reserved_word(name) {
			if !self.in_async() && name == "await" {
				return self.error(start, "Cannot use keyword 'await' outside an async function");
			}
			return self.error(start, format!("The keyword '{name}' is reserved"));
		}
		Ok(())
	}

	pub(crate) fn is_reserved_word(&self, name: &str) -> bool {
		match name {
			"enum" => true,
			"await" => self.options.module,
			"implements" | "interface" | "let" | "package" | "private" | "protected" | "public" | "static"
			| "yield" => self.strict,
			_ => false,
		}
	}

	pub(crate) fn parse_ident(&mut self, liberal: bool) -> Result<NodeId> {
		let start = self.tok.start;
		let name = match self.tok.kind {
			TokenKind::Ident(name) => name,
			TokenKind::Keyword(keyword) => self.intern(keyword.as_str()),
			_ => return self.unexpected(),
		};
		if liberal {
			self.next_liberal()?;
		} else {
			self.next()?;
		}
		let id = self.add(NodeKind::Identifier { name }, start);
		if !liberal {
			self.check_unreserved(id)?;
			if self.str(name) == "await" && self.await_ident_pos == 0 {
				self.await_ident_pos = start;
			}
		}
		Ok(id)
	}

	pub(crate) fn parse_private_ident(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		let TokenKind::PrivateName(name) = self.tok.kind else {
			return self.unexpected();
		};
		self.next()?;
		let id = self.add(NodeKind::PrivateIdentifier { name }, start);
		match self.private_names.last_mut() {
			Some(scope) => scope.used.push((name, start)),
			None => {
				return self.error(
					start,
					format!(
						"Private field '#{}' must be declared in an enclosing class",
						self.str(name)
					),
				);
			}
		}
		Ok(id)
	}

	fn parse_yield(&mut self, for_init: ForInit) -> Result<NodeId> {
		if self.yield_pos == 0 {
			self.yield_pos = self.tok.start;
		}
		let start = self.tok.start;
		self.next()?;
		let (delegate, argument) = if self.is(TokenKind::Semi)
			|| self.can_insert_semicolon()
			|| (!self.is(TokenKind::Star) && !starts_expression(self.tok.kind))
		{
			(false, None)
		} else {
			let delegate = self.eat(TokenKind::Star)?;
			(delegate, Some(self.parse_maybe_assign(for_init, &mut None)?))
		};
		Ok(self.add(NodeKind::YieldExpression { argument, delegate }, start))
	}

	fn parse_await(&mut self, for_init: ForInit) -> Result<NodeId> {
		if self.await_pos == 0 {
			self.await_pos = self.tok.start;
		}
		let start = self.tok.start;
		self.next()?;
		let argument = self.parse_maybe_unary(&mut None, true, false, for_init)?;
		Ok(self.add(NodeKind::AwaitExpression { argument }, start))
	}

	pub(crate) fn strict_directive(&self, pos: u32) -> bool {
		strict_directive(self.source(), pos)
	}
}

/// Scans for a `"use strict"` directive at the start of a body without tokenizing it.
pub(crate) fn strict_directive(source: &str, mut pos: u32) -> bool {
	let src = source.as_bytes();
	{
		loop {
			pos = skip_space(src, pos);
			let Some(quote) = src.get(pos as usize).filter(|b| **b == b'\'' || **b == b'"') else {
				return false;
			};
			let mut end = pos as usize + 1;
			while end < src.len() && src[end] != *quote {
				if src[end] == b'\\' {
					end += 1;
				}
				end += 1;
			}
			if end >= src.len() {
				return false;
			}
			let literal = &src[pos as usize + 1..end];
			let after = end as u32 + 1;
			if literal == b"use strict" {
				let next_pos = skip_space(src, after);
				let next = src.get(next_pos as usize).copied();
				if next == Some(b';') || next == Some(b'}') || next.is_none() {
					return true;
				}
				let between = &source[after as usize..next_pos as usize];
				let has_newline = between.chars().any(crate::lexer::is_new_line);
				let next = next.unwrap();
				return has_newline
					&& !(b"(`.[+-/*%<>=,?^&".contains(&next)
						|| (next == b'!' && src.get(next_pos as usize + 1) == Some(&b'=')));
			}
			pos = skip_space(src, after);
			if src.get(pos as usize) == Some(&b';') {
				pos += 1;
			}
		}
	}
}

/// Skips whitespace, line terminators and comments starting at a byte offset.
fn skip_space(src: &[u8], mut pos: u32) -> u32 {
	let text = std::str::from_utf8(src).unwrap();
	loop {
		let i = pos as usize;
		match src.get(i) {
			Some(b'/') if src.get(i + 1) == Some(&b'/') => {
				while let Some(&b) = src.get(pos as usize) {
					if b == b'\n' || b == b'\r' {
						break;
					}
					pos += 1;
				}
			}
			Some(b'/') if src.get(i + 1) == Some(&b'*') => {
				let mut j = i + 2;
				while j + 1 < src.len() && !(src[j] == b'*' && src[j + 1] == b'/') {
					j += 1;
				}
				pos = (j + 2).min(src.len()) as u32;
			}
			Some(_) => {
				let c = text[i..].chars().next().unwrap();
				if crate::lexer::is_new_line(c) || crate::lexer::is_whitespace(c) || c.is_ascii_whitespace() {
					pos += c.len_utf8() as u32;
				} else {
					return pos;
				}
			}
			None => return pos,
		}
	}
}

pub(crate) struct ParenItems {
	pub items: Vec<Option<NodeId>>,
	pub errors: Errors,
	pub inner_start: u32,
	pub inner_end: u32,
	pub last_is_comma: bool,
	pub spread_start: Option<u32>,
}

#[derive(Clone, Copy)]
enum Op {
	Binary(BinaryOperator),
	Logical(LogicalOperator),
	Coalesce,
}
