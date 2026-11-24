//! Statements, declarations, functions, classes and modules.

use super::checks::{
	Binding, Label, LabelKind, PrivateKind, PrivateScope, SCOPE_CLASS_FIELD_INIT, SCOPE_CLASS_STATIC_BLOCK,
	SCOPE_DIRECT_SUPER, SCOPE_SIMPLE_CATCH, SCOPE_SUPER, SCOPE_TOP, function_flags,
};
use super::parse::{Ctx, Parser};
use super::values::{self, Escape};
use super::{ESCAPED, Kind, Token};
use crate::ast::{Class, FieldKind, Function, List, MethodKind, NodeId, NodeKind, VariableKind};
use crate::error::{Code, SyntaxError};
use crate::interner::{FastMap, StrId};

/// Where a statement stands: in a list at the top of a program, in a block, under a `case`; or
/// as the body of a label, of an `if`, of a label in an `if`, of anything else. A declaration
/// needs one of the first three.
const TOP: u8 = 0;
const BLOCK: u8 = 1;
const CASE: u8 = 2;
const LABEL: u8 = 3;
const IF: u8 = 4;
const IF_LABEL: u8 = 5;
const OTHER: u8 = 6;

/// A function whose body is being read: what its parameters are checked against once the
/// body's directives are known.
#[derive(Clone, Copy)]
pub(super) struct Head {
	pub(super) start: u32,
	pub(super) id: Option<NodeId>,
	pub(super) params: List,
	pub(super) simple: bool,
	pub(super) is_arrow: bool,
	pub(super) is_method: bool,
	pub(super) old_strict: bool,
}

impl<'a, const TS: bool> Parser<'a, TS> {
	/// The program from `start` to the end of the text, its first token at the cursor.
	pub(super) fn program_from(&mut self, start: u32) -> Option<NodeId> {
		self.enter_scope(SCOPE_TOP);
		self.body(Kind::Eof, TOP, None);
		if !self.error && TS && self.options.has(crate::parser::Options::ERASE) {
			self.mark_type_exports(0);
		}
		if !self.error && self.module && !self.options.has(crate::parser::Options::ALLOW_UNDECLARED_EXPORTS) {
			self.check_exports();
		}
		if self.error || self.lexer.error.is_some() || self.lexer.escaped_keyword.is_some() {
			return None;
		}
		let body = self.list(0);
		Some(self.ast.add(
			NodeKind::Program {
				body,
				module: self.module,
			},
			start,
			self.src.len() as u32,
		))
	}

	/// What a module exports by a name it never declares: each name once, at its last export.
	#[cold]
	#[inline(never)]
	pub(super) fn check_exports(&mut self) {
		let undeclared = std::mem::take(&mut self.scratch.checks.undeclared_exports);
		if undeclared.is_empty() {
			return;
		}
		let mut last = FastMap::default();
		for &(name, pos) in &undeclared {
			last.insert(name, pos);
		}
		for &(name, _) in &undeclared {
			if let Some(pos) = last.remove(&name)
				&& !self.declares_export(name)
			{
				let error = SyntaxError::new(pos, Code::UndefinedExport).arg(name);
				let error = self.ended(error);
				if !self.softly(error) {
					return;
				}
			}
		}
	}

	pub(super) fn set_strict(&mut self, strict: bool) {
		self.strict = strict;
		self.lexer.strict = strict;
	}

	/// The statements up to `close`, pushed; the ones that open the body may be directives, and
	/// `"use strict"` among them makes the body strict from its start.
	#[inline(always)]
	fn body(&mut self, close: Kind, place: u8, head: Option<Head>) {
		if self.recover {
			return self.body_recovering(close, place, head);
		}
		if self.tok.kind == Kind::String || head.is_some() {
			self.prologue(close, place, head);
			if self.error {
				return;
			}
		}
		while self.tok.kind != close {
			if self.tok.kind == Kind::Eof {
				self.fail();
				return;
			}
			let statement = self.statement(place);
			if self.error {
				return;
			}
			self.scratch.items.push(Some(statement));
		}
	}

	/// The directives a body opens with, pushed, and what a function's head is held to once
	/// they are known; the cursor is left on the first statement that is none.
	#[inline(never)]
	fn prologue(&mut self, close: Kind, place: u8, head: Option<Head>) {
		let base = self.scratch.items.len();
		let mut pending = head;
		while self.tok.kind == Kind::String {
			let first = self.tok;
			let statement = self.statement(place);
			if self.error {
				// the parameters are checked before the body is read
				if let Some(head) = pending.take() {
					self.check_first(|p| p.check_head(head, false));
				}
				return;
			}
			self.scratch.items.push(Some(statement));
			if !self.directive(statement, first) {
				if let Some(head) = pending.take() {
					self.check_head(head, false);
					self.err_first = self.error;
				}
				return;
			}
			if (!self.strict || pending.is_some_and(|head| !head.simple))
				&& self.lexer.error.is_none()
				&& &self.src[first.start as usize + 1..first.end as usize - 1] == "use strict"
			{
				self.use_strict(pending.take(), base);
				if self.error {
					return;
				}
			}
		}
		if let Some(head) = pending.take() {
			// the parameters are checked once the body's first token is read, before the rest
			self.check_head(head, false);
			if self.tok.kind != close {
				self.err_first = self.error && (self.scratch.items.len() > base || self.lexer.error.is_none());
			}
		}
	}

	/// `check` run as if before what was read since: an error it gives comes first.
	#[cold]
	#[inline(never)]
	fn check_first(&mut self, check: impl FnOnce(&mut Self)) {
		let held = (self.err.take(), self.err_at, self.lexer.error.take());
		self.error = false;
		check(self);
		if self.error {
			self.err_first = true;
		} else {
			(self.err, self.err_at, self.lexer.error) = held;
			self.error = true;
		}
	}

	/// What a function's parameters and name may not be, known once its body's directives are.
	pub(super) fn check_head(&mut self, head: Head, use_strict: bool) {
		if use_strict && !head.simple {
			self.raise(head.start, Code::StrictDirectiveNonSimpleParams);
			return;
		}
		self.check_params(
			head.params,
			!head.old_strict && !use_strict && !head.is_arrow && !head.is_method && head.simple,
		);
		if self.strict
			&& let Some(id) = head.id
		{
			self.check_lval_simple(id, Binding::Outside, false);
		}
	}

	/// `"use strict"` was read as the last of the directives from `base` on: the body is strict
	/// from its start, so what was read of it as sloppy code is held to that.
	#[cold]
	#[inline(never)]
	fn use_strict(&mut self, head: Option<Head>, base: usize) {
		// what the body's first token would have met comes before what the cursor stands on
		let tok = self.tok;
		self.set_strict(true);
		if let Some(head) = head {
			self.check_head(head, true);
		}
		if !self.error {
			for i in base..self.scratch.items.len() {
				let statement = self.scratch.items[i].unwrap();
				let NodeKind::ExpressionStatement { expression, .. } = self.ast.node(statement).kind else {
					continue;
				};
				let node = *self.ast.node(expression);
				if let Some((code, at)) = self.octal_in(node.start, node.end) {
					self.record(SyntaxError::new(at, code));
					break;
				}
			}
		}
		if self.error {
			return;
		}
		// the token read ahead was read as sloppy code
		match tok.kind {
			Kind::Number => {
				let bytes = &self.src.as_bytes()[tok.start as usize..tok.end as usize];
				if bytes.len() >= 2 && bytes[0] == b'0' && bytes[1].is_ascii_digit() {
					self.record(SyntaxError::new(tok.start, Code::InvalidNumber));
				}
			}
			Kind::String if tok.flags & ESCAPED != 0 => {
				if let Some((code, at)) = self.octal_in(tok.start, tok.end) {
					self.record(SyntaxError::new(at, code));
				}
			}
			_ => {}
		}
	}

	/// The first legacy octal escape of the string from `start` to `end`, quotes included.
	fn octal_in(&self, start: u32, end: u32) -> Option<(Code, u32)> {
		let bytes = self.src.as_bytes();
		let (mut pos, close) = (start as usize + 1, end as usize - 1);
		while pos < close {
			if bytes[pos] != b'\\' {
				pos += 1;
				continue;
			}
			let (escape, next) = values::escape(self.src, pos + 1);
			if let Escape::Octal(_, at, is_89) = escape {
				return Some((if is_89 { Code::StrictEscape } else { Code::StrictOctal }, at as u32));
			}
			pos = next;
		}
		None
	}

	/// Whether `statement`, which `first` starts, is a directive: a string and nothing else.
	#[cold]
	#[inline(never)]
	fn directive(&mut self, statement: NodeId, first: Token) -> bool {
		let NodeKind::ExpressionStatement { expression, .. } = self.ast.node(statement).kind else {
			return false;
		};
		let node = *self.ast.node(expression);
		if !matches!(node.kind, NodeKind::StringLiteral { .. }) || node.start != first.start || node.end != first.end {
			return false;
		}
		let raw = &self.src[first.start as usize + 1..first.end as usize - 1];
		let directive = self.ast.strings.intern(raw);
		self.ast.node_mut(statement).kind = NodeKind::ExpressionStatement {
			expression,
			directive: Some(directive),
		};
		true
	}

	#[inline(always)]
	fn can_insert_semicolon(&self) -> bool {
		matches!(self.tok.kind, Kind::Eof | Kind::BraceR) || self.newline()
	}

