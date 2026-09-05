mod class;
mod expression;
mod pattern;
mod scope;
mod statement;

#[cfg(test)]
mod tests;

use crate::ast::{Ast, List, NodeId, NodeKind};
use crate::error::SyntaxError;
use crate::interner::StrId;
use crate::lexer::Lexer;
use crate::lexer::token::{Keyword, Token, TokenKind};
use scope::{SCOPE_TOP, Scope};
use std::collections::HashMap;

pub(crate) type Result<T> = std::result::Result<T, SyntaxError>;

const MAX_DEPTH: u32 = 1000;

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
	/// Parse as an ES module: strict mode, top-level `await`, `import` and `export`.
	pub module: bool,
	pub allow_return_outside_function: bool,
	pub allow_await_outside_function: bool,
	pub allow_super_outside_method: bool,
	pub allow_undeclared_exports: bool,
	pub preserve_parens: bool,
}

pub fn parse(src: &str, options: Options) -> Result<Ast> {
	let mut parser = Parser::new(src, 0, options)?;
	let program = parser.parse_program()?;
	debug_assert_eq!(program, NodeId(parser.ast.nodes.len() as u32 - 1));
	Ok(parser.finish())
}

/// Parses a single expression starting at `offset`, stopping where the expression ends.
pub fn parse_expression_at(src: &str, offset: u32, options: Options) -> Result<(Ast, NodeId)> {
	let mut parser = Parser::new(src, offset, options)?;
	parser.enter_scope(SCOPE_TOP);
	let expression = parser.parse_expression(false, &mut None)?;
	Ok((parser.finish(), expression))
}

pub(crate) struct Parser<'a> {
	lexer: Lexer<'a>,
	pub(crate) ast: Ast,
	options: Options,
	tok: Token,
	prev_end: u32,
	strict: bool,
	depth: u32,
	scopes: Vec<Scope>,
	labels: Vec<Label>,
	private_names: Vec<PrivateNameScope>,
	undeclared_exports: HashMap<StrId, (u32, usize)>,
	yield_pos: u32,
	await_pos: u32,
	await_ident_pos: u32,
	potential_arrow_at: u32,
	potential_arrow_in_for_await: bool,
}

