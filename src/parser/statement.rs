use super::expression::ForInit;
use super::scope::{
	Binding, SCOPE_CLASS_FIELD_INIT, SCOPE_CLASS_STATIC_BLOCK, SCOPE_SIMPLE_CATCH, SCOPE_SUPER, SCOPE_TOP,
	function_flags,
};
use super::{
	DestructuringErrors, Extension, FunctionKind, Label, LabelKind, Parser, PrivateKind, PrivateNameScope, Result,
	Unwrap,
};
use crate::ast::{Class, Function, List, MethodKind, NodeId, NodeKind, VariableKind};
use crate::error::Code;
use crate::interner::{FastSet, StrId};
use crate::lexer::token::{Keyword, TokenKind};
use crate::lexer::unicode::{is_id_continue, is_id_start};

pub(crate) const FUNC_STATEMENT: u8 = 1;
pub(crate) const FUNC_HANGING: u8 = 2;
pub(crate) const FUNC_NULLABLE_ID: u8 = 4;

/// The statement a nested statement is the body of, which restricts what may appear.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Context {
	None,
	If,
	Label,
	IfLabel,
	Other,
}

/// The statement list a statement appears directly in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatementPlace {
	TopLevel,
	Block,
	Case,
}

impl Context {
	fn with_label(self) -> Context {
		match self {
			Context::None => Context::Label,
			Context::If => Context::IfLabel,
			other => other,
		}
	}
}