	#[inline(always)]
	fn semicolon(&mut self) {
		if self.tok.kind == Kind::Semi {
			self.bump(true);
		} else if !self.can_insert_semicolon() {
			self.soft_unexpected();
		}
	}

	/// The cursor back on `t`, read before the token it stands on now.
	#[cold]
	#[inline(never)]
	pub(super) fn back(&mut self, t: Token, prev_end: u32, comments: usize) {
		self.untrack();
		if self.recover {
			self.lexer.forget_after(t.end);
		}
		self.lexer.rewind(t.end as usize);
		self.lexer.comments.truncate(comments);
		self.tok = t;
		self.prev_end = prev_end;
	}

	pub(super) fn expect_word(&mut self, word: &[u8]) {
		if self.is_word(word) {
			self.bump(false);
		} else {
			self.fail();
		}
	}

	fn string_literal(&mut self) -> NodeId {
		if self.tok.kind != Kind::String {
			return self.fail();
		}
		self.atom(&mut None, false)
	}

	pub(super) fn statement(&mut self, place: u8) -> NodeId {
		if self.too_deep() {
			return self.raise(self.tok.start, Code::NestingDepth);
		}
		self.statement_inner(place)
	}

	fn push_label(&mut self, kind: LabelKind) {
		self.scratch.checks.labels.push(Label {
			name: None,
			kind,
			statement_start: 0,
		});
	}

	#[inline(always)]
	fn statement_inner(&mut self, place: u8) -> NodeId {
		if TS
			&& place <= CASE
			&& matches!(self.tok.kind, Kind::At | Kind::Ident | Kind::Const)
			&& let Some(statement) = self.ts_statement()
		{
			return statement;
		}
		let t = self.tok;
		let start = t.start;
		match t.kind {
			Kind::Ident => self.word_statement(place),
			Kind::BraceL => self.block(true),
			Kind::Semi => {
				self.bump(true);
				self.ast.add(NodeKind::EmptyStatement, start, t.end)
			}
			Kind::Var => self.var_statement(start, VariableKind::Var),
			Kind::Const => {
				if place > CASE {
					return self.fail();
				}
				self.var_statement(start, VariableKind::Const)
			}
			Kind::Function => {
				// as a body, only a plain function, only of a label or an `if`, and not in strict code
				if place > CASE && (self.strict || (place != IF && place != LABEL)) {
					return self.fail();
				}
				self.bump(false);
				self.function(start, false, true, false, place > CASE)
			}
			Kind::Class => {
				if place > CASE {
					return self.fail();
				}
				self.class(true, false)
			}
			Kind::If => self.if_statement(start),
			Kind::Return => {
				if !self.in_function()
					&& !(self.options.has(crate::parser::Options::ALLOW_RETURN_OUTSIDE_FUNCTION) && self.at_top_scope())
				{
					return self.raise(start, Code::ReturnOutsideFunction);
				}
				self.bump(true);
				let argument = if self.tok.kind == Kind::Semi {
					self.bump(true);
					None
				} else if self.can_insert_semicolon() {
					None
				} else {
					let argument = self.sequence();
					self.semicolon();
					Some(argument)
				};
				self.add(NodeKind::ReturnStatement { argument }, start)
			}
			Kind::For => self.for_statement(start),
			Kind::While => {
				self.bump(false);
				let test = self.paren_expression();
				self.push_label(LabelKind::Loop);
				let body = self.statement(OTHER);
				self.scratch.checks.labels.pop();
				self.add(NodeKind::WhileStatement { test, body }, start)
			}
			Kind::Do => {
				self.bump(true);
				self.push_label(LabelKind::Loop);
				let body = self.statement(OTHER);
				self.scratch.checks.labels.pop();
				self.expect(Kind::While, false);
				let test = self.paren_expression();
				if self.tok.kind == Kind::Semi {
					self.bump(true);
				}
				self.add(NodeKind::DoWhileStatement { body, test }, start)
			}
			Kind::Switch => self.switch_statement(start),
			Kind::Try => self.try_statement(start),
			Kind::Throw => {
				self.bump(true);
				if self.newline() {
					return self.raise(self.prev_end, Code::NewlineAfterThrow);
				}
				let argument = self.sequence();
				self.semicolon();
				self.add(NodeKind::ThrowStatement { argument }, start)
			}
			Kind::Break | Kind::Continue => self.break_continue(start, t.kind == Kind::Break),
			Kind::Debugger => {
				self.bump(true);
				self.semicolon();
				self.add(NodeKind::DebuggerStatement, start)
			}
			Kind::With => {
				if self.strict {
					return self.raise(start, Code::StrictWith);
				}
				self.bump(false);
				let object = self.paren_expression();
				let body = self.statement(OTHER);
				self.add(NodeKind::WithStatement { object, body }, start)
			}
			Kind::Import | Kind::Export => self.import_export(start, place),
			_ => self.expression_statement(start),
		}
	}

	fn break_continue(&mut self, start: u32, is_break: bool) -> NodeId {
		self.bump(false);
		// the token after is read as no regular expression: the check below comes before what it is
		let label = if self.tok.kind == Kind::Semi {
			self.bump(false);
			None
		} else if self.can_insert_semicolon() {
			None
		} else if self.tok.kind != Kind::Ident {
			return self.fail();
		} else {
			let label = self.ident();
			self.class_semicolon();
			Some(label)
		};
		if self.error {
			return NodeId::at(0);
		}
		let name = label.map(|label| match self.ast.node(label).kind {
			NodeKind::Identifier { name } => name,
			_ => unreachable!(),
		});
		let found = self.scratch.checks.labels.iter().any(|lab| {
			(name.is_none() || lab.name == name)
				&& ((lab.kind != LabelKind::None && (is_break || lab.kind == LabelKind::Loop))
					|| (label.is_some() && is_break))
		});
		if !found {
			return self.raise_arg(start, Code::Unsyntactic, if is_break { "break" } else { "continue" });
		}
		let kind = if is_break {
			NodeKind::BreakStatement { label }
		} else {
			NodeKind::ContinueStatement { label }
		};
		self.add(kind, start)
	}

	fn expression_statement(&mut self, start: u32) -> NodeId {
		let expression = self.sequence();
		self.semicolon();
		self.add(
			NodeKind::ExpressionStatement {
				expression,
				directive: None,
			},
			start,
		)
	}

	/// A statement that starts with a word: a declaration by a contextual word, a label, or an
	/// expression.
	fn word_statement(&mut self, place: u8) -> NodeId {
		let t = self.tok;
		let start = t.start;
		if t.flags & ESCAPED == 0 {
			let word = &self.src.as_bytes()[start as usize..t.end as usize];
			if matches!(word[0], b'l' | b'a' | b'u')
				&& let Some(node) = self.contextual(word, place)
			{
				return node;
			}
		}
		let expression = self.sequence();
		if self.tok.kind == Kind::Colon && matches!(self.ast.node(expression).kind, NodeKind::Identifier { .. }) {
			self.bump(true);
			return self.labeled(start, expression, place);
		}
		self.semicolon();
		self.add(
			NodeKind::ExpressionStatement {
				expression,
				directive: None,
			},
			start,
		)
	}

	#[cold]
	#[inline(never)]
	fn labeled(&mut self, start: u32, label: NodeId, place: u8) -> NodeId {
		let node = *self.ast.node(label);
		let NodeKind::Identifier { name } = node.kind else {
			unreachable!()
		};
		if self.scratch.checks.labels.iter().any(|l| l.name == Some(name)) {
			return self.raise_name(node.start, Code::DuplicateLabel, name);
		}
		let kind = match self.tok.kind {
			Kind::Do | Kind::For | Kind::While => LabelKind::Loop,
			Kind::Switch => LabelKind::Switch,
			_ => LabelKind::None,
		};
		let statement_start = self.tok.start;
		for l in self.scratch.checks.labels.iter_mut().rev() {
			if l.statement_start == start {
				l.statement_start = statement_start;
				l.kind = kind;
			} else {
				break;
			}
		}
		self.scratch.checks.labels.push(Label {
			name: Some(name),
			kind,
			statement_start,
		});
		let body = self.statement(match place {
			TOP | BLOCK | CASE | LABEL => LABEL,
			IF | IF_LABEL => IF_LABEL,
			_ => OTHER,
		});
		self.scratch.checks.labels.pop();
		self.add(NodeKind::LabeledStatement { label, body }, start)
	}

	/// After `using`, or `let` when its word is spelled with an escape: what follows names a
	/// binding.
	fn names_binding(&self) -> bool {
		let t = self.tok;
		t.kind == Kind::Ident
			|| (t.kind.is_keyword() && (t.flags & ESCAPED != 0 || !matches!(t.kind, Kind::In | Kind::Instanceof)))
	}