#[derive(Clone, Copy)]
struct Label {
	name: Option<StrId>,
	kind: LabelKind,
	statement_start: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LabelKind {
	None,
	Loop,
	Switch,
}

#[derive(Default)]
struct PrivateNameScope {
	declared: Vec<(StrId, PrivateKind)>,
	used: Vec<(StrId, u32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrivateKind {
	Any,
	InstanceGet,
	InstanceSet,
	StaticGet,
	StaticSet,
}

/// Errors that only become real once an expression turns out to be a pattern, or vice versa.
#[derive(Clone, Copy, Default)]
pub(crate) struct DestructuringErrors {
	pub shorthand_assign: Option<u32>,
	pub trailing_comma: Option<u32>,
	pub parenthesized_assign: Option<u32>,
	pub parenthesized_bind: Option<u32>,
	pub double_proto: Option<u32>,
}

impl<'a> Parser<'a> {
	fn new(src: &'a str, offset: u32, options: Options) -> Result<Self> {
		let mut lexer = Lexer::new(src);
		lexer.set_pos(offset);
		let strict = options.module || expression::strict_directive(src, offset);
		lexer.strict = strict;
		lexer.module = options.module;
		let tok = lexer.next_token()?;
		Ok(Self {
			lexer,
			ast: Ast::default(),
			options,
			tok,
			prev_end: offset,
			strict,
			depth: 0,
			scopes: Vec::new(),
			labels: Vec::new(),
			private_names: Vec::new(),
			undeclared_exports: HashMap::new(),
			yield_pos: 0,
			await_pos: 0,
			await_ident_pos: 0,
			potential_arrow_at: u32::MAX,
			potential_arrow_in_for_await: false,
		})
	}

	fn finish(self) -> Ast {
		let mut ast = self.ast;
		let mut lexer = self.lexer;
		ast.comments = std::mem::take(&mut lexer.comments);
		ast.strings = std::mem::take(&mut lexer.strings);
		ast
	}

	pub(crate) fn source(&self) -> &'a str {
		self.lexer.source()
	}

	pub(crate) fn error<T>(&self, pos: u32, message: impl Into<String>) -> Result<T> {
		Err(SyntaxError::new(pos, message))
	}

	pub(crate) fn unexpected<T>(&self) -> Result<T> {
		self.unexpected_at(self.tok.start)
	}

	pub(crate) fn unexpected_at<T>(&self, pos: u32) -> Result<T> {
		self.error(pos, "Unexpected token")
	}

	pub(crate) fn str(&self, id: StrId) -> &str {
		self.lexer.strings.get(id)
	}

	pub(crate) fn intern(&mut self, s: &str) -> StrId {
		self.lexer.strings.intern(s)
	}

	pub(crate) fn add(&mut self, kind: NodeKind, start: u32) -> NodeId {
		self.ast.add(kind, start, self.prev_end)
	}

	pub(crate) fn add_with_end(&mut self, kind: NodeKind, start: u32, end: u32) -> NodeId {
		self.ast.add(kind, start, end)
	}

	pub(crate) fn list(&mut self, items: &[Option<NodeId>]) -> List {
		self.ast.add_list(items)
	}

	pub(crate) fn list_of(&mut self, items: &[NodeId]) -> List {
		let start = self.ast.lists.len() as u32;
		self.ast.lists.extend(items.iter().map(|&id| Some(id)));
		List {
			start,
			len: items.len() as u32,
		}
	}

	pub(crate) fn kind(&self, id: NodeId) -> NodeKind {
		self.ast.node(id).kind
	}

	pub(crate) fn start_of(&self, id: NodeId) -> u32 {
		self.ast.node(id).start
	}

	pub(crate) fn end_of(&self, id: NodeId) -> u32 {
		self.ast.node(id).end
	}

	pub(crate) fn set_strict(&mut self, strict: bool) {
		self.strict = strict;
		self.lexer.strict = strict;
	}

	/// Guards the recursive descent so deep nesting fails cleanly instead of overflowing the stack.
	pub(crate) fn enter(&mut self) -> Result<()> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			return self.error(self.tok.start, "Maximum nesting depth exceeded");
		}
		Ok(())
	}

	pub(crate) fn leave(&mut self) {
		self.depth -= 1;
	}

	// Tokens

	/// Consumes the current token; an escaped keyword is an error unless consumed as a name.
	pub(crate) fn next(&mut self) -> Result<()> {
		if self.tok.escaped
			&& let TokenKind::Keyword(keyword) = self.tok.kind
		{
			return self.error(
				self.tok.start,
				format!("Escape sequence in keyword {}", keyword.as_str()),
			);
		}
		self.next_liberal()
	}

	pub(crate) fn next_liberal(&mut self) -> Result<()> {
		self.prev_end = self.tok.end;
		self.tok = self.lexer.next_token()?;
		Ok(())
	}

	pub(crate) fn is(&self, kind: TokenKind) -> bool {
		self.tok.kind == kind
	}

	pub(crate) fn is_keyword(&self, keyword: Keyword) -> bool {
		self.tok.kind == TokenKind::Keyword(keyword)
	}

	pub(crate) fn eat(&mut self, kind: TokenKind) -> Result<bool> {
		if self.is(kind) {
			self.next()?;
			Ok(true)
		} else {
			Ok(false)
		}
	}

	pub(crate) fn eat_keyword(&mut self, keyword: Keyword) -> Result<bool> {
		self.eat(TokenKind::Keyword(keyword))
	}

	pub(crate) fn expect(&mut self, kind: TokenKind) -> Result<()> {
		if self.eat(kind)? { Ok(()) } else { self.unexpected() }
	}

