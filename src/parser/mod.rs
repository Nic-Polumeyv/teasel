pub(crate) mod class;
pub(crate) mod expression;
mod pattern;
pub(crate) mod scope;
pub(crate) mod statement;

#[cfg(test)]
pub(crate) mod tests;

use crate::ast::{Ast, List, NodeId, NodeKind, VariableKind};
use crate::error::SyntaxError;
use crate::interner::StrId;
use crate::lexer::Lexer;
use crate::lexer::token::{Keyword, Token, TokenKind};
pub(crate) use expression::ForInit;
use scope::{SCOPE_TOP, Scope};
pub(crate) use statement::Context;
use std::collections::{HashMap, HashSet};

pub(crate) type Result<T> = std::result::Result<T, SyntaxError>;

const MAX_DEPTH: u32 = 1000;
/// Subscripts and binary operators chain without recursion, but the tree they build is as deep
/// as the chain is long, and everything that walks it recurses.
const MAX_CHAIN: u32 = 10_000;

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

/// What a function-shaped node is, for the extension hooks around its signature.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionKind {
	Declaration,
	Expression,
	Method { in_class: bool },
	Arrow,
}

/// The check an extension node is being unwrapped for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unwrap {
	/// `check_lval_simple`: the target of an assignment or update.
	Simple,
	/// `check_lval_inner_pattern`: an element of a parameter list or pattern.
	InnerPattern,
}

/// Grammar an extension adds to the JavaScript parser at fixed points. Every hook has a no-op
/// default, so the plain JavaScript parser is the unit extension. State an extension keeps while
/// parsing lives in `Self` (cloned into snapshots, so keep it small); what it hands back with the
/// tree lives in `Data`.
#[allow(unused_variables)]
pub(crate) trait Extension: Default + Sized {
	type Data: Default;
	/// What a speculative parse needs to put the extension's state back.
	type Snapshot;

	/// Whether exporting the same name twice is an error.
	const DUPLICATE_EXPORT_ERRORS: bool = true;
	/// Whether a function's name is declared after its body, so a bodiless overload declares nothing.
	const DECLARES_FUNCTION_NAME_AFTER_BODY: bool = false;
	/// Whether `static` before a class member is only ever read by `class_modifiers`.
	const STATIC_IS_A_MODIFIER: bool = false;

	fn init(p: &mut Parser<Self>) {}
	fn save(&self) -> Self::Snapshot;
	fn restore(&mut self, snapshot: Self::Snapshot);

	// Statements and modules