	/// `let`, `async function`, `using` or `await using` where a statement starts, when the word
	/// at the cursor is one of them and what follows makes it a declaration.
	#[inline(never)]
	fn contextual(&mut self, word: &[u8], place: u8) -> Option<NodeId> {
		let (t, prev, comments) = (self.tok, self.prev_end, self.lexer.comments.len());
		let start = t.start;
		let nested = place > CASE;
		match word {
			b"let" => {
				// tried: the bytes after `let` looked at, then read again as a token, 1.5% more
				// instructions on the scripts of Svelte's tests
				if !self.recover {
					self.bump(false);
					if self.lexer.error.is_none() {
						let kind = self.tok.kind;
						let escape = kind == Kind::Ident && self.src.as_bytes()[self.tok.start as usize] == b'\\';
						let binds = matches!(kind, Kind::BraceL | Kind::Ident)
							|| (kind.is_keyword() && !matches!(kind, Kind::In | Kind::Instanceof));
						if kind == Kind::BracketL || escape || (!nested && binds) {
							if nested {
								self.back(t, prev, comments);
								return Some(self.fail());
							}
							let node = self.declarators(start, VariableKind::Let, false);
							return Some(self.declared(node));
						}
						self.back(t, prev, comments);
						return None;
					}
					// what does not lex is judged by its bytes, unread
					self.lexer.error = None;
					self.back(t, prev, comments);
				}
				if (self.strict && self.recovering()) || self.lexer.declares_after_let(nested) {
					if nested {
						return Some(self.fail());
					}
					return Some(self.var_statement(start, VariableKind::Let));
				}
				return None;
			}
			b"async" => {
				if self.recover && !self.lexer.lexes_ahead(1) {
					return None;
				}
				self.bump(false);
				let function = self.tok.kind == Kind::Function && !self.newline() && self.tok.flags & ESCAPED == 0;
				if function {
					if nested {
						self.back(t, prev, comments);
						return Some(self.fail());
					}
					self.bump(false);
					return Some(self.function(start, true, true, false, false));
				}
			}
			b"using" if !nested => {
				if self.recover && !self.lexer.lexes_ahead(1) {
					return None;
				}
				self.bump(false);
				let declares = self.names_binding() && !self.newline();
				self.back(t, prev, comments);
				if declares {
					return Some(self.using_statement(start, VariableKind::Using, place));
				}
				return None;
			}
			b"await" if !nested && self.can_await_here() => {
				if self.recover && (self.lexer.peek_byte() != Some(b'u') || !self.lexer.lexes_ahead(2)) {
					return None;
				}
				self.bump(false);
				let mut declares = false;
				if self.is_word(b"using") && !self.newline() {
					self.bump(false);
					declares = self.names_binding() && !self.newline();
				}
				self.back(t, prev, comments);
				if declares {
					return Some(self.using_statement(start, VariableKind::AwaitUsing, place));
				}
				return None;
			}
			_ => return None,
		}
		self.back(t, prev, comments);
		None
	}

	fn using_statement(&mut self, start: u32, kind: VariableKind, place: u8) -> NodeId {
		if place == CASE || (place == TOP && !self.module && !(TS && self.exports_in_script())) {
			let error = self.error_arg(start, Code::UsingOutsideBlock, kind.as_str());
			if !self.softly(error) {
				return NodeId::at(0);
			}
		}
		self.var_statement(start, kind)
	}

	/// A block, its `{` at the cursor; `scoped` unless what it is the body of holds its names.
	fn block(&mut self, scoped: bool) -> NodeId {
		let start = self.tok.start;
		if self.tok.kind != Kind::BraceL {
			return self.fail();
		}
		self.bump(true);
		if scoped {
			self.enter_scope(0);
		}
		let base = self.scratch.items.len();
		if self.recover {
			self.statements_recovering(Kind::BraceR, BLOCK);
			if self.error {
				return NodeId::at(0);
			}
		}
		while self.tok.kind != Kind::BraceR {
			let statement = self.statement(BLOCK);
			if self.error {
				return statement;
			}
			self.scratch.items.push(Some(statement));
		}
		self.bump(true);
		if scoped {
			self.exit_scope();
		}
		let body = self.list(base);
		self.add(NodeKind::BlockStatement { body }, start)
	}

	fn paren_expression(&mut self) -> NodeId {
		self.expect(Kind::ParenL, true);
		let expression = self.sequence();
		self.expect(Kind::ParenR, false);
		expression
	}

	fn if_statement(&mut self, start: u32) -> NodeId {
		self.bump(false);
		let test = self.paren_expression();
		let consequent = self.statement(IF);
		let alternate = if self.tok.kind == Kind::Else {
			self.bump(true);
			Some(self.statement(IF))
		} else {
			None
		};
		self.add(
			NodeKind::IfStatement {
				test,
				consequent,
				alternate,
			},
			start,
		)
	}

	fn switch_statement(&mut self, start: u32) -> NodeId {
		self.bump(false);
		let discriminant = self.paren_expression();
		self.expect(Kind::BraceL, false);
		self.push_label(LabelKind::Switch);
		self.enter_scope(0);
		let base = self.scratch.items.len();
		let mut current: Option<(u32, Option<NodeId>, usize)> = None;
		let mut defaulted = false;
		while self.tok.kind != Kind::BraceR && !self.error {
			if self.tok.kind == Kind::Case || self.tok.kind == Kind::Default {
				if let Some((at, test, from)) = current.take() {
					let consequent = self.list(from);
					let case = self.add(NodeKind::SwitchCase { test, consequent }, at);
					self.scratch.items.push(Some(case));
				}
				let at = self.tok.start;
				let is_case = self.tok.kind == Kind::Case;
				self.bump(true);
				let test = if is_case {
					Some(self.sequence())
				} else {
					if std::mem::replace(&mut defaulted, true) {
						return self.raise(at, Code::DuplicateDefault);
					}
					None
				};
				self.expect(Kind::Colon, true);
				current = Some((at, test, self.scratch.items.len()));
			} else {
				if current.is_none() {
					return self.fail();
				}
				let statement = self.statement(CASE);
				self.scratch.items.push(Some(statement));
			}
		}
		if self.error {
			return NodeId::at(0);
		}
		self.exit_scope();
		if let Some((at, test, from)) = current.take() {
			let consequent = self.list(from);
			let case = self.add(NodeKind::SwitchCase { test, consequent }, at);
			self.scratch.items.push(Some(case));
		}
		self.bump(true);
		self.scratch.checks.labels.pop();
		let cases = self.list(base);
		self.add(NodeKind::SwitchStatement { discriminant, cases }, start)
	}

	fn try_statement(&mut self, start: u32) -> NodeId {
		self.bump(false);
		let block = self.block(true);
		let mut handler = None;
		if self.tok.kind == Kind::Catch {
			let at = self.tok.start;
			self.bump(false);
			let param = if self.tok.kind == Kind::ParenL {
				self.bump(false);
				let param = self.binding_atom();
				if self.error {
					return param;
				}
				let simple = matches!(self.ast.node(param).kind, NodeKind::Identifier { .. });
				self.enter_scope(if simple { SCOPE_SIMPLE_CATCH } else { 0 });
				self.check_lval_pattern(
					param,
					if simple { Binding::SimpleCatch } else { Binding::Lexical },
					false,
				);
				if TS {
					self.catch_param(param);
				}
				self.expect(Kind::ParenR, false);
				Some(param)
			} else {
				self.enter_scope(0);
				None
			};
			let body = self.block(false);
			self.exit_scope();
			handler = Some(self.add(NodeKind::CatchClause { param, body }, at));
		}
		let finalizer = if self.tok.kind == Kind::Finally {
			self.bump(false);
			Some(self.block(true))
		} else {
			None
		};
		if handler.is_none() && finalizer.is_none() {
			return self.raise(start, Code::MissingCatchOrFinally);
		}
		self.add(
			NodeKind::TryStatement {
				block,
				handler,
				finalizer,
			},
			start,
		)
	}