	pub(crate) fn expect_keyword(&mut self, keyword: Keyword) -> Result<()> {
		self.expect(TokenKind::Keyword(keyword))
	}

	/// The current token is the unescaped identifier `name`.
	pub(crate) fn is_contextual(&self, name: &str) -> bool {
		match self.tok.kind {
			TokenKind::Ident(id) if !self.tok.escaped => self.str(id) == name,
			_ => false,
		}
	}

	pub(crate) fn eat_contextual(&mut self, name: &str) -> Result<bool> {
		if self.is_contextual(name) {
			self.next()?;
			Ok(true)
		} else {
			Ok(false)
		}
	}

	pub(crate) fn expect_contextual(&mut self, name: &str) -> Result<()> {
		if self.eat_contextual(name)? {
			Ok(())
		} else {
			self.unexpected()
		}
	}

	pub(crate) fn ident_name(&self) -> Option<StrId> {
		match self.tok.kind {
			TokenKind::Ident(name) => Some(name),
			_ => None,
		}
	}

	/// The next significant character and whether a line break precedes it, without tokenizing.
	pub(crate) fn peek_char(&self) -> (Option<char>, bool, usize) {
		self.lexer.peek_char()
	}

	pub(crate) fn can_insert_semicolon(&self) -> bool {
		self.is(TokenKind::Eof) || self.is(TokenKind::BraceR) || self.tok.newline_before
	}

	pub(crate) fn semicolon(&mut self) -> Result<()> {
		if !self.eat(TokenKind::Semi)? && !self.can_insert_semicolon() {
			return self.unexpected();
		}
		Ok(())
	}

	pub(crate) fn after_trailing_comma(&mut self, kind: TokenKind, not_next: bool) -> Result<bool> {
		if self.is(kind) {
			if !not_next {
				self.next()?;
			}
			return Ok(true);
		}
		Ok(false)
	}

	pub(crate) fn check_expression_errors(
		&self,
		errors: &Option<DestructuringErrors>,
		and_throw: bool,
	) -> Result<bool> {
		let Some(errors) = errors else { return Ok(false) };
		let has = errors.shorthand_assign.is_some() || errors.double_proto.is_some();
		if !and_throw {
			return Ok(has);
		}
		if let Some(pos) = errors.shorthand_assign {
			return self.error(
				pos,
				"Shorthand property assignments are valid only in destructuring patterns",
			);
		}
		if let Some(pos) = errors.double_proto {
			return self.error(pos, "Redefinition of __proto__ property");
		}
		Ok(false)
	}

	pub(crate) fn check_pattern_errors(&self, errors: &Option<DestructuringErrors>, is_assign: bool) -> Result<()> {
		let Some(errors) = errors else { return Ok(()) };
		if let Some(pos) = errors.trailing_comma {
			return self.error(pos, "Comma is not permitted after the rest element");
		}
		let parens = if is_assign {
			errors.parenthesized_assign
		} else {
			errors.parenthesized_bind
		};
		if let Some(pos) = parens {
			return self.error(
				pos,
				if is_assign {
					"Assigning to rvalue"
				} else {
					"Parenthesized pattern"
				},
			);
		}
		Ok(())
	}

	pub(crate) fn check_yield_await_in_default_params(&self) -> Result<()> {
		if self.yield_pos != 0 && (self.await_pos == 0 || self.yield_pos < self.await_pos) {
			return self.error(self.yield_pos, "Yield expression cannot be a default value");
		}
		if self.await_pos != 0 {
			return self.error(self.await_pos, "Await expression cannot be a default value");
		}
		Ok(())
	}

	pub(crate) fn is_simple_assign_target(&self, id: NodeId) -> bool {
		match self.kind(id) {
			NodeKind::ParenthesizedExpression { expression } => self.is_simple_assign_target(expression),
			NodeKind::Identifier { .. } | NodeKind::MemberExpression { .. } => true,
			_ => false,
		}
	}
}