impl<E: Extension> Parser<'_, E> {
	pub(crate) fn parse_program(&mut self) -> Result<NodeId> {
		let start = self.prev_end;
		let module = self.options.module;
		self.enter_scope(SCOPE_TOP);
		let mut body = self.items();
		let mut exports = FastSet::default();
		while !self.is(TokenKind::Eof) {
			if self.recovering() && self.lexer.unmatched {
				self.report_unexpected();
				self.next()?;
				continue;
			}
			let at = self.tok.start;
			if let Some(statement) = self.statement_recovered(|p| {
				p.parse_statement(Context::None, StatementPlace::TopLevel, Some(&mut exports))
			})? {
				body.push(Some(statement));
			}
			self.ensure_progress(at)?;
		}
		if module
			&& !self.options.allow_undeclared_exports
			&& let Some((&name, &(pos, _))) = self.undeclared_exports.iter().min_by_key(|(_, (_, order))| *order)
		{
			return self.error_name(pos, Code::UndefinedExport, name);
		}
		let body = self.list_from(body);
		self.adapt_directive_prologue(body);
		self.exit_scope();
		Ok(self.add_with_end(NodeKind::Program { body, module }, start, self.tok.end))
	}

	pub(crate) fn adapt_directive_prologue(&mut self, statements: List) {
		for i in 0..statements.len {
			let statement = self.nth(statements, i).unwrap();
			let NodeKind::ExpressionStatement {
				expression,
				directive: None,
			} = self.kind(statement)
			else {
				break;
			};
			let quoted = matches!(
				self.source().as_bytes().get(self.start_of(statement) as usize),
				Some(b'"' | b'\'')
			);
			if !quoted || !matches!(self.kind(expression), NodeKind::StringLiteral { .. }) {
				break;
			}
			let (start, end) = (self.start_of(expression), self.end_of(expression));
			// under recovery the string may end with the input, its quote never closed
			if end < start + 2 || self.source().as_bytes()[end as usize - 1] != self.source().as_bytes()[start as usize]
			{
				break;
			}
			let raw = &self.source()[start as usize + 1..end as usize - 1];
			let directive = self.intern(raw);
			self.ast.node_mut(statement).kind = NodeKind::ExpressionStatement {
				expression,
				directive: Some(directive),
			};
		}
	}

	pub(crate) fn parse_statement(
		&mut self,
		context: Context,
		place: StatementPlace,
		exports: Option<&mut FastSet<StrId>>,
	) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_statement_inner(context, place, exports);
		self.leave();
		result
	}

	fn parse_statement_inner(
		&mut self,
		context: Context,
		place: StatementPlace,
		exports: Option<&mut FastSet<StrId>>,
	) -> Result<NodeId> {
		if let Some(statement) = E::statement(self, context, place)? {
			return Ok(statement);
		}
		let start = self.tok.start;
		if self.is_let(context) {
			if context != Context::None {
				return self.unexpected();
			}
			return self.parse_var_statement(start, VariableKind::Let);
		}
		if context == Context::None
			&& let Some(kind) = self.using_kind(false)
		{
			if place == StatementPlace::Case
				|| (place == StatementPlace::TopLevel && !self.options.module && !E::exports_in_script(self))
			{
				let error = self.error_arg(start, Code::UsingOutsideBlock, kind.as_str());
				self.record(error)?;
			}
			return self.parse_var_statement(start, kind);
		}
		match self.tok.kind {
			TokenKind::Keyword(Keyword::Break) => self.parse_break_continue(start, true),
			TokenKind::Keyword(Keyword::Continue) => self.parse_break_continue(start, false),
			TokenKind::Keyword(Keyword::Debugger) => {
				self.next()?;
				self.semicolon()?;
				Ok(self.add(NodeKind::DebuggerStatement, start))
			}
			TokenKind::Keyword(Keyword::Do) => self.parse_do(start),
			TokenKind::Keyword(Keyword::For) => self.parse_for(start),
			TokenKind::Keyword(Keyword::Function) => {
				if context != Context::None && (self.strict || (context != Context::If && context != Context::Label)) {
					return self.unexpected();
				}
				self.next()?;
				let flags = FUNC_STATEMENT | if context != Context::None { FUNC_HANGING } else { 0 };
				self.parse_function(start, flags, false, ForInit::No)
			}
			TokenKind::Keyword(Keyword::Class) => {
				if context != Context::None {
					return self.unexpected();
				}
				self.parse_class(ClassKind::Declaration)
			}
			TokenKind::Keyword(Keyword::If) => self.parse_if(start),
			TokenKind::Keyword(Keyword::Return) => self.parse_return(start),
			TokenKind::Keyword(Keyword::Switch) => self.parse_switch(start),
			TokenKind::Keyword(Keyword::Throw) => self.parse_throw(start),
			TokenKind::Keyword(Keyword::Try) => self.parse_try(start),
			TokenKind::Keyword(Keyword::Const) => {
				if context != Context::None {
					return self.unexpected();
				}
				self.parse_var_statement(start, VariableKind::Const)
			}
			TokenKind::Keyword(Keyword::Var) => self.parse_var_statement(start, VariableKind::Var),
			TokenKind::Keyword(Keyword::While) => self.parse_while(start),
			TokenKind::Keyword(Keyword::With) => self.parse_with(start),
			TokenKind::BraceL => self.parse_block(true, false),
			TokenKind::Semi => {
				self.next()?;
				Ok(self.add(NodeKind::EmptyStatement, start))
			}
			TokenKind::Keyword(Keyword::Import) | TokenKind::Keyword(Keyword::Export) => {
				let is_import = self.is_keyword(Keyword::Import);
				if is_import && matches!(self.peek_char().0, Some('(' | '.')) {
					let expression = self.parse_expression(false, &mut None)?;
					return self.parse_expression_statement(start, expression, context);
				}
				if place != StatementPlace::TopLevel {
					return self.error(start, Code::ImportExportNotTopLevel);
				}
				if !self.options.module && !E::exports_in_script(self) {
					return self.error(start, Code::ImportExportInScript);
				}
				if is_import {
					self.parse_import(start)
				} else {
					self.parse_export(start, exports.unwrap())
				}
			}
			_ => {
				if self.is_async_function() {
					if context != Context::None {
						return self.unexpected();
					}
					self.next()?;
					self.next()?;
					return self.parse_function(start, FUNC_STATEMENT, true, ForInit::No);
				}
				let maybe_label = self.ident_name();
				let expression = self.parse_expression(false, &mut None)?;
				if let Some(name) = maybe_label
					&& matches!(self.kind(expression), NodeKind::Identifier { .. })
					&& self.eat(TokenKind::Colon)?
				{
					return self.parse_labeled(start, name, expression, context);
				}
				self.parse_expression_statement(start, expression, context)
			}
		}
	}

	/// Whether a `let` token starts a declaration rather than being an identifier.
	fn is_let(&self, context: Context) -> bool {
		if !self.is_contextual("let") {
			return false;
		}
		if self.strict && self.recovering() {
			return true;
		}
		let (next, _, pos) = self.peek_char();
		let Some(next) = next else { return false };
		if next == '[' || next == '\\' {
			return true;
		}
		if context != Context::None {
			return false;
		}
		if next == '{' {
			return true;
		}
		self.starts_binding_identifier(pos)
	}

	fn starts_binding_identifier(&self, pos: usize) -> bool {
		let rest = &self.source()[pos..];
		if rest.starts_with('\\') {
			return true;
		}
		if rest.chars().next().is_some_and(is_id_start) {
			let len = rest.find(|c| !is_id_continue(c)).unwrap_or(rest.len());
			if rest[len..].starts_with('\\') {
				return true;
			}
			let word = &rest[..len];
			return word != "in" && word != "instanceof";
		}
		false
	}

	/// Whether `using` or `await using` here starts a declaration: a binding name follows on the
	/// same line, and in a for head `using of` is a declaration only when what follows `of` says so.
	fn using_kind(&self, is_for: bool) -> Option<VariableKind> {
		let kind = if self.is_contextual("using") {
			VariableKind::Using
		} else if self.can_await() && self.is_contextual("await") {
			VariableKind::AwaitUsing
		} else {
			return None;
		};
		let (_, newline, mut pos) = self.peek_char();
		if newline {
			return None;
		}
		if kind == VariableKind::AwaitUsing {
			let rest = self.source()[pos..].strip_prefix("using")?;
			if rest.starts_with('\\') || rest.chars().next().is_some_and(is_id_continue) {
				return None;
			}
			let (_, newline, binding) = self.lexer.peek_char_from(pos + 5);
			if newline {
				return None;
			}
			pos = binding;
		}
		if !self.starts_binding_identifier(pos) {
			return None;
		}
		if is_for
			&& kind == VariableKind::Using
			&& let Some(rest) = self.source()[pos..].strip_prefix("of")
			&& !rest.starts_with('\\')
			&& !rest.chars().next().is_some_and(is_id_continue)
		{
			let (next, _, pos) = self.lexer.peek_char_from(pos + 2);
			let after = self.source().as_bytes().get(pos + 1);
			if !matches!(next, Some(';' | ':'))
				&& !(next == Some('=') && !matches!(after, Some(b'=' | b'>')))
				&& !(next == Some('!') && after != Some(&b'='))
			{
				return None;
			}
		}
		Some(kind)
	}

	fn is_async_function(&self) -> bool {
		if !self.is_contextual("async") {
			return false;
		}
		let (next, newline, pos) = self.peek_char();
		if newline || next != Some('f') {
			return false;
		}
		let rest = &self.source()[pos..];
		rest.starts_with("function") && !rest[8..].chars().next().is_some_and(is_id_continue)
	}

	fn parse_break_continue(&mut self, start: u32, is_break: bool) -> Result<NodeId> {
		self.next()?;
		let label = if self.eat(TokenKind::Semi)? || self.can_insert_semicolon() {
			None
		} else if !matches!(self.tok.kind, TokenKind::Ident(_)) {
			return self.unexpected();
		} else {
			let label = self.parse_ident(false)?;
			self.semicolon()?;
			Some(label)
		};
		let label_name = label.map(|l| match self.kind(l) {
			NodeKind::Identifier { name } => name,
			_ => unreachable!(),
		});
		let found = self.labels.iter().any(|lab| {
			(label_name.is_none() || lab.name == label_name)
				&& ((lab.kind != LabelKind::None && (is_break || lab.kind == LabelKind::Loop))
					|| (label.is_some() && is_break))
		});
		if !found {
			return self.error_arg(start, Code::Unsyntactic, if is_break { "break" } else { "continue" });
		}
		let kind = if is_break {
			NodeKind::BreakStatement { label }
		} else {
			NodeKind::ContinueStatement { label }
		};
		Ok(self.add(kind, start))
	}

	fn push_label(&mut self, kind: LabelKind) {
		self.labels.push(Label {
			name: None,
			kind,
			statement_start: 0,
		});
	}

	fn parse_do(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		self.push_label(LabelKind::Loop);
		let body = self.parse_statement(Context::Other, StatementPlace::Block, None)?;
		self.labels.pop();
		self.expect_keyword(Keyword::While)?;
		let test = self.parse_paren_expression()?;
		self.eat(TokenKind::Semi)?;
		Ok(self.add(NodeKind::DoWhileStatement { body, test }, start))
	}

	fn parse_for(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		let await_at = if self.can_await() && self.eat_contextual("await")? {
			Some(self.prev_end - 5)
		} else {
			None
		};
		self.push_label(LabelKind::Loop);
		self.enter_scope(0);
		self.expect(TokenKind::ParenL)?;
		if self.is(TokenKind::Semi) {
			if let Some(pos) = await_at {
				return self.unexpected_at(pos);
			}
			return self.parse_for_rest(start, None);
		}
		let is_let = self.is_let(Context::None);
		let using = self.using_kind(true);
		if self.is_keyword(Keyword::Var) || self.is_keyword(Keyword::Const) || is_let || using.is_some() {
			let init_start = self.tok.start;
			let kind = if let Some(kind) = using {
				kind
			} else if is_let {
				VariableKind::Let
			} else if self.is_keyword(Keyword::Var) {
				VariableKind::Var
			} else {
				VariableKind::Const
			};
			let init = self.parse_var(init_start, true, kind)?;
			let NodeKind::VariableDeclaration { declarations, .. } = self.kind(init) else {
				unreachable!()
			};
			if kind == VariableKind::Var && self.is_contextual("of") {
				self.check_for_of_var(declarations)?;
			}
			if (self.is_keyword(Keyword::In) || self.is_contextual("of")) && declarations.len == 1 {
				let is_await = if self.is_keyword(Keyword::In) {
					if let Some(pos) = await_at {
						return self.unexpected_at(pos);
					}
					false
				} else {
					await_at.is_some()
				};
				return self.parse_for_in(start, init, is_await);
			}
			if let Some(pos) = await_at {
				return self.unexpected_at(pos);
			}
			return self.parse_for_rest(start, Some(init));
		}
		let starts_with_let = self.is_contextual("let");
		let escaped = self.tok.escaped;
		let mut errors = Some(DestructuringErrors::default());
		let init_pos = self.tok.start;
		let init = if await_at.is_some() {
			self.parse_expr_subscripts(&mut errors, ForInit::Await)?
		} else {
			self.parse_sequence(ForInit::Yes, &mut errors)?
		};
		let is_for_of = self.is_contextual("of");
		if self.is_keyword(Keyword::In) || is_for_of {
			let mut is_await = false;
			if let Some(pos) = await_at {
				if self.is_keyword(Keyword::In) {
					return self.unexpected_at(pos);
				}
				is_await = true;
			} else if is_for_of && self.start_of(init) == init_pos && !escaped && self.ident_is(init, "async") {
				return self.unexpected();
			}
			if starts_with_let && is_for_of {
				return self.error(self.start_of(init), Code::ForOfLet);
			}
			let init = self.make_pattern(init, false, &mut errors)?;
			self.check_lval_pattern(init, Binding::None, &mut None)?;
			return self.parse_for_in(start, init, is_await);
		}
		self.check_expression_errors(&errors, true)?;
		if let Some(pos) = await_at {
			return self.unexpected_at(pos);
		}
		self.parse_for_rest(start, Some(init))
	}

	fn parse_for_rest(&mut self, start: u32, init: Option<NodeId>) -> Result<NodeId> {
		self.expect(TokenKind::Semi)?;
		let test = if self.is(TokenKind::Semi) {
			None
		} else {
			Some(self.parse_expression(false, &mut None)?)
		};
		self.expect(TokenKind::Semi)?;
		let update = if self.is(TokenKind::ParenR) {
			None
		} else {
			Some(self.parse_expression(false, &mut None)?)
		};
		self.expect(TokenKind::ParenR)?;
		let body = self.parse_statement(Context::Other, StatementPlace::Block, None)?;
		self.exit_scope();
		self.labels.pop();
		Ok(self.add(
			NodeKind::ForStatement {
				init,
				test,
				update,
				body,
			},
			start,
		))
	}

	fn parse_for_in(&mut self, start: u32, left: NodeId, is_await: bool) -> Result<NodeId> {
		let is_for_in = self.is_keyword(Keyword::In);
		self.next()?;
		if let NodeKind::VariableDeclaration { declarations, kind } = self.kind(left) {
			if is_for_in && kind.is_using() {
				let error = self.error_arg(self.start_of(left), Code::UsingInForIn, kind.as_str());
				self.record(error)?;
			}
			let first = self.ast.list(declarations)[0].unwrap();
			let NodeKind::VariableDeclarator { id, init } = self.kind(first) else {
				unreachable!()
			};
			if init.is_some()
				&& (!is_for_in
					|| self.strict || kind != VariableKind::Var
					|| !matches!(self.kind(id), NodeKind::Identifier { .. }))
			{
				let loop_kind = if is_for_in { "for-in" } else { "for-of" };
				return self.error_arg(self.start_of(left), Code::ForInOfInitializer, loop_kind);
			}
		}
		let right = if is_for_in {
			self.parse_expression(false, &mut None)?
		} else {
			self.parse_maybe_assign(ForInit::No, &mut None)?
		};
		self.expect(TokenKind::ParenR)?;
		let body = self.parse_statement(Context::Other, StatementPlace::Block, None)?;
		self.exit_scope();
		self.labels.pop();
		let kind = if is_for_in {
			NodeKind::ForInStatement { left, right, body }
		} else {
			NodeKind::ForOfStatement {
				left,
				right,
				body,
				is_await,
			}
		};
		Ok(self.add(kind, start))
	}

	pub(crate) fn parse_function(
		&mut self,
		start: u32,
		flags: u8,
		is_async: bool,
		for_init: ForInit,
	) -> Result<NodeId> {
		let is_statement = flags & FUNC_STATEMENT != 0;
		if self.is(TokenKind::Star) && flags & FUNC_HANGING != 0 {
			return self.unexpected();
		}
		let generator = self.eat(TokenKind::Star)?;
		let mut id = None;
		if is_statement && (flags & FUNC_NULLABLE_ID == 0 || matches!(self.tok.kind, TokenKind::Ident(_))) {
			let name = self.parse_ident(false)?;
			if flags & FUNC_HANGING == 0 && !E::DECLARES_FUNCTION_NAME_AFTER_BODY {
				let binding = self.function_binding(generator, is_async);
				self.check_lval_simple(name, binding, &mut None)?;
			}
			id = Some(name);
		}
		let old = self.take_yield_await();
		self.enter_scope(function_flags(is_async, generator));
		if !is_statement && matches!(self.tok.kind, TokenKind::Ident(_)) {
			id = Some(self.parse_ident(false)?);
		}
		let kind = if is_statement {
			FunctionKind::Declaration
		} else {
			FunctionKind::Expression
		};
		E::function_start(self, kind)?;
		self.expect(TokenKind::ParenL)?;
		let params = self.parse_binding_list(TokenKind::ParenR, false, true, false)?;
		self.check_yield_await_in_default_params()?;
		let params = self.list_from(params);
		let node = match E::function_body(self, start, id, params, is_async, generator, kind)? {
			Some(node) => {
				self.exit_scope();
				node
			}
			None => {
				let (body, _) = self.parse_function_body(start, id, params, false, false, for_init)?;
				let function = Function {
					id,
					params,
					body,
					is_async,
					generator,
				};
				let kind = if is_statement {
					NodeKind::FunctionDeclaration { function }
				} else {
					NodeKind::FunctionExpression { function }
				};
				self.add(kind, start)
			}
		};
		self.restore_yield_await(old);
		if is_statement
			&& flags & FUNC_HANGING == 0
			&& E::DECLARES_FUNCTION_NAME_AFTER_BODY
			&& let Some(name) = id
		{
			let binding = if matches!(self.kind(node), NodeKind::FunctionDeclaration { .. }) {
				self.function_binding(generator, is_async)
			} else {
				Binding::None
			};
			self.check_lval_simple(name, binding, &mut None)?;
		}
		E::function_end(self, node)?;
		Ok(node)
	}

	fn function_binding(&self, generator: bool, is_async: bool) -> Binding {
		if self.strict || generator || is_async {
			if self.treat_functions_as_var() {
				Binding::Var
			} else {
				Binding::Lexical
			}
		} else {
			Binding::Function
		}
	}

	fn parse_if(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		let test = self.parse_paren_expression()?;
		let consequent = self.parse_statement(Context::If, StatementPlace::Block, None)?;
		let alternate = if self.eat_keyword(Keyword::Else)? {
			Some(self.parse_statement(Context::If, StatementPlace::Block, None)?)
		} else {
			None
		};
		Ok(self.add(
			NodeKind::IfStatement {
				test,
				consequent,
				alternate,
			},
			start,
		))
	}

	fn parse_return(&mut self, start: u32) -> Result<NodeId> {
		if !self.in_function()
			&& !(self.options.allow_return_outside_function && self.current_var_scope().flags & SCOPE_TOP != 0)
		{
			return self.error(start, Code::ReturnOutsideFunction);
		}
		self.next()?;
		let argument = if self.eat(TokenKind::Semi)? || self.can_insert_semicolon() {
			None
		} else {
			let argument = self.parse_expression(false, &mut None)?;
			self.semicolon()?;
			Some(argument)
		};
		Ok(self.add(NodeKind::ReturnStatement { argument }, start))
	}

	fn parse_switch(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		let discriminant = self.parse_paren_expression()?;
		self.expect(TokenKind::BraceL)?;
		self.push_label(LabelKind::Switch);
		self.enter_scope(0);
		let mut cases = self.items();
		let mut current: Option<(u32, Option<NodeId>, Vec<Option<NodeId>>)> = None;
		let mut saw_default = false;
		while !self.is(TokenKind::BraceR) {
			if self.is_keyword(Keyword::Case) || self.is_keyword(Keyword::Default) {
				let is_case = self.is_keyword(Keyword::Case);
				if let Some(case) = current.take() {
					cases.push(Some(self.switch_case(case)));
				}
				let case_start = self.tok.start;
				self.next()?;
				let test = if is_case {
					Some(self.parse_expression(false, &mut None)?)
				} else {
					if saw_default {
						return self.error(case_start, Code::DuplicateDefault);
					}
					saw_default = true;
					None
				};
				self.expect(TokenKind::Colon)?;
				current = Some((case_start, test, self.items()));
			} else {
				let Some(current) = current.as_mut() else {
					return self.unexpected();
				};
				let statement = self.parse_statement(Context::None, StatementPlace::Case, None)?;
				current.2.push(Some(statement));
			}
		}
		self.exit_scope();
		if let Some(case) = current.take() {
			cases.push(Some(self.switch_case(case)));
		}
		self.next()?;
		self.labels.pop();
		let cases = self.list_from(cases);
		Ok(self.add(NodeKind::SwitchStatement { discriminant, cases }, start))
	}

	fn switch_case(&mut self, (start, test, consequent): (u32, Option<NodeId>, Vec<Option<NodeId>>)) -> NodeId {
		let consequent = self.list_from(consequent);
		self.add(NodeKind::SwitchCase { test, consequent }, start)
	}

	fn parse_throw(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		if self.tok.newline_before {
			return self.error(self.prev_end, Code::NewlineAfterThrow);
		}
		let argument = self.parse_expression(false, &mut None)?;
		self.semicolon()?;
		Ok(self.add(NodeKind::ThrowStatement { argument }, start))
	}

	fn parse_try(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		let block = self.parse_block(true, false)?;
		let mut handler = None;
		if self.is_keyword(Keyword::Catch) {
			let clause_start = self.tok.start;
			self.next()?;
			let param = if self.eat(TokenKind::ParenL)? {
				let param = self.parse_binding_atom()?;
				let simple = matches!(self.kind(param), NodeKind::Identifier { .. });
				self.enter_scope(if simple { SCOPE_SIMPLE_CATCH } else { 0 });
				self.check_lval_pattern(
					param,
					if simple { Binding::SimpleCatch } else { Binding::Lexical },
					&mut None,
				)?;
				E::catch_param(self, param)?;
				self.expect(TokenKind::ParenR)?;
				Some(param)
			} else {
				self.enter_scope(0);
				None
			};
			let body = self.parse_block(false, false)?;
			self.exit_scope();
			handler = Some(self.add(NodeKind::CatchClause { param, body }, clause_start));
		}
		let finalizer = if self.eat_keyword(Keyword::Finally)? {
			Some(self.parse_block(true, false)?)
		} else {
			None
		};
		if handler.is_none() && finalizer.is_none() {
			return self.error(start, Code::MissingCatchOrFinally);
		}
		Ok(self.add(
			NodeKind::TryStatement {
				block,
				handler,
				finalizer,
			},
			start,
		))
	}

	pub(crate) fn parse_var_statement(&mut self, start: u32, kind: VariableKind) -> Result<NodeId> {
		let node = self.parse_var(start, false, kind)?;
		self.semicolon()?;
		self.ast.node_mut(node).end = self.prev_end;
		Ok(node)
	}

	fn parse_while(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		let test = self.parse_paren_expression()?;
		self.push_label(LabelKind::Loop);
		let body = self.parse_statement(Context::Other, StatementPlace::Block, None)?;
		self.labels.pop();
		Ok(self.add(NodeKind::WhileStatement { test, body }, start))
	}

	fn parse_with(&mut self, start: u32) -> Result<NodeId> {
		if self.strict {
			return self.error(start, Code::StrictWith);
		}
		self.next()?;
		let object = self.parse_paren_expression()?;
		let body = self.parse_statement(Context::Other, StatementPlace::Block, None)?;
		Ok(self.add(NodeKind::WithStatement { object, body }, start))
	}

	fn parse_labeled(&mut self, start: u32, name: StrId, label: NodeId, context: Context) -> Result<NodeId> {
		if self.labels.iter().any(|l| l.name == Some(name)) {
			return self.error_name(self.start_of(label), Code::DuplicateLabel, name);
		}
		let kind = match self.tok.kind {
			TokenKind::Keyword(Keyword::Do | Keyword::For | Keyword::While) => LabelKind::Loop,
			TokenKind::Keyword(Keyword::Switch) => LabelKind::Switch,
			_ => LabelKind::None,
		};
		let statement_start = self.tok.start;
		for l in self.labels.iter_mut().rev() {
			if l.statement_start == start {
				l.statement_start = statement_start;
				l.kind = kind;
			} else {
				break;
			}
		}
		self.labels.push(Label {
			name: Some(name),
			kind,
			statement_start,
		});
		let body = self.parse_statement(context.with_label(), StatementPlace::Block, None)?;
		self.labels.pop();
		Ok(self.add(NodeKind::LabeledStatement { label, body }, start))
	}

	fn parse_expression_statement(&mut self, start: u32, expression: NodeId, context: Context) -> Result<NodeId> {
		if matches!(self.kind(expression), NodeKind::Identifier { .. })
			&& let Some(statement) = E::expression_statement(self, start, expression, context)?
		{
			return Ok(statement);
		}
		self.semicolon()?;
		Ok(self.add(
			NodeKind::ExpressionStatement {
				expression,
				directive: None,
			},
			start,
		))
	}

	fn parse_paren_expression(&mut self) -> Result<NodeId> {
		self.expect(TokenKind::ParenL)?;
		let value = self.parse_expression(false, &mut None)?;
		self.expect(TokenKind::ParenR)?;
		Ok(value)
	}

	pub(crate) fn parse_block(&mut self, new_scope: bool, exit_strict: bool) -> Result<NodeId> {
		let start = self.tok.start;
		self.expect(TokenKind::BraceL)?;
		if new_scope {
			self.enter_scope(0);
		}
		let mut body = self.items();
		while !self.is(TokenKind::BraceR) {
			let at = self.tok.start;
			if let Some(statement) =
				self.statement_recovered(|p| p.parse_statement(Context::None, StatementPlace::Block, None))?
			{
				body.push(Some(statement));
			}
			self.ensure_progress(at)?;
		}
		if exit_strict {
			self.set_strict(false);
		}
		self.next()?;
		if new_scope {
			self.exit_scope();
		}
		let body = self.list_from(body);
		Ok(self.add(NodeKind::BlockStatement { body }, start))
	}

	/// Annex B.3.4 lets a var redeclare the parameter of a simple catch clause, except the binding
	/// of a for-of, which is checked once the `of` is seen.
	fn check_for_of_var(&mut self, declarations: List) -> Result<()> {
		let mut stack = self.items();
		stack.extend_from_slice(self.ast.list(declarations));
		while let Some(item) = stack.pop() {
			let Some(id) = item else { continue };
			match self.kind(id) {
				NodeKind::Identifier { name } => {
					if self.rebinds_catch_param(name) {
						return self.error_name(self.start_of(id), Code::Redeclaration, name);
					}
				}
				NodeKind::VariableDeclarator { id, .. } => stack.push(Some(id)),
				NodeKind::ObjectPattern { properties: list } | NodeKind::ArrayPattern { elements: list } => {
					stack.extend_from_slice(self.ast.list(list));
				}
				NodeKind::Property { value, .. } => stack.push(Some(value)),
				NodeKind::RestElement { argument } => stack.push(Some(argument)),
				NodeKind::AssignmentPattern { left, .. } => stack.push(Some(left)),
				NodeKind::Extension(_) => {
					if let Some(inner) = E::unwrap(self, id, Unwrap::InnerPattern) {
						stack.push(Some(inner));
					}
				}
				_ => {}
			}
		}
		self.recycle(stack);
		Ok(())
	}

	fn parse_var(&mut self, start: u32, is_for: bool, kind: VariableKind) -> Result<NodeId> {
		if kind.is_using() && E::in_ambient(self) {
			let error = self.error_arg(start, Code::UsingInAmbient, kind.as_str());
			self.record(error)?;
		}
		self.next()?;
		if kind == VariableKind::AwaitUsing {
			self.next()?;
		}
		let mut declarations = self.items();
		loop {
			let decl_start = self.tok.start;
			let id = self.parse_binding_atom()?;
			if kind.is_using() && !matches!(self.kind(id), NodeKind::Identifier { .. }) {
				let error = self.error_arg(decl_start, Code::UsingPattern, kind.as_str());
				self.record(error)?;
			}
			let binding = if kind == VariableKind::Var {
				Binding::Var
			} else {
				Binding::Lexical
			};
			self.check_lval_pattern(id, binding, &mut None)?;
			E::var_id(self, id)?;
			let init = if self.eat(TokenKind::Eq)? {
				Some(self.parse_maybe_assign(if is_for { ForInit::Yes } else { ForInit::No }, &mut None)?)
			} else {
				let in_or_of = self.is_keyword(Keyword::In) || self.is_contextual("of");
				let missing_allowed = E::allows_missing_initializer(self);
				if kind.is_using() && !(is_for && in_or_of) && !missing_allowed {
					let error = self.error_arg(self.prev_end, Code::UsingWithoutInitializer, kind.as_str());
					self.record(error)?;
				} else if kind == VariableKind::Const && !in_or_of && !missing_allowed {
					let error = self.unexpected();
					self.record(error)?;
				} else if !matches!(self.kind(id), NodeKind::Identifier { .. })
					&& !(is_for && in_or_of)
					&& !missing_allowed
				{
					let error = self.error(self.prev_end, Code::PatternWithoutInitializer);
					self.record(error)?;
				}
				None
			};
			let declarator = self.add(NodeKind::VariableDeclarator { id, init }, decl_start);
			E::var_declarator(self, declarator, kind)?;
			declarations.push(Some(declarator));
			if !self.eat(TokenKind::Comma)? {
				break;
			}
		}
		let declarations = self.list_from(declarations);
		Ok(self.add(NodeKind::VariableDeclaration { declarations, kind }, start))
	}

	// Modules

	fn parse_import(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		if let Some(node) = E::import_head(self, start)? {
			return Ok(node);
		}
		let specifiers;
		let source;
		if matches!(self.tok.kind, TokenKind::String(_)) {
			specifiers = self.items();
			source = self.parse_expr_atom(&mut None, ForInit::No, false)?;
		} else {
			specifiers = self.parse_import_specifiers()?;
			self.expect_contextual("from")?;
			if !matches!(self.tok.kind, TokenKind::String(_)) {
				return self.unexpected();
			}
			source = self.parse_expr_atom(&mut None, ForInit::No, false)?;
		}
		let attributes = self.parse_with_clause()?;
		self.semicolon()?;
		let specifiers = self.list_from(specifiers);
		let node = self.add(
			NodeKind::ImportDeclaration {
				specifiers,
				source,
				attributes,
			},
			start,
		);
		E::import_end(self, node)?;
		Ok(node)
	}

	fn parse_import_specifiers(&mut self) -> Result<Vec<Option<NodeId>>> {
		let mut nodes = self.items();
		if matches!(self.tok.kind, TokenKind::Ident(_)) {
			let start = self.tok.start;
			let local = self.parse_ident(false)?;
			self.check_lval_simple(local, Binding::Lexical, &mut None)?;
			nodes.push(Some(self.add(NodeKind::ImportDefaultSpecifier { local }, start)));
			if !self.eat(TokenKind::Comma)? {
				return Ok(nodes);
			}
		}
		if self.is(TokenKind::Star) {
			let start = self.tok.start;
			self.next()?;
			self.expect_contextual("as")?;
			let local = self.parse_ident(false)?;
			self.check_lval_simple(local, Binding::Lexical, &mut None)?;
			nodes.push(Some(self.add(NodeKind::ImportNamespaceSpecifier { local }, start)));
			return Ok(nodes);
		}
		self.expect(TokenKind::BraceL)?;
		let mut first = true;
		while !self.eat(TokenKind::BraceR)? {
			if self.list_comma(TokenKind::BraceR, &mut first, true)? {
				break;
			}
			if let Some(node) = E::import_specifier(self)? {
				nodes.push(Some(node));
				continue;
			}
			let start = self.tok.start;
			let imported = self.parse_module_export_name()?;
			let local = if self.eat_contextual("as")? {
				self.parse_ident(false)?
			} else {
				self.check_unreserved(imported)?;
				imported
			};
			self.check_lval_simple(local, Binding::Lexical, &mut None)?;
			nodes.push(Some(self.add(NodeKind::ImportSpecifier { imported, local }, start)));
		}
		Ok(nodes)
	}

	fn parse_with_clause(&mut self) -> Result<crate::ast::List> {
		if !self.eat_keyword(Keyword::With)? {
			return Ok(List::EMPTY);
		}
		let mut nodes = self.items();
		self.expect(TokenKind::BraceL)?;
		let mut first = true;
		let mut seen: Vec<StrId> = Vec::new();
		while !self.eat(TokenKind::BraceR)? {
			if self.list_comma(TokenKind::BraceR, &mut first, true)? {
				break;
			}
			let start = self.tok.start;
			let key = if matches!(self.tok.kind, TokenKind::String(_)) {
				self.parse_expr_atom(&mut None, ForInit::No, false)?
			} else {
				self.parse_ident(true)?
			};
			self.expect(TokenKind::Colon)?;
			if !matches!(self.tok.kind, TokenKind::String(_)) {
				return self.unexpected();
			}
			let value = self.parse_expr_atom(&mut None, ForInit::No, false)?;
			let key_name = match self.kind(key) {
				NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } => name,
				_ => unreachable!(),
			};
			if seen.contains(&key_name) {
				return self.error_name(self.start_of(key), Code::DuplicateImportAttribute, key_name);
			}
			seen.push(key_name);
			nodes.push(Some(self.add(NodeKind::ImportAttribute { key, value }, start)));
		}
		Ok(self.list_from(nodes))
	}

	pub(crate) fn parse_module_export_name(&mut self) -> Result<NodeId> {
		if matches!(self.tok.kind, TokenKind::String(_)) {
			let literal = self.parse_expr_atom(&mut None, ForInit::No, false)?;
			return Ok(literal);
		}
		self.parse_ident(true)
	}

	fn parse_export(&mut self, start: u32, exports: &mut FastSet<StrId>) -> Result<NodeId> {
		self.next()?;
		if let Some(node) = E::export_head(self, start)? {
			return Ok(node);
		}
		if self.eat(TokenKind::Star)? {
			let exported = if self.eat_contextual("as")? {
				let exported = self.parse_module_export_name()?;
				self.check_export(exports, exported, self.start_of(exported))?;
				Some(exported)
			} else {
				None
			};
			self.expect_contextual("from")?;
			if !matches!(self.tok.kind, TokenKind::String(_)) {
				return self.unexpected();
			}
			let source = self.parse_expr_atom(&mut None, ForInit::No, false)?;
			let attributes = self.parse_with_clause()?;
			self.semicolon()?;
			let node = self.add(
				NodeKind::ExportAllDeclaration {
					exported,
					source,
					attributes,
				},
				start,
			);
			E::export_end(self, node);
			return Ok(node);
		}
		if self.is_keyword(Keyword::Default) {
			let default_start = self.tok.start;
			self.next()?;
			let name = self.intern("default");
			self.check_export_name(exports, name, default_start)?;
			let declaration = self.parse_export_default_declaration()?;
			let node = self.add(NodeKind::ExportDefaultDeclaration { declaration }, start);
			E::export_end(self, node);
			return Ok(node);
		}
		if self.should_parse_export_statement() {
			let declaration = match E::export_declaration(self)? {
				Some(declaration) => declaration,
				None => self.parse_statement(Context::None, StatementPlace::Block, None)?,
			};
			match self.kind(declaration) {
				NodeKind::VariableDeclaration { declarations, .. } => {
					for i in 0..declarations.len {
						let decl = self.nth(declarations, i).unwrap();
						let NodeKind::VariableDeclarator { id, .. } = self.kind(decl) else {
							unreachable!()
						};
						self.check_pattern_export(exports, id)?;
					}
				}
				NodeKind::FunctionDeclaration {
					function: Function { id: Some(id), .. },
				}
				| NodeKind::ClassDeclaration {
					class: crate::ast::Class { id: Some(id), .. },
				} => {
					self.check_export(exports, id, self.start_of(id))?;
				}
				_ => {}
			}
			let node = self.add(NodeKind::ExportDeclaration { declaration }, start);
			E::export_end(self, node);
			return Ok(node);
		}
		let specifiers = self.parse_export_specifiers(exports)?;
		let (source, attributes) = if self.eat_contextual("from")? {
			if !matches!(self.tok.kind, TokenKind::String(_)) {
				return self.unexpected();
			}
			(
				Some(self.parse_expr_atom(&mut None, ForInit::No, false)?),
				self.parse_with_clause()?,
			)
		} else {
			for &spec in specifiers.iter().flatten() {
				let NodeKind::ExportSpecifier { local, .. } = self.kind(spec) else {
					unreachable!()
				};
				self.check_unreserved(local)?;
				match self.kind(local) {
					NodeKind::Identifier { name } => self.check_local_export(name, self.start_of(local)),
					_ => {
						return self.error(self.start_of(local), Code::StringExportWithoutFrom);
					}
				}
			}
			(None, List::EMPTY)
		};
		self.semicolon()?;
		let specifiers = self.list_from(specifiers);
		let node = self.add(
			NodeKind::ExportNamedDeclaration {
				specifiers,
				source,
				attributes,
			},
			start,
		);
		E::export_end(self, node);
		Ok(node)
	}

	fn parse_export_default_declaration(&mut self) -> Result<NodeId> {
		if let Some(declaration) = E::export_default(self)? {
			return Ok(declaration);
		}
		let start = self.tok.start;
		let is_async = self.is_async_function();
		if self.is_keyword(Keyword::Function) || is_async {
			self.next()?;
			if is_async {
				self.next()?;
			}
			return self.parse_function(start, FUNC_STATEMENT | FUNC_NULLABLE_ID, is_async, ForInit::No);
		}
		if self.is_keyword(Keyword::Class) {
			return self.parse_class(ClassKind::NullableId);
		}
		let declaration = self.parse_maybe_assign(ForInit::No, &mut None)?;
		self.semicolon()?;
		Ok(declaration)
	}

	pub(crate) fn should_parse_export_statement(&mut self) -> bool {
		self.is_keyword(Keyword::Var)
			|| self.is_keyword(Keyword::Const)
			|| self.is_keyword(Keyword::Class)
			|| self.is_keyword(Keyword::Function)
			|| self.is_let(Context::None)
			|| self.is_async_function()
			|| E::starts_export_declaration(self)
	}

	fn parse_export_specifiers(&mut self, exports: &mut FastSet<StrId>) -> Result<Vec<Option<NodeId>>> {
		let mut nodes = self.items();
		self.expect(TokenKind::BraceL)?;
		let mut first = true;
		while !self.eat(TokenKind::BraceR)? {
			if self.list_comma(TokenKind::BraceR, &mut first, true)? {
				break;
			}
			if let Some(node) = E::export_specifier(self)? {
				nodes.push(Some(node));
				continue;
			}
			let start = self.tok.start;
			let local = self.parse_module_export_name()?;
			let exported = if self.eat_contextual("as")? {
				self.parse_module_export_name()?
			} else {
				local
			};
			self.check_export(exports, exported, self.start_of(exported))?;
			nodes.push(Some(self.add(NodeKind::ExportSpecifier { local, exported }, start)));
		}
		Ok(nodes)
	}

	fn check_export(&self, exports: &mut FastSet<StrId>, name: NodeId, pos: u32) -> Result<()> {
		let name = match self.kind(name) {
			NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } => name,
			_ => return Ok(()),
		};
		self.check_export_name(exports, name, pos)
	}

	fn check_export_name(&self, exports: &mut FastSet<StrId>, name: StrId, pos: u32) -> Result<()> {
		if !exports.insert(name) && E::DUPLICATE_EXPORT_ERRORS {
			return self.error_name(pos, Code::DuplicateExport, name);
		}
		Ok(())
	}

	fn check_pattern_export(&self, exports: &mut FastSet<StrId>, pattern: NodeId) -> Result<()> {
		match self.kind(pattern) {
			NodeKind::Identifier { .. } => self.check_export(exports, pattern, self.start_of(pattern)),
			NodeKind::ObjectPattern { properties } => {
				for prop in self.ast.list(properties).iter().flatten() {
					self.check_pattern_export(exports, *prop)?;
				}
				Ok(())
			}
			NodeKind::ArrayPattern { elements } => {
				for element in self.ast.list(elements).iter().flatten() {
					self.check_pattern_export(exports, *element)?;
				}
				Ok(())
			}
			NodeKind::Property { value, .. } => self.check_pattern_export(exports, value),
			NodeKind::AssignmentPattern { left, .. } => self.check_pattern_export(exports, left),
			NodeKind::RestElement { argument } => self.check_pattern_export(exports, argument),
			_ => Ok(()),
		}
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassKind {
	Expression,
	Declaration,
	NullableId,
}

impl<E: Extension> Parser<'_, E> {
	pub(crate) fn parse_class(&mut self, kind: ClassKind) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		E::class_start(self, kind)?;
		let old_strict = self.strict;
		self.set_strict(true);
		let id = if matches!(self.tok.kind, TokenKind::Ident(_))
			&& !(kind == ClassKind::Expression && E::starts_class_heritage(self))
		{
			let id = self.parse_ident(false)?;
			// a class expression binds its name inside its own strict body, nowhere else
			let binding = if kind == ClassKind::Expression {
				Binding::Outside
			} else {
				Binding::Lexical
			};
			self.check_lval_simple(id, binding, &mut None)?;
			Some(id)
		} else {
			if kind == ClassKind::Declaration {
				return self.unexpected();
			}
			None
		};
		E::class_type_parameters(self)?;
		let super_class = if self.eat_keyword(Keyword::Extends)? {
			Some(self.parse_expr_subscripts(&mut None, ForInit::No)?)
		} else {
			None
		};
		E::class_heritage(self, super_class.is_some())?;
		self.private_names.push(PrivateNameScope::default());
		let body_start = self.tok.start;
		let mut body = self.items();
		let mut had_constructor = false;
		self.expect(TokenKind::BraceL)?;
		while !self.is(TokenKind::BraceR) {
			let at = self.tok.start;
			let element = self.parse_class_element(super_class.is_some())?;
			self.ensure_progress(at)?;
			let Some(element) = element else {
				continue;
			};
			body.push(Some(element));
			match self.kind(element) {
				NodeKind::MethodDefinition {
					kind: MethodKind::Constructor,
					value,
					..
				} if matches!(self.kind(value), NodeKind::FunctionExpression { .. }) => {
					if had_constructor {
						return self.error(self.start_of(element), Code::DuplicateConstructor);
					}
					had_constructor = true;
				}
				NodeKind::MethodDefinition {
					key, is_static, value, ..
				} if matches!(self.kind(key), NodeKind::PrivateIdentifier { .. })
					&& !matches!(self.kind(value), NodeKind::Extension(_)) =>
				{
					self.declare_private_element(element, key, is_static)?;
				}
				NodeKind::PropertyDefinition { key, is_static, .. }
					if matches!(self.kind(key), NodeKind::PrivateIdentifier { .. }) =>
				{
					self.declare_private_element(element, key, is_static)?;
				}
				_ => {}
			}
		}
		self.set_strict(old_strict);
		self.next()?;
		let body = self.list_from(body);
		let body = self.add(NodeKind::ClassBody { body }, body_start);
		self.exit_class_body()?;
		let class = Class { id, super_class, body };
		let kind = if kind == ClassKind::Expression {
			NodeKind::ClassExpression { class }
		} else {
			NodeKind::ClassDeclaration { class }
		};
		let node = self.add(kind, start);
		E::class_end(self, node);
		Ok(node)
	}

	fn declare_private_element(&mut self, element: NodeId, key: NodeId, is_static: bool) -> Result<()> {
		let NodeKind::PrivateIdentifier { name } = self.kind(key) else {
			unreachable!()
		};
		let private_kind = match self.kind(element) {
			NodeKind::MethodDefinition {
				kind: MethodKind::Get, ..
			} => {
				if is_static {
					PrivateKind::StaticGet
				} else {
					PrivateKind::InstanceGet
				}
			}
			NodeKind::MethodDefinition {
				kind: MethodKind::Set, ..
			} => {
				if is_static {
					PrivateKind::StaticSet
				} else {
					PrivateKind::InstanceSet
				}
			}
			_ => PrivateKind::Any,
		};
		if self.declare_private_name(name, private_kind) {
			return self.error_arg(
				self.start_of(key),
				Code::Redeclaration,
				format_args!("#{}", self.str(name)),
			);
		}
		Ok(())
	}

	fn declare_private_name(&mut self, name: StrId, kind: PrivateKind) -> bool {
		let scope = self.private_names.last_mut().unwrap();
		let Some(entry) = scope.declared.iter_mut().find(|(n, _)| *n == name) else {
			scope.declared.push((name, kind));
			return false;
		};
		let pair = |a: PrivateKind, b: PrivateKind| (entry.1 == a && kind == b) || (entry.1 == b && kind == a);
		if pair(PrivateKind::InstanceGet, PrivateKind::InstanceSet)
			|| pair(PrivateKind::StaticGet, PrivateKind::StaticSet)
		{
			entry.1 = PrivateKind::Any;
			return false;
		}
		true
	}

	fn exit_class_body(&mut self) -> Result<()> {
		let scope = self.private_names.pop().unwrap();
		for (name, pos) in scope.used {
			if scope.declared.iter().any(|(n, _)| *n == name) {
				continue;
			}
			match self.private_names.last_mut() {
				Some(parent) => parent.used.push((name, pos)),
				None => {
					return self.error_name(pos, Code::UndeclaredPrivateName, name);
				}
			}
		}
		Ok(())
	}

	fn parse_class_element(&mut self, constructor_allows_super: bool) -> Result<Option<NodeId>> {
		if self.eat(TokenKind::Semi)? {
			return Ok(None);
		}
		let start = self.tok.start;
		let mut key_name: Option<&'static str> = None;
		let mut generator = false;
		let mut is_async = false;
		let mut kind = MethodKind::Method;
		let mut is_static = E::class_modifiers(self)?;
		if let Some(signature) = E::class_index_signature(self, start)? {
			E::class_element_end(self, signature)?;
			return Ok(Some(signature));
		}
		if !is_static
			&& self.is_contextual("static")
			&& (!E::STATIC_IS_A_MODIFIER || self.peek_char().0 == Some('{'))
			&& self.eat_contextual("static")?
		{
			if self.eat(TokenKind::BraceL)? {
				let block = self.parse_class_static_block(start)?;
				E::class_element_end(self, block)?;
				return Ok(Some(block));
			}
			if self.is_class_element_name_start() || self.is(TokenKind::Star) {
				is_static = true;
			} else {
				key_name = Some("static");
			}
		}
		if key_name.is_none() && self.eat_contextual("async")? {
			if (self.is_class_element_name_start() || self.is(TokenKind::Star)) && !self.can_insert_semicolon() {
				is_async = true;
			} else {
				key_name = Some("async");
			}
		}
		if key_name.is_none() && self.eat(TokenKind::Star)? {
			generator = true;
		}
		if key_name.is_none() && !is_async && !generator {
			let accessor = if self.is_contextual("get") {
				Some(("get", MethodKind::Get))
			} else if self.is_contextual("set") {
				Some(("set", MethodKind::Set))
			} else {
				None
			};
			if let Some((name, accessor_kind)) = accessor {
				self.next()?;
				if self.is_class_element_name_start() {
					kind = accessor_kind;
				} else {
					key_name = Some(name);
				}
			}
		}
		let (key, computed) = match key_name {
			Some(name) => {
				let key_start = self.prev_end - name.len() as u32;
				let name = self.intern(name);
				(
					self.add_with_end(NodeKind::Identifier { name }, key_start, self.prev_end),
					false,
				)
			}
			None => self.parse_class_element_name()?,
		};
		E::class_key_end(self, key, computed)?;
		if self.is(TokenKind::ParenL)
			|| E::starts_class_method(self)
			|| kind != MethodKind::Method
			|| generator
			|| is_async
		{
			let is_constructor = !is_static && self.check_key_name(key, computed, "constructor");
			let allows_direct_super = is_constructor && constructor_allows_super;
			if is_constructor && kind != MethodKind::Method {
				return self.error(self.start_of(key), Code::AccessorConstructor);
			}
			if is_constructor {
				kind = MethodKind::Constructor;
			}
			let method = self.parse_class_method(
				start,
				key,
				computed,
				kind,
				is_static,
				generator,
				is_async,
				allows_direct_super,
			)?;
			return Ok(Some(method));
		}
		Ok(Some(self.parse_class_field(start, key, computed, is_static)?))
	}

	fn is_class_element_name_start(&self) -> bool {
		matches!(
			self.tok.kind,
			TokenKind::Ident(_)
				| TokenKind::PrivateName(_)
				| TokenKind::Number(_)
				| TokenKind::BigInt
				| TokenKind::String(_)
				| TokenKind::BracketL
				| TokenKind::Keyword(_)
		)
	}

	fn parse_class_element_name(&mut self) -> Result<(NodeId, bool)> {
		if let TokenKind::PrivateName(name) = self.tok.kind {
			if self.str(name) == "constructor" {
				return self.error(self.tok.start, Code::PrivateConstructor);
			}
			return Ok((self.parse_private_ident()?, false));
		}
		self.parse_property_name()
	}

	fn check_key_name(&self, key: NodeId, computed: bool, name: &str) -> bool {
		if computed {
			return false;
		}
		match self.kind(key) {
			NodeKind::Identifier { name: n } | NodeKind::StringLiteral { value: n } => self.str(n) == name,
			_ => false,
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn parse_class_method(
		&mut self,
		start: u32,
		key: NodeId,
		computed: bool,
		kind: MethodKind,
		is_static: bool,
		generator: bool,
		is_async: bool,
		allows_direct_super: bool,
	) -> Result<NodeId> {
		if kind == MethodKind::Constructor {
			if generator {
				return self.error(self.start_of(key), Code::GeneratorConstructor);
			}
			if is_async {
				return self.error(self.start_of(key), Code::AsyncConstructor);
			}
		} else if is_static && !E::in_ambient(self) && self.check_key_name(key, computed, "prototype") {
			return self.error_with(
				self.start_of(key),
				Code::StaticPrototype,
				"Classes may not have a static property named prototype",
			);
		}
		E::class_method_start(self, kind)?;
		let value = self.parse_method(generator, is_async, allows_direct_super, true, kind)?;
		if kind == MethodKind::Get || kind == MethodKind::Set {
			self.check_accessor_params(value, kind == MethodKind::Get, true)?;
		}
		let node = self.add(
			NodeKind::MethodDefinition {
				key,
				value,
				kind,
				computed,
				is_static,
			},
			start,
		);
		E::class_element_end(self, node)?;
		Ok(node)
	}

	fn parse_class_field(&mut self, start: u32, key: NodeId, computed: bool, is_static: bool) -> Result<NodeId> {
		if self.check_key_name(key, computed, "constructor") {
			return self.error(self.start_of(key), Code::ConstructorField);
		}
		if is_static && !E::in_ambient(self) && self.check_key_name(key, computed, "prototype") {
			return self.error(self.start_of(key), Code::StaticPrototype);
		}
		E::class_field_annotation(self)?;
		let value = if self.eat(TokenKind::Eq)? {
			self.enter_scope(SCOPE_CLASS_FIELD_INIT | SCOPE_SUPER);
			let value = self.parse_maybe_assign(ForInit::No, &mut None)?;
			self.exit_scope();
			Some(value)
		} else {
			None
		};
		self.semicolon()?;
		let node = self.add(
			NodeKind::PropertyDefinition {
				key,
				value,
				computed,
				is_static,
			},
			start,
		);
		E::class_element_end(self, node)?;
		Ok(node)
	}

	fn parse_class_static_block(&mut self, start: u32) -> Result<NodeId> {
		let old_labels = std::mem::take(&mut self.labels);
		self.enter_scope(SCOPE_CLASS_STATIC_BLOCK | SCOPE_SUPER);
		let mut body = self.items();
		while !self.is(TokenKind::BraceR) {
			body.push(Some(self.parse_statement(
				Context::None,
				StatementPlace::Block,
				None,
			)?));
		}
		self.next()?;
		self.exit_scope();
		self.labels = old_labels;
		let body = self.list_from(body);
		Ok(self.add(NodeKind::StaticBlock { body }, start))
	}
}