	fn for_statement(&mut self, start: u32) -> NodeId {
		self.bump(false);
		let mut await_at = None;
		if self.is_word(b"await") && self.can_await_here() {
			self.bump(false);
			await_at = Some(self.prev_end - 5);
		}
		self.push_label(LabelKind::Loop);
		self.enter_scope(0);
		self.expect(Kind::ParenL, true);
		if self.tok.kind == Kind::Semi {
			if let Some(pos) = await_at {
				return self.raise(pos, Code::UnexpectedToken);
			}
			return self.for_rest(start, None);
		}
		let init_start = self.tok.start;
		let mut kind = match self.tok.kind {
			Kind::Var => Some(VariableKind::Var),
			Kind::Const => Some(VariableKind::Const),
			_ => None,
		};
		if kind.is_none() && self.tok.kind == Kind::Ident && self.tok.flags & ESCAPED == 0 {
			let (t, prev, comments) = (self.tok, self.prev_end, self.lexer.comments.len());
			if self.is_word(b"let") {
				if (self.strict && self.recovering()) || self.lexer.declares_after_let(false) {
					kind = Some(VariableKind::Let);
				}
			} else if self.is_word(b"using") && !(self.recover && !self.lexer.lexes_ahead(2)) {
				self.bump(false);
				if self.names_binding() && !self.newline() {
					if self.is_word(b"of") {
						// `using of` declares `of` only when what follows cannot be a loop's subject
						self.bump(false);
						if matches!(self.tok.kind, Kind::Semi | Kind::Colon | Kind::Eq | Kind::Bang) {
							kind = Some(VariableKind::Using);
						}
					} else {
						kind = Some(VariableKind::Using);
					}
				}
			} else if self.is_word(b"await")
				&& self.can_await_here()
				&& !(self.recover && (self.lexer.peek_byte() != Some(b'u') || !self.lexer.lexes_ahead(3)))
			{
				self.bump(false);
				if self.is_word(b"using") && !self.newline() {
					self.bump(false);
					if self.names_binding() && !self.newline() {
						kind = Some(VariableKind::AwaitUsing);
					}
				}
			}
			if self.tok != t {
				self.back(t, prev, comments);
			}
		}
		if let Some(kind) = kind {
			let init = self.var(init_start, kind, true);
			if self.error {
				return init;
			}
			let NodeKind::VariableDeclaration { declarations, .. } = self.ast.node(init).kind else {
				unreachable!()
			};
			let of = self.is_word(b"of");
			if kind == VariableKind::Var && of {
				self.check_for_of_var(declarations);
				if self.error {
					return init;
				}
			}
			if (self.tok.kind == Kind::In || of) && declarations.len == 1 {
				let is_await = if self.tok.kind == Kind::In {
					if let Some(pos) = await_at {
						return self.raise(pos, Code::UnexpectedToken);
					}
					false
				} else {
					await_at.is_some()
				};
				return self.for_in(start, init, is_await);
			}
			if let Some(pos) = await_at {
				return self.raise(pos, Code::UnexpectedToken);
			}
			return self.for_rest(start, Some(init));
		}
		let starts_with_let = self.is_word(b"let");
		let escaped = self.tok.flags & ESCAPED != 0;
		let mut errors = Some(super::checks::Facts::default());
		let init_pos = self.tok.start;
		let init = if await_at.is_some() {
			// the subject of a `for await` is a left-hand side: no arrow function starts it
			self.arrow_at = u32::MAX;
			self.ctx.no_in = true;
			let atom = self.atom(&mut errors, false);
			let init = if self.error || std::mem::take(&mut self.bare_arrow) {
				atom
			} else {
				self.suffixes(atom, init_pos, true)
			};
			self.ctx.no_in = false;
			init
		} else {
			self.ctx.no_in = true;
			let init = self.sequence_with(&mut errors);
			self.ctx.no_in = false;
			init
		};
		if self.error {
			return init;
		}
		let is_for_of = self.is_word(b"of");
		if self.tok.kind == Kind::In || is_for_of {
			let mut is_await = false;
			if let Some(pos) = await_at {
				if self.tok.kind == Kind::In {
					return self.raise(pos, Code::UnexpectedToken);
				}
				is_await = true;
			} else if is_for_of
				&& self.ast.node(init).start == init_pos
				&& !escaped && self.ident_is(init, "async")
			{
				return self.fail();
			}
			if starts_with_let && is_for_of {
				let at = self.ast.node(init).start;
				return self.raise(at, Code::ForOfLet);
			}
			let held = errors.unwrap();
			if held.owner != Some(init) && held.pending() {
				self.raise_expression_errors(&held);
				return init;
			}
			let init = self.make_pattern(init, Some(held));
			self.check_lval_pattern(init, Binding::None, false);
			if self.error {
				return init;
			}
			return self.for_in(start, init, is_await);
		}
		let held = errors.unwrap();
		if held.pending() {
			self.raise_expression_errors(&held);
			return init;
		}
		if let Some(pos) = await_at {
			return self.raise(pos, Code::UnexpectedToken);
		}
		self.for_rest(start, Some(init))
	}

	/// Annex B.3.4 lets a var redeclare the parameter of a simple catch clause, except the
	/// binding of a for-of.
	#[cold]
	#[inline(never)]
	fn check_for_of_var(&mut self, declarations: List) {
		let mut stack: Vec<Option<NodeId>> = (0..declarations.len).map(|i| self.ast.nth(declarations, i)).collect();
		while let Some(item) = stack.pop() {
			let Some(id) = item else { continue };
			let node = *self.ast.node(id);
			match node.kind {
				NodeKind::Identifier { name } => {
					if self.rebinds_catch_param(name) {
						self.raise_name(node.start, Code::Redeclaration, name);
						return;
					}
				}
				NodeKind::VariableDeclarator { id, .. } => stack.push(Some(id)),
				NodeKind::ObjectPattern { properties: list } | NodeKind::ArrayPattern { elements: list } => {
					stack.extend((0..list.len).map(|i| self.ast.nth(list, i)));
				}
				NodeKind::Property { value, .. } => stack.push(Some(value)),
				NodeKind::RestElement { argument } => stack.push(Some(argument)),
				NodeKind::AssignmentPattern { left, .. } => stack.push(Some(left)),
				_ => {}
			}
		}
	}

	fn for_rest(&mut self, start: u32, init: Option<NodeId>) -> NodeId {
		self.expect(Kind::Semi, true);
		let test = if self.tok.kind == Kind::Semi {
			None
		} else {
			Some(self.sequence())
		};
		self.expect(Kind::Semi, true);
		let update = if self.tok.kind == Kind::ParenR {
			None
		} else {
			Some(self.sequence())
		};
		self.expect(Kind::ParenR, false);
		let body = self.statement(OTHER);
		self.exit_scope();
		self.scratch.checks.labels.pop();
		self.add(
			NodeKind::ForStatement {
				init,
				test,
				update,
				body,
			},
			start,
		)
	}

	fn for_in(&mut self, start: u32, left: NodeId, is_await: bool) -> NodeId {
		let is_in = self.tok.kind == Kind::In;
		self.bump(true);
		let node = *self.ast.node(left);
		if let NodeKind::VariableDeclaration { declarations, kind } = node.kind {
			if is_in && kind.is_using() {
				let error = self.error_arg(node.start, Code::UsingInForIn, kind.as_str());
				if !self.softly(error) {
					return NodeId::at(0);
				}
			}
			let NodeKind::VariableDeclarator { id, init } = self.ast.node(self.ast.nth(declarations, 0).unwrap()).kind else {
				unreachable!()
			};
			// only `for (var name = value in …)` outside strict code keeps an initializer
			if init.is_some()
				&& (!is_in
					|| self.strict || kind != VariableKind::Var
					|| !matches!(self.ast.node(id).kind, NodeKind::Identifier { .. }))
			{
				return self.raise_arg(node.start, Code::ForInOfInitializer, if is_in { "for-in" } else { "for-of" });
			}
		}
		let right = if is_in { self.sequence() } else { self.assign() };
		self.expect(Kind::ParenR, false);
		let body = self.statement(OTHER);
		self.exit_scope();
		self.scratch.checks.labels.pop();
		let kind = if is_in {
			NodeKind::ForInStatement { left, right, body }
		} else {
			NodeKind::ForOfStatement {
				left,
				right,
				body,
				is_await,
			}
		};
		self.add(kind, start)
	}

	// ── declarations

	/// A declaration, its keyword at the cursor and at `start`.
	pub(super) fn var_statement(&mut self, start: u32, kind: VariableKind) -> NodeId {
		let node = self.var(start, kind, false);
		self.declared(node)
	}

	/// The end of a declaration statement whose declarators are `node`.
	fn declared(&mut self, node: NodeId) -> NodeId {
		self.semicolon();
		if !self.error {
			self.ast.node_mut(node).end = self.prev_end;
		}
		node
	}

	/// The declarators after the keyword at the cursor.
	fn var(&mut self, start: u32, kind: VariableKind, in_for: bool) -> NodeId {
		if TS && kind.is_using() && self.in_ambient() {
			let error = self.error_arg(start, Code::UsingInAmbient, kind.as_str());
			if !self.softly(error) {
				return NodeId::at(0);
			}
		}
		self.bump(false);
		if kind == VariableKind::AwaitUsing {
			self.bump(false);
		}
		self.declarators(start, kind, in_for)
	}

	/// The declarators at the cursor, their keyword read.
	fn declarators(&mut self, start: u32, kind: VariableKind, in_for: bool) -> NodeId {
		let base = self.scratch.items.len();
		loop {
			let at = self.tok.start;
			let id = self.binding_atom();
			if self.error {
				return id;
			}
			let named = matches!(self.ast.node(id).kind, NodeKind::Identifier { .. });
			if kind.is_using() && !named {
				let error = self.error_arg(at, Code::UsingPattern, kind.as_str());
				if !self.softly(error) {
					return NodeId::at(0);
				}
			}
			let binding = if kind == VariableKind::Var {
				Binding::Var
			} else {
				Binding::Lexical
			};
			if named {
				self.check_lval_simple(id, binding, false);
			} else {
				self.check_lval_pattern(id, binding, false);
			}
			if self.error {
				return id;
			}
			let definite = TS && self.var_id(id);
			if self.error {
				return id;
			}
			let init = if self.tok.kind == Kind::Eq {
				self.bump(true);
				let no_in = std::mem::replace(&mut self.ctx.no_in, in_for);
				let init = self.assign();
				self.ctx.no_in = no_in;
				Some(init)
			} else {
				// a constant, a resource and a pattern are given a value, but for what a loop iterates
				let in_or_of = self.tok.kind == Kind::In || self.is_word(b"of");
				let missing_allowed = TS && self.in_ambient();
				if kind.is_using() && !(in_for && in_or_of) && !missing_allowed {
					let error = self.error_arg(self.prev_end, Code::UsingWithoutInitializer, kind.as_str());
					if !self.softly(error) {
						return NodeId::at(0);
					}
				} else if kind == VariableKind::Const && !in_or_of && !missing_allowed {
					if !self.soft_unexpected() {
						return NodeId::at(0);
					}
				} else if !named && !(in_for && in_or_of) && !missing_allowed {
					let error = self.error_at(self.prev_end, Code::PatternWithoutInitializer);
					if !self.softly(error) {
						return NodeId::at(0);
					}
				}
				None
			};
			let declarator = self.add(NodeKind::VariableDeclarator { id, init }, at);
			if TS {
				self.var_declarator(declarator, kind, definite);
			}
			self.scratch.items.push(Some(declarator));
			if self.tok.kind != Kind::Comma || self.error {
				break;
			}
			self.bump(false);
		}
		let declarations = self.list(base);
		self.add(NodeKind::VariableDeclaration { declarations, kind }, start)
	}

