use super::class::ClassKind;
use super::expression::ForInit;
use super::pattern::Binding;
use super::scope::{SCOPE_SIMPLE_CATCH, SCOPE_TOP, function_flags};
use super::{DestructuringErrors, Extension, FunctionKind, Label, LabelKind, Parser, Result};
use crate::ast::{Function, NodeId, NodeKind, VariableKind};
use crate::error::Code;
use crate::interner::{FastSet, StrId};
use crate::lexer::token::{Keyword, TokenKind};
use crate::lexer::unicode::is_id_start;

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
		let mut body = Vec::new();
		let mut exports = FastSet::default();
		while !self.is(TokenKind::Eof) {
			body.push(self.parse_statement(Context::None, true, Some(&mut exports))?);
		}
		if module
			&& !self.options.allow_undeclared_exports
			&& let Some((&name, &(pos, _))) = self.undeclared_exports.iter().min_by_key(|(_, (_, order))| *order)
		{
			return self.error_with(
				pos,
				Code::UndefinedExport,
				format!("Export '{}' is not defined", self.str(name)),
			);
		}
		let body = self.list_of(&body);
		self.adapt_directive_prologue(body);
		self.exit_scope();
		Ok(self.add_with_end(NodeKind::Program { body, module }, start, self.tok.end))
	}

	pub(crate) fn adapt_directive_prologue(&mut self, statements: crate::ast::List) {
		for i in 0..statements.len {
			let statement = self.ast.lists[(statements.start + i) as usize].unwrap();
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
		top_level: bool,
		exports: Option<&mut FastSet<StrId>>,
	) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_statement_inner(context, top_level, exports);
		self.leave();
		result
	}

	fn parse_statement_inner(
		&mut self,
		context: Context,
		top_level: bool,
		exports: Option<&mut FastSet<StrId>>,
	) -> Result<NodeId> {
		if let Some(statement) = E::statement(self, context, top_level)? {
			return Ok(statement);
		}
		let start = self.tok.start;
		if self.is_let(context) {
			if context != Context::None {
				return self.unexpected();
			}
			return self.parse_var_statement(start, VariableKind::Let);
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
					return self.parse_expression_statement(start, expression);
				}
				if !top_level {
					return self.error(start, Code::ImportExportNotTopLevel);
				}
				if !self.options.module {
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
				self.parse_expression_statement(start, expression)
			}
		}
	}

	/// Whether a `let` token starts a declaration rather than being an identifier.
	fn is_let(&self, context: Context) -> bool {
		if !self.is_contextual("let") {
			return false;
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
		if next == '$' || next == '_' || is_id_start(next) {
			let rest = &self.source()[pos..];
			let len = rest
				.char_indices()
				.find(|&(_, c)| {
					!(c == '$'
						|| c == '_' || crate::lexer::unicode::is_id_continue(c)
						|| c == '\u{200c}' || c == '\u{200d}')
				})
				.map_or(rest.len(), |(i, _)| i);
			if rest[len..].starts_with('\\') {
				return true;
			}
			let word = &rest[..len];
			return word != "in" && word != "instanceof";
		}
		false
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
		rest.starts_with("function")
			&& !rest[8..]
				.chars()
				.next()
				.is_some_and(|c| c == '$' || c == '_' || crate::lexer::unicode::is_id_continue(c))
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
			return self.error_with(
				start,
				Code::Unsyntactic,
				format!("Unsyntactic {}", if is_break { "break" } else { "continue" }),
			);
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
		let body = self.parse_statement(Context::Other, false, None)?;
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
		if self.is_keyword(Keyword::Var) || self.is_keyword(Keyword::Const) || is_let {
			let init_start = self.tok.start;
			let kind = if is_let {
				VariableKind::Let
			} else if self.is_keyword(Keyword::Var) {
				VariableKind::Var
			} else {
				VariableKind::Const
			};
			self.next()?;
			let init = self.parse_var(init_start, true, kind)?;
			let NodeKind::VariableDeclaration { declarations, .. } = self.kind(init) else {
				unreachable!()
			};
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
		let body = self.parse_statement(Context::Other, false, None)?;
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
				return self.error_with(
					self.start_of(left),
					Code::ForInOfInitializer,
					format!("{loop_kind} loop variable declaration may not have an initializer"),
				);
			}
		}
		let right = if is_for_in {
			self.parse_expression(false, &mut None)?
		} else {
			self.parse_maybe_assign(ForInit::No, &mut None)?
		};
		self.expect(TokenKind::ParenR)?;
		let body = self.parse_statement(Context::Other, false, None)?;
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
		let (old_yield, old_await, old_await_ident) = (self.yield_pos, self.await_pos, self.await_ident_pos);
		self.yield_pos = 0;
		self.await_pos = 0;
		self.await_ident_pos = 0;
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
		let params = self.list(&params);
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
		self.yield_pos = old_yield;
		self.await_pos = old_await;
		self.await_ident_pos = old_await_ident;
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
		let consequent = self.parse_statement(Context::If, false, None)?;
		let alternate = if self.eat_keyword(Keyword::Else)? {
			Some(self.parse_statement(Context::If, false, None)?)
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
		let mut cases = Vec::new();
		let mut current: Option<(u32, Option<NodeId>, Vec<NodeId>)> = None;
		let mut saw_default = false;
		while !self.is(TokenKind::BraceR) {
			if self.is_keyword(Keyword::Case) || self.is_keyword(Keyword::Default) {
				let is_case = self.is_keyword(Keyword::Case);
				if let Some((case_start, test, consequent)) = current.take() {
					let consequent = self.list_of(&consequent);
					cases.push(self.add(NodeKind::SwitchCase { test, consequent }, case_start));
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
				current = Some((case_start, test, Vec::new()));
			} else {
				let Some(current) = current.as_mut() else {
					return self.unexpected();
				};
				current.2.push(self.parse_statement(Context::None, false, None)?);
			}
		}
		self.exit_scope();
		if let Some((case_start, test, consequent)) = current.take() {
			let consequent = self.list_of(&consequent);
			cases.push(self.add(NodeKind::SwitchCase { test, consequent }, case_start));
		}
		self.next()?;
		self.labels.pop();
		let cases = self.list_of(&cases);
		Ok(self.add(NodeKind::SwitchStatement { discriminant, cases }, start))
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
		self.next()?;
		let node = self.parse_var(start, false, kind)?;
		self.semicolon()?;
		self.ast.node_mut(node).end = self.prev_end;
		Ok(node)
	}

	fn parse_while(&mut self, start: u32) -> Result<NodeId> {
		self.next()?;
		let test = self.parse_paren_expression()?;
		self.push_label(LabelKind::Loop);
		let body = self.parse_statement(Context::Other, false, None)?;
		self.labels.pop();
		Ok(self.add(NodeKind::WhileStatement { test, body }, start))
	}

	fn parse_with(&mut self, start: u32) -> Result<NodeId> {
		if self.strict {
			return self.error(start, Code::StrictWith);
		}
		self.next()?;
		let object = self.parse_paren_expression()?;
		let body = self.parse_statement(Context::Other, false, None)?;
		Ok(self.add(NodeKind::WithStatement { object, body }, start))
	}

	fn parse_labeled(&mut self, start: u32, name: StrId, label: NodeId, context: Context) -> Result<NodeId> {
		if self.labels.iter().any(|l| l.name == Some(name)) {
			return self.error_with(
				self.start_of(label),
				Code::DuplicateLabel,
				format!("Label '{}' is already declared", self.str(name)),
			);
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
		let body = self.parse_statement(context.with_label(), false, None)?;
		self.labels.pop();
		Ok(self.add(NodeKind::LabeledStatement { label, body }, start))
	}

	fn parse_expression_statement(&mut self, start: u32, expression: NodeId) -> Result<NodeId> {
		if matches!(self.kind(expression), NodeKind::Identifier { .. })
			&& let Some(statement) = E::expression_statement(self, start, expression)?
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
		let mut body = Vec::new();
		while !self.is(TokenKind::BraceR) {
			body.push(self.parse_statement(Context::None, false, None)?);
		}
		if exit_strict {
			self.set_strict(false);
		}
		self.next()?;
		if new_scope {
			self.exit_scope();
		}
		let body = self.list_of(&body);
		Ok(self.add(NodeKind::BlockStatement { body }, start))
	}

	fn parse_var(&mut self, start: u32, is_for: bool, kind: VariableKind) -> Result<NodeId> {
		let mut declarations = Vec::new();
		loop {
			let decl_start = self.tok.start;
			let id = self.parse_binding_atom()?;
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
				if kind == VariableKind::Const && !in_or_of && !missing_allowed {
					return self.unexpected();
				}
				if !matches!(self.kind(id), NodeKind::Identifier { .. }) && !(is_for && in_or_of) && !missing_allowed {
					return self.error(self.prev_end, Code::PatternWithoutInitializer);
				}
				None
			};
			let declarator = self.add(NodeKind::VariableDeclarator { id, init }, decl_start);
			E::var_declarator(self, declarator, kind)?;
			declarations.push(declarator);
			if !self.eat(TokenKind::Comma)? {
				break;
			}
		}
		let declarations = self.list_of(&declarations);
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
			specifiers = Vec::new();
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
		let specifiers = self.list_of(&specifiers);
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

	fn parse_import_specifiers(&mut self) -> Result<Vec<NodeId>> {
		let mut nodes = Vec::new();
		if matches!(self.tok.kind, TokenKind::Ident(_)) {
			let start = self.tok.start;
			let local = self.parse_ident(false)?;
			self.check_lval_simple(local, Binding::Lexical, &mut None)?;
			nodes.push(self.add(NodeKind::ImportDefaultSpecifier { local }, start));
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
			nodes.push(self.add(NodeKind::ImportNamespaceSpecifier { local }, start));
			return Ok(nodes);
		}
		self.expect(TokenKind::BraceL)?;
		let mut first = true;
		while !self.eat(TokenKind::BraceR)? {
			if !first {
				self.expect(TokenKind::Comma)?;
				if self.after_trailing_comma(TokenKind::BraceR, false)? {
					break;
				}
			} else {
				first = false;
			}
			if let Some(node) = E::import_specifier(self)? {
				nodes.push(node);
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
			nodes.push(self.add(NodeKind::ImportSpecifier { imported, local }, start));
		}
		Ok(nodes)
	}

	fn parse_with_clause(&mut self) -> Result<crate::ast::List> {
		let mut nodes = Vec::new();
		if !self.eat_keyword(Keyword::With)? {
			return Ok(self.list_of(&nodes));
		}
		self.expect(TokenKind::BraceL)?;
		let mut first = true;
		let mut seen: Vec<StrId> = Vec::new();
		while !self.eat(TokenKind::BraceR)? {
			if !first {
				self.expect(TokenKind::Comma)?;
				if self.after_trailing_comma(TokenKind::BraceR, false)? {
					break;
				}
			} else {
				first = false;
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
				return self.error_with(
					self.start_of(key),
					Code::DuplicateImportAttribute,
					format!("Duplicate attribute key '{}'", self.str(key_name)),
				);
			}
			seen.push(key_name);
			nodes.push(self.add(NodeKind::ImportAttribute { key, value }, start));
		}
		Ok(self.list_of(&nodes))
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
				None => self.parse_statement(Context::None, false, None)?,
			};
			match self.kind(declaration) {
				NodeKind::VariableDeclaration { declarations, .. } => {
					for i in 0..declarations.len {
						let decl = self.ast.lists[(declarations.start + i) as usize].unwrap();
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
			let specifiers = self.list_of(&[]);
			let attributes = self.list_of(&[]);
			let node = self.add(
				NodeKind::ExportNamedDeclaration {
					declaration: Some(declaration),
					specifiers,
					source: None,
					attributes,
				},
				start,
			);
			E::export_end(self, node);
			return Ok(node);
		}
		let specifiers = self.parse_export_specifiers(exports)?;
		let source;
		let attributes;
		if self.eat_contextual("from")? {
			if !matches!(self.tok.kind, TokenKind::String(_)) {
				return self.unexpected();
			}
			source = Some(self.parse_expr_atom(&mut None, ForInit::No, false)?);
			attributes = self.parse_with_clause()?;
		} else {
			for &spec in &specifiers {
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
			source = None;
			attributes = self.list_of(&[]);
		}
		self.semicolon()?;
		let specifiers = self.list_of(&specifiers);
		let node = self.add(
			NodeKind::ExportNamedDeclaration {
				declaration: None,
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

	fn parse_export_specifiers(&mut self, exports: &mut FastSet<StrId>) -> Result<Vec<NodeId>> {
		let mut nodes = Vec::new();
		self.expect(TokenKind::BraceL)?;
		let mut first = true;
		while !self.eat(TokenKind::BraceR)? {
			if !first {
				self.expect(TokenKind::Comma)?;
				if self.after_trailing_comma(TokenKind::BraceR, false)? {
					break;
				}
			} else {
				first = false;
			}
			if let Some(node) = E::export_specifier(self)? {
				nodes.push(node);
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
			nodes.push(self.add(NodeKind::ExportSpecifier { local, exported }, start));
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
			return self.error_with(
				pos,
				Code::DuplicateExport,
				format!("Duplicate export '{}'", self.str(name)),
			);
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