	/// First look at a statement; `Some` replaces it entirely.
	fn statement(p: &mut Parser<Self>, context: Context, top_level: bool) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// An expression statement whose expression is a bare identifier may be a declaration instead.
	fn expression_statement(p: &mut Parser<Self>, start: u32, expression: NodeId) -> Result<Option<NodeId>> {
		Ok(None)
	}
	fn starts_export_declaration(p: &mut Parser<Self>) -> bool {
		false
	}
	/// Right after `export`; `Some` is a whole export statement.
	fn export_head(p: &mut Parser<Self>, start: u32) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// The declaration after `export`, when `starts_export_declaration` or the plain grammar said so.
	fn export_declaration(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		Ok(None)
	}
	fn export_default(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		Ok(None)
	}
	fn export_end(p: &mut Parser<Self>, node: NodeId) {}
	fn export_specifier(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// Right after `import`; `Some` is a whole import statement.
	fn import_head(p: &mut Parser<Self>, start: u32) -> Result<Option<NodeId>> {
		Ok(None)
	}
	fn import_end(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		Ok(())
	}
	fn import_specifier(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// Whether the extension declared `name` in a way that satisfies a local `export { name }`.
	fn declares_export(p: &mut Parser<Self>, name: StrId) -> bool {
		false
	}
	fn scope_exit(p: &mut Parser<Self>) {}

	// Bindings

	/// After the id of a variable declarator.
	fn var_id(p: &mut Parser<Self>, id: NodeId) -> Result<()> {
		Ok(())
	}
	fn var_declarator(p: &mut Parser<Self>, node: NodeId, kind: VariableKind) -> Result<()> {
		Ok(())
	}
	/// Whether a declarator may go without an initializer where the plain grammar requires one.
	fn allows_missing_initializer(p: &mut Parser<Self>) -> bool {
		false
	}
	fn binding_atom(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// Before an element of a binding list; `allow_modifiers` inside class methods. Returns where
	/// the element's own node starts.
	fn binding_item_start(p: &mut Parser<Self>, allow_modifiers: bool) -> Result<u32> {
		Ok(p.tok.start)
	}
	/// After a binding, before its default.
	fn binding_annotation(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		Ok(())
	}
	fn binding_item_end(p: &mut Parser<Self>, item: NodeId) -> Result<NodeId> {
		Ok(item)
	}
	fn catch_param(p: &mut Parser<Self>, param: NodeId) -> Result<()> {
		Ok(())
	}
	/// After a pattern read on its own, the way Svelte reads an each-block context.
	fn pattern_annotation(p: &mut Parser<Self>, pattern: NodeId) -> Result<()> {
		Ok(())
	}

	// Functions and classes

	/// Before the parameters of a function, method or arrow.
	fn function_start(p: &mut Parser<Self>, kind: FunctionKind) -> Result<()> {
		Ok(())
	}
	/// After the parameters, before the body; `Some` is a function without a body.
	#[allow(clippy::too_many_arguments)]
	fn function_body(
		p: &mut Parser<Self>,
		start: u32,
		id: Option<NodeId>,
		params: List,
		is_async: bool,
		generator: bool,
		kind: FunctionKind,
	) -> Result<Option<NodeId>> {
		Ok(None)
	}
	fn function_end(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		Ok(())
	}
	/// Whether an object literal accessor's first parameter is a `this` parameter, which does not
	/// count.
	fn accessor_this_param(p: &Parser<Self>, params: List) -> bool {
		false
	}
	/// The parameters of a function-shaped extension node.
	fn function_params(p: &Parser<Self>, node: NodeId) -> Option<List> {
		None
	}
	fn class_start(p: &mut Parser<Self>) -> Result<()> {
		Ok(())
	}
	/// Whether the token after `class` opens a heritage clause rather than naming the class.
	fn starts_class_heritage(p: &mut Parser<Self>) -> bool {
		false
	}
	fn class_type_parameters(p: &mut Parser<Self>) -> Result<()> {
		Ok(())
	}
	fn class_heritage(p: &mut Parser<Self>, has_super: bool) -> Result<()> {
		Ok(())
	}
	fn class_end(p: &mut Parser<Self>, node: NodeId) {}
	/// Modifiers before a class element; returns whether `static` was among them.
	fn class_modifiers(p: &mut Parser<Self>) -> Result<bool> {
		Ok(false)
	}
	fn class_index_signature(p: &mut Parser<Self>, start: u32) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// After the key of a class element.
	fn class_key_end(p: &mut Parser<Self>, key: NodeId, computed: bool) -> Result<()> {
		Ok(())
	}
	fn starts_class_method(p: &mut Parser<Self>) -> bool {
		false
	}
	fn class_method_start(p: &mut Parser<Self>) -> Result<()> {
		Ok(())
	}
	/// After the key of a class field, before its initializer.
	fn class_field_annotation(p: &mut Parser<Self>) -> Result<()> {
		Ok(())
	}
	fn class_element_end(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		Ok(())
	}

	// Expressions

	fn maybe_assign(p: &mut Parser<Self>, for_init: ForInit, errors: &mut Errors) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// Entering a parenthesized list that may turn out to be arrow parameters.
	fn paren_list_start(p: &mut Parser<Self>) {}
	fn paren_list_end(p: &mut Parser<Self>) {}
	/// An item of a parenthesized list, after its expression.
	fn paren_item(p: &mut Parser<Self>, item: NodeId) -> Result<NodeId> {
		Ok(item)
	}
	/// A spread in an argument list.
	fn spread(p: &mut Parser<Self>, spread: NodeId) -> Result<()> {
		Ok(())
	}
	/// At `?` after an expression; `Some` replaces the conditional.
	fn conditional(p: &mut Parser<Self>, expr: NodeId, start: u32, for_init: ForInit) -> Result<Option<NodeId>> {
		Ok(None)
	}
	fn unary(p: &mut Parser<Self>, for_init: ForInit) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// First look at an atom.
	fn atom(p: &mut Parser<Self>, errors: &mut Errors, for_init: ForInit, for_new: bool) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// At an operator position; `Some` is the new left operand.
	fn expr_op(p: &mut Parser<Self>, left: NodeId, left_start: u32, min_prec: i8) -> Result<Option<NodeId>> {
		Ok(None)
	}
	#[allow(clippy::too_many_arguments)]
	fn subscript(
		p: &mut Parser<Self>,
		base: NodeId,
		start: u32,
		no_calls: bool,
		maybe_async_arrow: bool,
		optional_chained: bool,
		for_init: ForInit,
	) -> Result<Option<(NodeId, bool)>> {
		Ok(None)
	}
	fn should_parse_arrow(p: &mut Parser<Self>, items: &[Option<NodeId>]) -> Result<bool> {
		Ok(!p.can_insert_semicolon())
	}
	fn should_parse_async_arrow(p: &mut Parser<Self>) -> Result<bool> {
		Ok(!p.can_insert_semicolon() && p.eat(TokenKind::Arrow)?)
	}
	/// Whether the target of an assignment is checked here.
	fn checks_assignment_target(p: &mut Parser<Self>) -> bool {
		true
	}
	fn new_expression(p: &mut Parser<Self>, node: NodeId) {}
	/// An object property whose value starts unexpectedly for the plain grammar.
	#[allow(clippy::too_many_arguments)]
	fn property_value(
		p: &mut Parser<Self>,
		start: u32,
		key: NodeId,
		computed: bool,
		is_pattern: bool,
		generator: bool,
		is_async: bool,
	) -> Result<Option<NodeId>> {
		Ok(None)
	}
	fn template_expression(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// Reinterprets an extension node as a pattern; the result replaces it.
	fn make_pattern(p: &mut Parser<Self>, id: NodeId, is_binding: bool, errors: &mut Errors) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// Replaces the items of a list about to become patterns.
	fn convert_items(p: &mut Parser<Self>, items: &mut [Option<NodeId>]) {}
	/// What a parenthesized expression becomes as a pattern, given what its inner one became.
	fn parenthesized_pattern(p: &mut Parser<Self>, paren: NodeId, inner: NodeId, pattern: NodeId) -> NodeId {
		paren
	}
	/// The plain node an extension wrapper stands for in a check, if any.
	fn unwrap(p: &Parser<Self>, id: NodeId, context: Unwrap) -> Option<NodeId> {
		None
	}
}

impl Extension for () {
	type Data = ();
	type Snapshot = ();

	fn save(&self) {}

	fn restore(&mut self, _: ()) {}
}

pub(crate) type Errors = Option<DestructuringErrors>;

pub(crate) fn parse<E: Extension>(src: &str, options: Options) -> Result<Ast<E::Data>> {
	let mut parser = Parser::<E>::new(src, 0, options)?;
	let program = parser.parse_program()?;
	debug_assert_eq!(program, parser.ast.last());
	Ok(parser.finish())
}

/// Parses a single expression starting at `offset`, stopping where the expression ends.
pub(crate) fn parse_expression_at<E: Extension>(
	src: &str,
	offset: u32,
	options: Options,
) -> Result<(Ast<E::Data>, NodeId)> {
	let mut parser = Parser::<E>::new(src, offset, options)?;
	parser.enter_scope(SCOPE_TOP);
	let expression = parser.parse_expression(false, &mut None)?;
	Ok((parser.finish(), expression))
}

/// Parses an assignment target starting at `offset`: an identifier or a destructuring pattern,
/// the way Svelte reads `{#each list as pattern}` by handing `(pattern = 1)` to acorn. Svelte reads
/// a bare identifier itself, so only the destructuring forms are reached through acorn there.
pub(crate) fn parse_pattern_at<E: Extension>(
	src: &str,
	offset: u32,
	options: Options,
) -> Result<(Ast<E::Data>, NodeId)> {
	let mut parser = Parser::<E>::new(src, offset, options)?;
	parser.enter_scope(SCOPE_TOP);
	let mut errors = Some(DestructuringErrors::default());
	let expression = match parser.tok.kind {
		TokenKind::BraceL | TokenKind::BracketL => {
			parser.parse_expr_atom(&mut errors, expression::ForInit::No, false)?
		}
		_ => parser.parse_ident(false)?,
	};
	let pattern = parser.make_pattern(expression, false, &mut errors)?;
	parser.check_lval_pattern(pattern, scope::Binding::None, &mut None)?;
	E::pattern_annotation(&mut parser, pattern)?;
	Ok((parser.finish(), pattern))
}

/// Parses a parenthesized parameter list starting at `offset`, the way acorn reads the
/// parameters of `(params) => {}`: as expressions in the enclosing scope, reinterpreted as
/// patterns once the list is complete. Returns the parameters and the offset after the closing
/// paren.
pub(crate) fn parse_params_at<E: Extension>(
	src: &str,
	offset: u32,
	options: Options,
) -> Result<(Ast<E::Data>, Vec<NodeId>, u32)> {
	let mut parser = Parser::<E>::new(src, offset, options)?;
	parser.enter_scope(SCOPE_TOP);
	parser.expect(TokenKind::ParenL)?;
	let paren = parser.parse_paren_items()?;
	let end = parser.prev_end;
	parser.check_pattern_errors(&paren.errors, false)?;
	parser.check_yield_await_in_default_params()?;
	parser.enter_scope(scope::function_flags(false, false) | scope::SCOPE_ARROW);
	let params = parser.make_patterns(paren.items, true)?;
	let list = parser.list(&params);
	parser.check_params(list, false)?;
	let params = params.into_iter().flatten().collect();
	Ok((parser.finish(), params, end))
}

/// Parses a single statement starting at `offset`, as if at the top level of a module.
pub(crate) fn parse_statement_at<E: Extension>(
	src: &str,
	offset: u32,
	options: Options,
) -> Result<(Ast<E::Data>, NodeId)> {
	let mut parser = Parser::<E>::new(src, offset, options)?;
	parser.enter_scope(SCOPE_TOP);
	let mut exports = HashSet::new();
	let statement = parser.parse_statement(statement::Context::None, true, Some(&mut exports))?;
	Ok((parser.finish(), statement))
}

pub(crate) struct Parser<'a, E: Extension = ()> {
	pub(crate) lexer: Lexer<'a>,
	pub(crate) ast: Ast<E::Data>,
	pub(crate) ext: E,
	pub(crate) options: Options,
	pub(crate) tok: Token,
	pub(crate) prev_end: u32,
	pub(crate) strict: bool,
	pub(crate) depth: u32,
	pub(crate) scopes: Vec<Scope>,
	labels: Vec<Label>,
	private_names: Vec<PrivateNameScope>,
	pub(crate) undeclared_exports: HashMap<StrId, (u32, usize)>,
	pub(crate) yield_pos: u32,
	pub(crate) await_pos: u32,
	pub(crate) await_ident_pos: u32,
	pub(crate) potential_arrow_at: u32,
	potential_arrow_in_for_await: bool,
}

pub(crate) struct Snapshot<E: Extension> {
	tokens: TokenSnapshot,
	depth: u32,
	ext: E::Snapshot,
}

pub(crate) struct TokenSnapshot {
	pos: u32,
	in_type: bool,
	tok: Token,
	prev_end: u32,
	comments: usize,
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

impl<'a, E: Extension> Parser<'a, E> {
	fn new(src: &'a str, offset: u32, options: Options) -> Result<Self> {
		let mut lexer = Lexer::new(src);
		lexer.set_pos(offset);
		let strict = options.module || expression::strict_directive(src, offset);
		lexer.strict = strict;
		lexer.module = options.module;
		let mut parser = Self {
			lexer,
			ast: Ast::default(),
			ext: E::default(),
			options,
			tok: Token::eof(offset),
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
		};
		E::init(&mut parser);
		parser.tok = parser.lexer.next_token()?;
		Ok(parser)
	}

	/// Enough state to retry a speculative parse from here. Nodes built by a failed attempt stay
	/// in the arena, unreferenced.
	pub(crate) fn snapshot(&self) -> Snapshot<E> {
		Snapshot {
			tokens: self.token_snapshot(),
			depth: self.depth,
			ext: self.ext.save(),
		}
	}

	pub(crate) fn restore(&mut self, snapshot: Snapshot<E>) {
		self.restore_tokens(snapshot.tokens);
		self.depth = snapshot.depth;
		self.ext.restore(snapshot.ext);
	}

	/// The tokenizer alone, enough for a lookahead that parses nothing.
	pub(crate) fn token_snapshot(&self) -> TokenSnapshot {
		TokenSnapshot {
			pos: self.lexer.pos(),
			in_type: self.lexer.in_type,
			tok: self.tok,
			prev_end: self.prev_end,
			comments: self.lexer.comments.len(),
		}
	}

	pub(crate) fn restore_tokens(&mut self, snapshot: TokenSnapshot) {
		self.lexer.set_pos(snapshot.pos);
		self.lexer.in_type = snapshot.in_type;
		self.tok = snapshot.tok;
		self.prev_end = snapshot.prev_end;
		self.lexer.comments.truncate(snapshot.comments);
	}

	/// Runs `f`, undoing it when it fails.
	pub(crate) fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Option<T> {
		let snapshot = self.snapshot();
		match f(self) {
			Ok(value) => Some(value),
			Err(_) => {
				self.restore(snapshot);
				None
			}
		}
	}

	pub(crate) fn peek_token(&mut self) -> Result<Token> {
		let escaped = self.lexer.escaped();
		let token = self.lexer.peek_token();
		self.lexer.set_escaped(escaped);
		token
	}

	#[allow(dead_code)]
	fn peek_token_raw(&mut self) -> Result<Token> {
		self.lexer.peek_token()
	}

	/// Re-reads the current token, after the lexer's mode changed under it.
	pub(crate) fn relex(&mut self) -> Result<()> {
		let newline_before = self.tok.newline_before;
		self.lexer.set_pos(self.tok.start);
		self.tok = self.lexer.next_token()?;
		self.tok.newline_before = newline_before;
		Ok(())
	}

	fn finish(self) -> Ast<E::Data> {
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

	/// Guards a loop that nests each iteration's node inside the previous one.
	pub(crate) fn chain(&self, links: u32) -> Result<()> {
		if links > MAX_CHAIN {
			return self.error(self.tok.start, "Maximum nesting depth exceeded");
		}
		Ok(())
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
			NodeKind::Extension(_) => {
				E::unwrap(self, id, Unwrap::Simple).is_some_and(|inner| self.is_simple_assign_target(inner))
			}
			_ => false,
		}
	}
}