	pub(super) fn binding_atom(&mut self) -> NodeId {
		match self.tok.kind {
			Kind::BracketL => {
				if self.too_deep() {
					return self.raise(self.tok.start, Code::NestingDepth);
				}
				let start = self.tok.start;
				self.bump(false);
				let no_in = std::mem::replace(&mut self.ctx.no_in, false);
				let elements = self.binding_list(Kind::BracketR, true);
				self.ctx.no_in = no_in;
				self.add(NodeKind::ArrayPattern { elements }, start)
			}
			Kind::BraceL => {
				if self.too_deep() {
					return self.raise(self.tok.start, Code::NestingDepth);
				}
				let no_in = std::mem::replace(&mut self.ctx.no_in, false);
				let node = self.object_pattern();
				self.ctx.no_in = no_in;
				node
			}
			_ => self.ident(),
		}
	}

	/// A binding with its default, when it has one.
	fn binding_element(&mut self) -> NodeId {
		let start = self.tok.start;
		let left = self.binding_atom();
		if self.tok.kind != Kind::Eq || self.error {
			return left;
		}
		self.bump(true);
		let right = self.assign();
		self.add(NodeKind::AssignmentPattern { left, right }, start)
	}

	fn binding_list(&mut self, close: Kind, holes: bool) -> List {
		let base = self.scratch.items.len();
		self.binding_items(close, holes);
		self.list(base)
	}

	/// The bindings after an opening bracket, through `close`, pushed.
	pub(super) fn binding_items(&mut self, close: Kind, holes: bool) {
		let allow_modifiers = std::mem::take(&mut self.modifiers_ok);
		let mut first = true;
		loop {
			if self.tok.kind == close {
				self.bump(false);
				break;
			}
			if !std::mem::take(&mut first) {
				self.expect(Kind::Comma, false);
				if self.error {
					break;
				}
			}
			if holes && self.tok.kind == Kind::Comma {
				self.scratch.items.push(None);
				continue;
			}
			if self.tok.kind == close {
				self.bump(false);
				break;
			}
			if close == Kind::ParenR && TS {
				let item = self.parameter(allow_modifiers);
				self.scratch.items.push(Some(item));
				if self.error {
					break;
				}
				if matches!(self.ast.node(item).kind, NodeKind::RestElement { .. }) {
					if self.tok.kind == Kind::Comma {
						self.raise(self.tok.start, Code::CommaAfterRest);
						break;
					}
					self.expect(close, false);
					break;
				}
				continue;
			}
			if self.tok.kind == Kind::Ellipsis {
				let start = self.tok.start;
				self.bump(false);
				let argument = self.binding_atom();
				let rest = self.add(NodeKind::RestElement { argument }, start);
				self.scratch.items.push(Some(rest));
				if self.error {
					break;
				}
				if self.tok.kind == Kind::Comma {
					self.raise(self.tok.start, Code::CommaAfterRest);
					break;
				}
				self.expect(close, false);
				break;
			}
			let item = self.binding_element();
			self.scratch.items.push(Some(item));
			if self.error {
				break;
			}
		}
	}

	fn object_pattern(&mut self) -> NodeId {
		let start = self.tok.start;
		self.bump(false);
		let base = self.scratch.items.len();
		let mut first = true;
		while self.tok.kind != Kind::BraceR {
			if !std::mem::take(&mut first) {
				self.expect(Kind::Comma, false);
				if self.tok.kind == Kind::BraceR || self.error {
					break;
				}
			}
			let t = self.tok;
			if t.kind == Kind::Ellipsis {
				self.bump(false);
				let argument = self.ident();
				if self.error {
					return argument;
				}
				if self.tok.kind == Kind::Comma {
					return self.raise(self.tok.start, Code::CommaAfterRest);
				}
				let rest = self.add(NodeKind::RestElement { argument }, t.start);
				self.scratch.items.push(Some(rest));
				continue;
			}
			let (key, computed) = self.property_name();
			if self.error {
				return key;
			}
			// TypeScript reads a method's type parameters here, and a pattern holds no method
			if TS && self.tok.kind == Kind::Lt {
				return self.fail();
			}
			let (value, shorthand) = if self.tok.kind == Kind::Colon {
				self.bump(false);
				(self.binding_element(), false)
			} else if !computed
				&& self.tok.kind != Kind::ParenL
				&& matches!(self.ast.node(key).kind, NodeKind::Identifier { .. })
			{
				if t.kind != Kind::Ident || self.special(t) {
					self.check_unreserved(key);
					if self.error {
						return key;
					}
				}
				let NodeKind::Identifier { name } = self.ast.node(key).kind else {
					unreachable!()
				};
				let copy = self.ast.add(NodeKind::Identifier { name }, t.start, self.ast.node(key).end);
				if self.tok.kind == Kind::Eq {
					self.bump(true);
					let right = self.assign();
					(self.add(NodeKind::AssignmentPattern { left: copy, right }, t.start), true)
				} else {
					(copy, true)
				}
			} else {
				return self.fail();
			};
			let property = self.add(
				NodeKind::Property {
					key,
					value,
					kind: crate::ast::PropertyKind::Init,
					computed,
					method: false,
					shorthand,
				},
				t.start,
			);
			self.scratch.items.push(Some(property));
			if self.error {
				return property;
			}
		}
		self.bump(false);
		let properties = self.list(base);
		self.add(NodeKind::ObjectPattern { properties }, start)
	}

	// ── functions

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

	/// A function after its `function`, which stands at `start` or after an `async` there;
	/// `hanging` as the body of a label or an `if`, where it binds nothing and is no generator.
	pub(super) fn function(
		&mut self,
		start: u32,
		is_async: bool,
		declaration: bool,
		nullable: bool,
		hanging: bool,
	) -> NodeId {
		if self.tok.kind == Kind::Star && hanging {
			return self.fail();
		}
		let generator = self.tok.kind == Kind::Star;
		if generator {
			self.bump(false);
		}
		let mut id = None;
		// a declaration's name is bound outside it, an expression's inside
		if declaration && (!nullable || self.tok.kind == Kind::Ident) {
			let name = self.ident();
			if self.error {
				return name;
			}
			// declared afterwards in TypeScript: an overload without a body would redeclare its implementation
			if !hanging && !TS {
				let binding = self.function_binding(generator, is_async);
				self.check_lval_simple(name, binding, false);
				if self.error {
					return name;
				}
			}
			id = Some(name);
		}
		let old = self.take_yield_await();
		self.enter_scope(function_flags(is_async, generator));
		let outer = self.ctx;
		self.ctx = Ctx {
			generator,
			can_await: is_async,
			no_in: false,
		};
		if !declaration && self.tok.kind == Kind::Ident {
			id = Some(self.ident());
		}
		if TS {
			return self.function_ts(start, id, is_async, generator, declaration, hanging, outer, old);
		}
		self.expect(Kind::ParenL, false);
		let params = self.binding_list(Kind::ParenR, false);
		if !self.error {
			self.check_yield_await_in_default_params();
		}
		let body = self.function_body(start, id, params, false, false);
		self.ctx = outer;
		self.restore_yield_await(old);
		let function = Function {
			id,
			params,
			body,
			is_async,
			generator,
		};
		let kind = if declaration {
			NodeKind::FunctionDeclaration { function }
		} else {
			NodeKind::FunctionExpression { function }
		};
		self.add(kind, start)
	}

	/// A method's function, its `(` at the cursor.
	pub(super) fn method(&mut self, generator: bool, is_async: bool, direct_super: bool, in_class: bool) -> NodeId {
		if TS {
			return self.method_ts(generator, is_async, direct_super, in_class, MethodKind::Method);
		}
		let old = self.take_yield_await();
		self.enter_scope(
			function_flags(is_async, generator) | SCOPE_SUPER | if direct_super { SCOPE_DIRECT_SUPER } else { 0 },
		);
		let start = self.tok.start;
		let outer = self.ctx;
		self.ctx = Ctx {
			generator,
			can_await: is_async,
			no_in: false,
		};
		self.expect(Kind::ParenL, false);
		let params = self.binding_list(Kind::ParenR, false);
		if !self.error {
			self.check_yield_await_in_default_params();
		}
		let body = self.function_body(start, None, params, false, true);
		self.ctx = outer;
		self.restore_yield_await(old);
		let function = Function {
			id: None,
			params,
			body,
			is_async,
			generator,
		};
		self.add(NodeKind::FunctionExpression { function }, start)
	}

	/// A function's body after its parameters, in the function's scope, which it closes: a
	/// block, or an arrow function's expression. What `"use strict"` in it says holds to its end.
	pub(super) fn function_body(
		&mut self,
		start: u32,
		id: Option<NodeId>,
		params: List,
		is_arrow: bool,
		is_method: bool,
	) -> NodeId {
		if self.error {
			return NodeId::at(0);
		}
		let old_strict = self.strict;
		if is_arrow && self.tok.kind != Kind::BraceL {
			let body = self.assign();
			if !self.error {
				self.check_params(params, false);
			}
			self.exit_scope();
			return body;
		}
		let simple = self.is_simple_param_list(params);
		let block_start = self.tok.start;
		if self.tok.kind != Kind::BraceL {
			return self.fail();
		}
		self.bump(true);
		let old_labels = std::mem::take(&mut self.scratch.checks.labels);
		let base = self.scratch.items.len();
		self.body(
			Kind::BraceR,
			BLOCK,
			Some(Head {
				start,
				id,
				params,
				simple,
				is_arrow,
				is_method,
				old_strict,
			}),
		);
		self.scratch.checks.labels = old_labels;
		if self.error {
			return NodeId::at(0);
		}
		self.set_strict(old_strict);
		self.bump(false);
		self.exit_scope();
		let body = self.list(base);
		self.add(NodeKind::BlockStatement { body }, block_start)
	}

	// ── classes

	/// A class, `class` at the cursor.
	pub(super) fn class(&mut self, declaration: bool, nullable: bool) -> NodeId {
		let start = self.tok.start;
		self.bump(false);
		if TS {
			self.class_start(if !declaration {
				super::ts::ClassKind::Expression
			} else if nullable {
				super::ts::ClassKind::NullableId
			} else {
				super::ts::ClassKind::Declaration
			});
		}
		let old_strict = self.strict;
		self.set_strict(true);
		let id = if self.tok.kind == Kind::Ident && !(!declaration && TS && self.starts_class_heritage()) {
			let id = self.ident();
			// a class expression binds its name inside its own strict body, nowhere else
			self.check_lval_simple(
				id,
				if declaration { Binding::Lexical } else { Binding::Outside },
				false,
			);
			if self.error {
				return id;
			}
			Some(id)
		} else {
			if declaration && !nullable {
				return self.fail();
			}
			None
		};
		if TS {
			self.class_type_parameters();
			if self.error {
				return NodeId::at(0);
			}
		}
		let super_class = if self.tok.kind == Kind::Extends {
			if self.too_deep() {
				return self.raise(self.tok.start, Code::NestingDepth);
			}
			self.bump(true);
			self.arrow_at = u32::MAX;
			let at = self.tok.start;
			let atom = self.atom(&mut None, false);
			if self.error {
				return atom;
			}
			Some(if std::mem::take(&mut self.bare_arrow) {
				atom
			} else {
				self.suffixes(atom, at, true)
			})
		} else {
			None
		};
		if self.error {
			return NodeId::at(0);
		}
		if TS {
			self.class_heritage(super_class.is_some());
			if self.error {
				return NodeId::at(0);
			}
		}
		self.scratch.checks.private_names.push(PrivateScope::default());
		let body_start = self.tok.start;
		self.expect(Kind::BraceL, false);
		let base = self.scratch.items.len();
		let mut had_constructor = false;
		while self.tok.kind != Kind::BraceR && !self.error {
			if self.tok.kind == Kind::Semi {
				self.bump(false);
				continue;
			}
			let at = self.tok.start;
			let element = if TS {
				match self.ts_class_element(super_class.is_some()) {
					Some(element) => element,
					None => {
						if self.recover && !self.ensure_progress(at) {
							break;
						}
						continue;
					}
				}
			} else {
				self.class_element(super_class.is_some())
			};
			if self.error || (self.recover && !self.ensure_progress(at)) {
				break;
			}
			self.scratch.items.push(Some(element));
			let node = *self.ast.node(element);
			match node.kind {
				// a signature without a body, TypeScript's, is no constructor of its own nor a declaration
				NodeKind::MethodDefinition { value, .. }
					if TS && !matches!(self.ast.node(value).kind, NodeKind::FunctionExpression { .. }) => {}
				NodeKind::MethodDefinition {
					kind: MethodKind::Constructor,
					..
				} => {
					if std::mem::replace(&mut had_constructor, true) {
						return self.raise(node.start, Code::DuplicateConstructor);
					}
				}
				NodeKind::MethodDefinition { key, is_static, kind, .. }
					if matches!(self.ast.node(key).kind, NodeKind::PrivateIdentifier { .. }) =>
				{
					self.declare_private(key, is_static, kind);
				}
				NodeKind::PropertyDefinition { key, is_static, .. }
					if matches!(self.ast.node(key).kind, NodeKind::PrivateIdentifier { .. }) =>
				{
					self.declare_private(key, is_static, MethodKind::Method);
				}
				_ => {}
			}
		}
		if self.error {
			return NodeId::at(0);
		}
		self.set_strict(old_strict);
		self.bump(false);
		let body = self.list(base);
		let body = self.add(NodeKind::ClassBody { body }, body_start);
		self.exit_class_body();
		let class = Class { id, super_class, body };
		let kind = if declaration {
			NodeKind::ClassDeclaration { class }
		} else {
			NodeKind::ClassExpression { class }
		};
		let node = self.add(kind, start);
		if TS {
			self.class_end(node);
		}
		node
	}

	fn declare_private(&mut self, key: NodeId, is_static: bool, kind: MethodKind) {
		let node = *self.ast.node(key);
		let NodeKind::PrivateIdentifier { name } = node.kind else {
			unreachable!()
		};
		let kind = match (kind, is_static) {
			(MethodKind::Get, false) => PrivateKind::Get,
			(MethodKind::Get, true) => PrivateKind::StaticGet,
			(MethodKind::Set, false) => PrivateKind::Set,
			(MethodKind::Set, true) => PrivateKind::StaticSet,
			_ => PrivateKind::Other,
		};
		let scope = self.scratch.checks.private_names.last_mut().unwrap();
		let redeclared = match scope.declared.iter_mut().find(|(n, _)| *n == name) {
			None => {
				scope.declared.push((name, kind));
				false
			}
			Some(entry) => {
				let pair = |a: PrivateKind, b: PrivateKind| (entry.1 == a && kind == b) || (entry.1 == b && kind == a);
				if pair(PrivateKind::Get, PrivateKind::Set) || pair(PrivateKind::StaticGet, PrivateKind::StaticSet) {
					entry.1 = PrivateKind::Other;
					false
				} else {
					true
				}
			}
		};
		if redeclared {
			let text = format!("#{}", self.ast.strings.get(name));
			self.raise_arg(node.start, Code::Redeclaration, &text);
		}
	}

	fn exit_class_body(&mut self) {
		let scope = self.scratch.checks.private_names.pop().unwrap();
		for (name, pos) in scope.used {
			if scope.declared.iter().any(|(n, _)| *n == name) {
				continue;
			}
			match self.scratch.checks.private_names.last_mut() {
				Some(parent) => parent.used.push((name, pos)),
				None => {
					let text = format!("#{}", self.ast.strings.get(name));
					self.raise_arg(pos, Code::UndeclaredPrivateName, &text);
					return;
				}
			}
		}
	}

	pub(super) fn at_class_name(&self) -> bool {
		self.tok.kind == Kind::PrivateName || self.at_property_name()
	}

	pub(super) fn key_is(&self, key: NodeId, computed: bool, word: &str) -> bool {
		if computed {
			return false;
		}
		match self.ast.node(key).kind {
			NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } => self.ast.strings.is(name, word),
			_ => false,
		}
	}

	fn class_element(&mut self, extends: bool) -> NodeId {
		let start = self.tok.start;
		// a word read as a modifier that turned out to be the member's name
		let mut named: Option<Token> = None;
		let (mut generator, mut is_async, mut is_static) = (false, false, false);
		let mut kind = MethodKind::Method;
		if self.is_word(b"static") {
			let t = self.tok;
			self.bump(false);
			if self.tok.kind == Kind::BraceL {
				self.bump(true);
				return self.static_block(start);
			}
			if self.at_class_name() || self.tok.kind == Kind::Star {
				is_static = true;
			} else {
				named = Some(t);
			}
		}
		if named.is_none() && self.is_word(b"async") {
			let t = self.tok;
			self.bump(false);
			if (self.at_class_name() || self.tok.kind == Kind::Star) && !self.can_insert_semicolon() {
				is_async = true;
			} else {
				named = Some(t);
			}
		}
		if named.is_none() && self.tok.kind == Kind::Star {
			self.bump(false);
			generator = true;
		}
		if named.is_none() && !is_async && !generator {
			let accessor = if self.is_word(b"get") {
				Some(MethodKind::Get)
			} else if self.is_word(b"set") {
				Some(MethodKind::Set)
			} else {
				None
			};
			if let Some(accessor) = accessor {
				let t = self.tok;
				self.bump(false);
				if self.at_class_name() {
					kind = accessor;
				} else {
					named = Some(t);
				}
			}
		}
		let (key, computed) = match named {
			Some(t) => {
				let name = self.ast.strings.intern(&self.src[t.start as usize..t.end as usize]);
				(self.ast.add(NodeKind::Identifier { name }, t.start, t.end), false)
			}
			None if self.tok.kind == Kind::PrivateName => {
				let t = self.tok;
				if t.flags & ESCAPED == 0 && &self.src[t.start as usize + 1..t.end as usize] == "constructor" {
					return self.raise(t.start, Code::PrivateConstructor);
				}
				(self.private_name(), false)
			}
			None => self.property_name(),
		};
		if self.error {
			return key;
		}
		let key_start = self.ast.node(key).start;
		if self.tok.kind == Kind::ParenL || kind != MethodKind::Method || generator || is_async {
			let constructor = !is_static && self.key_is(key, computed, "constructor");
			if constructor && kind != MethodKind::Method {
				return self.raise(key_start, Code::AccessorConstructor);
			}
			if constructor {
				kind = MethodKind::Constructor;
				if generator {
					return self.raise(key_start, Code::GeneratorConstructor);
				}
				if is_async {
					return self.raise(key_start, Code::AsyncConstructor);
				}
			} else if is_static && self.key_is(key, computed, "prototype") {
				return self.raise(key_start, Code::StaticPrototype);
			}
			let value = self.method(generator, is_async, constructor && extends, true);
			if kind == MethodKind::Get || kind == MethodKind::Set {
				self.check_accessor_params(value, kind == MethodKind::Get);
			}
			return self.add(
				NodeKind::MethodDefinition {
					key,
					value,
					kind,
					computed,
					is_static,
				},
				start,
			);
		}
		if self.key_is(key, computed, "constructor") {
			return self.raise(key_start, Code::ConstructorField);
		}
		if is_static && self.key_is(key, computed, "prototype") {
			return self.raise(key_start, Code::StaticPrototype);
		}
		let value = if self.tok.kind == Kind::Eq {
			self.bump(true);
			self.enter_scope(SCOPE_CLASS_FIELD_INIT | SCOPE_SUPER);
			let outer = std::mem::replace(&mut self.ctx, Ctx::default());
			let value = self.assign();
			self.ctx = outer;
			self.exit_scope();
			Some(value)
		} else {
			None
		};
		self.class_semicolon();
		self.add(
			NodeKind::PropertyDefinition {
				key,
				value,
				computed,
				is_static,
				kind: FieldKind::Property,
			},
			start,
		)
	}

	/// The end of a class field: what follows is a member, where no regular expression starts.
	pub(super) fn class_semicolon(&mut self) {
		if self.tok.kind == Kind::Semi {
			self.bump(false);
		} else if !self.can_insert_semicolon() {
			self.soft_unexpected();
		}
	}

	/// A static block after its `{`.
	pub(super) fn static_block(&mut self, start: u32) -> NodeId {
		let old_labels = std::mem::take(&mut self.scratch.checks.labels);
		self.enter_scope(SCOPE_CLASS_STATIC_BLOCK | SCOPE_SUPER);
		let outer = std::mem::replace(&mut self.ctx, Ctx::default());
		let base = self.scratch.items.len();
		while self.tok.kind != Kind::BraceR && !self.error {
			let statement = self.statement(BLOCK);
			self.scratch.items.push(Some(statement));
		}
		self.ctx = outer;
		if self.error {
			return NodeId::at(0);
		}
		self.bump(false);
		self.exit_scope();
		self.scratch.checks.labels = old_labels;
		let body = self.list(base);
		self.add(NodeKind::StaticBlock { body }, start)
	}

	// ── modules

	/// A name a module exports or imports under: a word, a keyword too, or a string that is
	/// well-formed text.
	pub(super) fn module_name(&mut self) -> NodeId {
		if self.tok.kind == Kind::String {
			let t = self.tok;
			if t.flags & ESCAPED != 0 {
				self.scratch
					.values
					.string(self.src, t.start as usize, t.end as usize, &mut self.ast.strings);
				if self.scratch.values.marked() {
					return self.raise(t.start, Code::LoneSurrogateInModuleName);
				}
			}
			return self.atom(&mut None, false);
		}
		self.name()
	}

	/// `with { key: "value" }` after a module's source.
	fn with_clause(&mut self) -> List {
		if self.tok.kind != Kind::With {
			return List::EMPTY;
		}
		self.bump(false);
		self.expect(Kind::BraceL, false);
		let base = self.scratch.items.len();
		let mut first = true;
		let mut seen: Vec<StrId> = Vec::new();
		while self.tok.kind != Kind::BraceR && !self.error {
			if !std::mem::take(&mut first) {
				self.expect(Kind::Comma, false);
				if self.tok.kind == Kind::BraceR {
					break;
				}
			}
			let start = self.tok.start;
			let key = if self.tok.kind == Kind::String {
				self.atom(&mut None, false)
			} else {
				self.name()
			};
			self.expect(Kind::Colon, false);
			let value = self.string_literal();
			if self.error {
				break;
			}
			let node = *self.ast.node(key);
			let (NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name }) = node.kind else {
				unreachable!()
			};
			if seen.contains(&name) {
				self.raise_name(node.start, Code::DuplicateImportAttribute, name);
				break;
			}
			seen.push(name);
			let attribute = self.add(NodeKind::ImportAttribute { key, value }, start);
			self.scratch.items.push(Some(attribute));
		}
		self.bump(false);
		self.list(base)
	}

	fn import_export(&mut self, start: u32, place: u8) -> NodeId {
		let is_import = self.tok.kind == Kind::Import;
		if is_import {
			// `import(` and `import.` are expressions: told by the byte that follows
			if matches!(self.lexer.peek_byte(), Some(b'(' | b'.')) {
				return self.expression_statement(start);
			}
		}
		if place != TOP {
			return self.raise(start, Code::ImportExportNotTopLevel);
		}
		if !self.module && !(TS && self.exports_in_script()) {
			return self.raise(start, Code::ImportExportInScript);
		}
		if is_import {
			self.import_statement(start)
		} else {
			self.export_statement(start)
		}
	}

	fn import_statement(&mut self, start: u32) -> NodeId {
		self.bump(false);
		if TS
			&& let Some(node) = self.import_head(start)
		{
			return node;
		}
		let base = self.scratch.items.len();
		let source = if self.tok.kind == Kind::String {
			self.atom(&mut None, false)
		} else {
			self.import_specifiers();
			if self.error {
				return NodeId::at(0);
			}
			self.expect_word(b"from");
			self.string_literal()
		};
		let attributes = self.with_clause();
		self.semicolon();
		let specifiers = self.list(base);
		let node = self.add(
			NodeKind::ImportDeclaration {
				specifiers,
				source,
				attributes,
			},
			start,
		);
		if TS {
			self.import_end(node);
		}
		node
	}

	fn import_specifiers(&mut self) {
		let binding = if TS { self.import_binding() } else { Binding::Lexical };
		if self.tok.kind == Kind::Ident {
			let at = self.tok.start;
			let local = self.ident();
			self.check_lval_simple(local, binding, false);
			let specifier = self.add(NodeKind::ImportDefaultSpecifier { local }, at);
			self.scratch.items.push(Some(specifier));
			if self.tok.kind != Kind::Comma || self.error {
				return;
			}
			self.bump(false);
		}
		if self.tok.kind == Kind::Star {
			let at = self.tok.start;
			self.bump(false);
			self.expect_word(b"as");
			let local = self.ident();
			self.check_lval_simple(local, binding, false);
			let specifier = self.add(NodeKind::ImportNamespaceSpecifier { local }, at);
			self.scratch.items.push(Some(specifier));
			return;
		}
		self.expect(Kind::BraceL, false);
		let mut first = true;
		while self.tok.kind != Kind::BraceR && !self.error {
			if !std::mem::take(&mut first) {
				self.expect(Kind::Comma, false);
				if self.tok.kind == Kind::BraceR {
					break;
				}
			}
			if TS
				&& let Some(node) = self.import_specifier_ts()
			{
				self.scratch.items.push(Some(node));
				continue;
			}
			let at = self.tok.start;
			let imported = self.module_name();
			if self.error {
				return;
			}
			let local = if self.is_word(b"as") {
				self.bump(false);
				self.ident()
			} else {
				self.check_unreserved(imported);
				imported
			};
			self.check_lval_simple(local, binding, false);
			let specifier = self.add(NodeKind::ImportSpecifier { imported, local }, at);
			self.scratch.items.push(Some(specifier));
		}
		self.bump(false);
	}

	/// A name exported a second time is an error at the second.
	fn check_export(&mut self, name: NodeId, pos: u32) {
		let name = match self.ast.node(name).kind {
			NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } => name,
			_ => return,
		};
		self.check_export_name(name, pos);
	}

	fn check_export_name(&mut self, name: StrId, pos: u32) {
		if self.error {
			return;
		}
		// no error in TypeScript: an exported overload would be one
		if self.scratch.checks.exports.has(name, 1) {
			if !TS {
				self.raise_name(pos, Code::DuplicateExport, name);
			}
			return;
		}
		self.scratch.checks.exports.push(name, 1, 0);
	}

	fn check_pattern_export(&mut self, pattern: NodeId) {
		let node = *self.ast.node(pattern);
		match node.kind {
			NodeKind::Identifier { .. } => self.check_export(pattern, node.start),
			NodeKind::ObjectPattern { properties: list } | NodeKind::ArrayPattern { elements: list } => {
				for i in 0..list.len {
					if let Some(item) = self.ast.nth(list, i) {
						self.check_pattern_export(item);
					}
				}
			}
			NodeKind::Property { value, .. } => self.check_pattern_export(value),
			NodeKind::AssignmentPattern { left, .. } => self.check_pattern_export(left),
			NodeKind::RestElement { argument } => self.check_pattern_export(argument),
			_ => {}
		}
	}

	fn export_statement(&mut self, start: u32) -> NodeId {
		self.bump(false);
		if TS
			&& let Some(node) = self.export_head(start)
		{
			return node;
		}
		let node = self.export_rest(start);
		if TS {
			self.export_end(node);
		}
		node
	}

	fn export_rest(&mut self, start: u32) -> NodeId {
		if TS
			&& !matches!(self.tok.kind, Kind::Star | Kind::Default)
			&& let Some(declaration) = self.export_declaration_ts()
		{
			return self.export_declared(start, declaration);
		}
		match self.tok.kind {
			Kind::Star => {
				self.bump(false);
				let exported = if self.is_word(b"as") {
					self.bump(false);
					let exported = self.module_name();
					if self.error {
						return exported;
					}
					let at = self.ast.node(exported).start;
					self.check_export(exported, at);
					Some(exported)
				} else {
					None
				};
				self.expect_word(b"from");
				let source = self.string_literal();
				let attributes = self.with_clause();
				self.semicolon();
				self.add(
					NodeKind::ExportAllDeclaration {
						exported,
						source,
						attributes,
					},
					start,
				)
			}
			Kind::Default => {
				let at = self.tok.start;
				self.bump(true);
				let name = self.ast.strings.intern("default");
				self.check_export_name(name, at);
				if self.error {
					return NodeId::at(0);
				}
				let declaration = self.export_default();
				self.add(NodeKind::ExportDefaultDeclaration { declaration }, start)
			}
			Kind::Var | Kind::Const | Kind::Class | Kind::Function => self.export_declaration(start),
			Kind::Ident if self.tok.flags & ESCAPED == 0 && (self.is_word(b"let") || self.is_word(b"async")) => {
				// `let` that declares, `async function`: anything else is not exported this way
				let (t, prev, comments) = (self.tok, self.prev_end, self.lexer.comments.len());
				let declares = if self.is_word(b"let") {
					(self.strict && self.recovering()) || self.lexer.declares_after_let(false)
				} else {
					self.bump(false);
					let function = self.tok.kind == Kind::Function && !self.newline() && self.tok.flags & ESCAPED == 0;
					self.back(t, prev, comments);
					function
				};
				if declares {
					self.export_declaration(start)
				} else {
					self.export_specifiers(start)
				}
			}
			_ => self.export_specifiers(start),
		}
	}

	fn export_declaration(&mut self, start: u32) -> NodeId {
		let declaration = self.statement(BLOCK);
		self.export_declared(start, declaration)
	}

	fn export_declared(&mut self, start: u32, declaration: NodeId) -> NodeId {
		if self.error {
			return declaration;
		}
		match self.ast.node(declaration).kind {
			NodeKind::VariableDeclaration { declarations, .. } => {
				for i in 0..declarations.len {
					let NodeKind::VariableDeclarator { id, .. } = self.ast.node(self.ast.nth(declarations, i).unwrap()).kind
					else {
						unreachable!()
					};
					self.check_pattern_export(id);
				}
			}
			NodeKind::FunctionDeclaration {
				function: Function { id: Some(id), .. },
			}
			| NodeKind::ClassDeclaration {
				class: Class { id: Some(id), .. },
			} => {
				let at = self.ast.node(id).start;
				self.check_export(id, at);
			}
			_ => {}
		}
		self.add(NodeKind::ExportDeclaration { declaration }, start)
	}

	fn export_specifiers(&mut self, start: u32) -> NodeId {
		self.expect(Kind::BraceL, false);
		let base = self.scratch.items.len();
		let mut first = true;
		while self.tok.kind != Kind::BraceR && !self.error {
			if !std::mem::take(&mut first) {
				self.expect(Kind::Comma, false);
				if self.tok.kind == Kind::BraceR {
					break;
				}
			}
			if TS
				&& let Some(node) = self.export_specifier_ts()
			{
				self.scratch.items.push(Some(node));
				continue;
			}
			let at = self.tok.start;
			let local = self.module_name();
			let exported = if self.is_word(b"as") {
				self.bump(false);
				self.module_name()
			} else {
				local
			};
			if self.error {
				break;
			}
			let exported_at = self.ast.node(exported).start;
			self.check_export(exported, exported_at);
			let specifier = self.add(NodeKind::ExportSpecifier { local, exported }, at);
			self.scratch.items.push(Some(specifier));
		}
		if self.error {
			return NodeId::at(0);
		}
		self.bump(false);
		let (source, attributes) = if self.is_word(b"from") {
			self.bump(false);
			(Some(self.string_literal()), self.with_clause())
		} else {
			for i in base..self.scratch.items.len() {
				let NodeKind::ExportSpecifier { local, .. } = self.ast.node(self.scratch.items[i].unwrap()).kind else {
					unreachable!()
				};
				self.check_unreserved(local);
				if self.error {
					return NodeId::at(0);
				}
				let node = *self.ast.node(local);
				match node.kind {
					NodeKind::Identifier { name } => {
						if !self.declares_export(name) {
							self.scratch.checks.undeclared_exports.push((name, node.start));
						}
					}
					_ => return self.raise(node.start, Code::StringExportWithoutFrom),
				}
			}
			(None, List::EMPTY)
		};
		self.semicolon();
		let specifiers = self.list(base);
		self.add(
			NodeKind::ExportNamedDeclaration {
				specifiers,
				source,
				attributes,
			},
			start,
		)
	}

	fn export_default(&mut self) -> NodeId {
		if TS
			&& let Some(declaration) = self.export_default_ts()
		{
			return declaration;
		}
		let start = self.tok.start;
		match self.tok.kind {
			Kind::Function => {
				self.bump(false);
				return self.function(start, false, true, true, false);
			}
			Kind::Class => return self.class(true, true),
			Kind::Ident if self.is_word(b"async") => {
				let (t, prev, comments) = (self.tok, self.prev_end, self.lexer.comments.len());
				self.bump(false);
				if self.tok.kind == Kind::Function && !self.newline() && self.tok.flags & ESCAPED == 0 {
					self.bump(false);
					return self.function(start, true, true, true, false);
				}
				self.back(t, prev, comments);
			}
			_ => {}
		}
		let declaration = self.assign();
		self.semicolon();
		declaration
	}

	/// A function after its name, in its scope, with what TypeScript reads around it.
	#[expect(clippy::too_many_arguments)]
	fn function_ts(
		&mut self,
		start: u32,
		id: Option<NodeId>,
		is_async: bool,
		generator: bool,
		declaration: bool,
		hanging: bool,
		outer: Ctx,
		old: (u32, u32, u32),
	) -> NodeId {
		use super::ts::FunctionKind;
		let kind = if declaration {
			FunctionKind::Declaration
		} else {
			FunctionKind::Expression
		};
		let mut frame = self.function_start(kind);
		self.expect(Kind::ParenL, false);
		let params = self.binding_list(Kind::ParenR, false);
		if !self.error {
			self.check_yield_await_in_default_params();
		}
		let bodiless = self.function_body_ts(&mut frame, start, id, params, is_async, generator, kind);
		let node = match bodiless {
			Some(node) => {
				self.exit_scope();
				node
			}
			None => {
				let body = self.function_body(start, id, params, false, false);
				let function = Function {
					id,
					params,
					body,
					is_async,
					generator,
				};
				let kind = if declaration {
					NodeKind::FunctionDeclaration { function }
				} else {
					NodeKind::FunctionExpression { function }
				};
				self.add(kind, start)
			}
		};
		self.ctx = outer;
		self.restore_yield_await(old);
		if self.error {
			return node;
		}
		if declaration
			&& !hanging && let Some(name) = id
		{
			let binding = if matches!(self.ast.node(node).kind, NodeKind::FunctionDeclaration { .. }) {
				self.function_binding(generator, is_async)
			} else {
				Binding::None
			};
			self.check_lval_simple(name, binding, false);
		}
		self.function_end(node, frame);
		node
	}

	/// A method's function, what TypeScript reads before its `(` at the cursor.
	pub(super) fn method_ts(
		&mut self,
		generator: bool,
		is_async: bool,
		direct_super: bool,
		in_class: bool,
		method_kind: MethodKind,
	) -> NodeId {
		use super::ts::FunctionKind;
		let old = self.take_yield_await();
		self.enter_scope(
			function_flags(is_async, generator) | SCOPE_SUPER | if direct_super { SCOPE_DIRECT_SUPER } else { 0 },
		);
		let kind = FunctionKind::Method {
			in_class,
			kind: method_kind,
		};
		let mut frame = self.function_start(kind);
		let start = self.tok.start;
		let outer = self.ctx;
		self.ctx = Ctx {
			generator,
			can_await: is_async,
			no_in: false,
		};
		self.expect(Kind::ParenL, false);
		self.modifiers_ok = in_class;
		let params = self.binding_list(Kind::ParenR, false);
		if !self.error {
			self.check_yield_await_in_default_params();
		}
		let bodiless = self.function_body_ts(&mut frame, start, None, params, is_async, generator, kind);
		let node = match bodiless {
			Some(node) => {
				self.exit_scope();
				node
			}
			None => {
				let body = self.function_body(start, None, params, false, true);
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
		self.ctx = outer;
		self.restore_yield_await(old);
		self.function_end(node, frame);
		node
	}

	pub(super) fn ts_binding_items(&mut self, close: Kind, holes: bool, allow_modifiers: bool) {
		self.modifiers_ok = allow_modifiers;
		self.binding_items(close, holes);
	}

	pub(super) fn unary_for_ts(&mut self) -> NodeId {
		self.unary(true, &mut None)
	}

	pub(super) fn ts_unary(&mut self) -> NodeId {
		self.unary(true, &mut None)
	}

	pub(super) fn ts_template(&mut self) -> NodeId {
		self.template(false)
	}
}
