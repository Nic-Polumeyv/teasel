use crate::error::Code;
pub(crate) mod expression;
pub(crate) mod scope;
pub(crate) mod statement;

#[cfg(test)]
pub(crate) mod tests;

use crate::ast::{Ast, List, MethodKind, NodeId, NodeKind, Reuse, VariableKind};
use crate::error::SyntaxError;
use crate::interner::{FastMap, FastSet, StrId};
use crate::lexer::Lexer;
use crate::lexer::token::{Keyword, Token, TokenKind};
pub(crate) use expression::ForInit;
use scope::{SCOPE_TOP, Scope};
pub(crate) use statement::{ClassKind, Context, StatementPlace};

/// Errors travel boxed so every `Result` stays two words wide.
pub(crate) type Result<T> = std::result::Result<T, Box<SyntaxError>>;

const MAX_DEPTH: u32 = 1000;
/// Subscripts and binary operators chain without recursion, but the tree they build is as deep
/// as the chain is long, and everything that walks it recurses.
const MAX_CHAIN: u32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Decorators {
	#[default]
	Any,
	Legacy,
	Proposal,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
	/// Parse as an ES module: strict mode, top-level `await`, `import` and `export`.
	pub module: bool,
	/// Errors are recorded on the tree instead of ending the parse: a missing operand, name or
	/// pattern is an empty `Identifier` of no width where it was expected, and a statement or
	/// entry that cannot be read is skipped to the next stop token or unmatched closer, an empty
	/// `Identifier` standing for it.
	pub error_recovery: bool,
	pub allow_return_outside_function: bool,
	pub allow_await_outside_function: bool,
	pub allow_super_outside_method: bool,
	pub allow_undeclared_exports: bool,
	/// Mark a node the source wraps in parens with the fact `parenthesized`, instead of a wrapper node.
	pub parenthesized: bool,
	/// Which decorators are read; `Any` reads both the proposal's and the legacy syntax.
	pub decorators: Decorators,
}

/// What a function-shaped node is, for the extension hooks around its signature.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionKind {
	Declaration,
	Expression,
	Method { in_class: bool, kind: MethodKind },
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
	type Data: Reuse;
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

	/// Whether the current token, one of the host's stop tokens, is one the extension's grammar
	/// reads after an expression; it then decides where the host's syntax starts.
	fn reads_stop(p: &Parser<Self>) -> bool {
		false
	}
	/// First look at a statement; `Some` replaces it entirely.
	fn statement(p: &mut Parser<Self>, context: Context, place: StatementPlace) -> Result<Option<NodeId>> {
		Ok(None)
	}
	/// An expression statement whose expression is a bare identifier may be a declaration instead.
	fn expression_statement(
		p: &mut Parser<Self>,
		start: u32,
		expression: NodeId,
		context: Context,
	) -> Result<Option<NodeId>> {
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
	/// Whether an `export` here belongs to the extension's own scoping, a namespace body say, so
	/// the script goal does not forbid it.
	fn exports_in_script(p: &Parser<Self>) -> bool {
		let _ = p;
		false
	}
	/// Whether the code describes rather than defines, an ambient declaration's body, where a
	/// definition's rules do not apply.
	fn in_ambient(p: &Parser<Self>) -> bool {
		let _ = p;
		false
	}
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
	/// After a pattern read on its own, the `Pattern` entry.
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
	fn class_start(p: &mut Parser<Self>, kind: ClassKind) -> Result<()> {
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
	/// A type parameter list `<...>` at the cursor, for the entry that parses one on its own.
	fn type_parameters(p: &mut Parser<Self>) -> Result<NodeId> {
		p.error(p.tok.start, Code::NotTypeScript)
	}
	/// After the key of a class element.
	fn class_key_end(p: &mut Parser<Self>, key: NodeId, computed: bool) -> Result<()> {
		Ok(())
	}
	fn starts_class_method(p: &mut Parser<Self>) -> bool {
		false
	}
	fn class_method_start(p: &mut Parser<Self>, kind: MethodKind) -> Result<()> {
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
	/// A finished list of expressions: array elements, arguments, or a parenthesized expression
	/// that did not become parameters.
	fn list_items(p: &mut Parser<Self>, items: &[Option<NodeId>]) -> Result<()> {
		Ok(())
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

/// What a parse reads at its offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Entry {
	#[default]
	Program,
	Expression,
	Pattern,
	Params,
	Statement,
	TypeParameters,
}

impl Entry {
	pub fn from_index(index: u32) -> Entry {
		match index {
			1 => Entry::Expression,
			2 => Entry::Pattern,
			3 => Entry::Params,
			4 => Entry::Statement,
			5 => Entry::TypeParameters,
			_ => Entry::Program,
		}
	}
}

/// Parses one `entry` starting at `start` of a source cut at `end`, with positions of the whole:
/// a program, or what a host embedding JavaScript in a larger syntax reads at a point of it. A
/// program reads to `end`, where a hashbang or an HTML comment at `start` is what it would be in
/// the middle of a file; anything else stops where its grammar ends, or at one of `stop`, the
/// host's own tokens separated by spaces, read outside every bracket the parse opened where the
/// expression could end (an extension may read one as its own, see `Extension::reads_stop`). Returns
/// the tree, its roots (one node, or the patterns of a parameter list) and the offset after
/// everything the parse consumed. `reused` is an emptied tree from an earlier parse, its room kept.
/// The tree, whether the parse succeeded or not, so the next parse can reuse it; with the roots
/// read and where the parse ended.
pub(crate) fn parse_at<E: Extension>(
	src: &str,
	start: u32,
	end: Option<u32>,
	entry: Entry,
	options: Options,
	stop: &str,
	reused: Option<Ast<E::Data>>,
) -> (Ast<E::Data>, Result<(List, u32)>) {
	let end = end.unwrap_or(src.len() as u32);
	let src = &src[..end as usize];
	let budget = if entry == Entry::Program {
		(end - start) as usize
	} else {
		0
	};
	let ast = reused.unwrap_or_else(|| Ast::sized(budget));
	let mut parser = Parser::<E>::new(src, start, options, budget, stop, ast);
	let roots = parser.read_roots(entry).map(|roots| {
		let roots = parser.list_of(&roots);
		let end = if entry == Entry::Program {
			end
		} else {
			parser.consumed_end()
		};
		(roots, end)
	});
	(parser.finish(), roots)
}

impl<E: Extension> Parser<'_, E> {
	fn read_roots(&mut self, entry: Entry) -> Result<Vec<NodeId>> {
		self.start()?;
		if entry == Entry::Program {
			return Ok(vec![self.parse_program()?]);
		}
		match self.read_entry(entry) {
			// under recovery, what was read is skipped and an empty identifier stands where it failed
			Err(error) if self.recovering() => {
				let at = error.pos;
				self.record(Err(error)).unwrap();
				self.skip_to_end();
				self.prev_end = self.prev_end.max(at);
				if entry == Entry::Params {
					Ok(Vec::new())
				} else {
					let name = self.intern("");
					Ok(vec![self.add_with_end(NodeKind::Identifier { name }, at, at)])
				}
			}
			result => result,
		}
	}
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
	/// Name vectors of scopes left, for the next scope entered.
	spare_names: Vec<Vec<(StrId, u8)>>,
	/// List buffers earlier lists gave back: a list costs no allocation after the first at its depth.
	spare_lists: Vec<Vec<Option<NodeId>>>,
	param_names: Vec<StrId>,
	labels: Vec<Label>,
	private_names: Vec<PrivateNameScope>,
	pub(crate) undeclared_exports: FastMap<StrId, (u32, usize)>,
	pub(crate) yield_pos: u32,
	pub(crate) await_pos: u32,
	pub(crate) await_ident_pos: u32,
	pub(crate) potential_arrow_at: u32,
	potential_arrow_in_for_await: bool,
	/// Recovered errors; the lexer keeps its own until `finish`.
	pub(crate) errors: Vec<SyntaxError>,
	/// Inside a speculation, where the parse must fail as strict parsing would, not recover.
	speculating: u32,
	/// More nodes than this is a parse that stopped consuming input.
	tree_limit: usize,
}

/// The buffers a parse works in, handed on through the tree so the next parse allocates none.
#[derive(Default)]
pub struct Spare {
	scopes: Vec<Scope>,
	names: Vec<Vec<(StrId, u8)>>,
	lists: Vec<Vec<Option<NodeId>>>,
	param_names: Vec<StrId>,
	labels: Vec<Label>,
	private_names: Vec<PrivateNameScope>,
	errors: Vec<SyntaxError>,
	regexp: crate::lexer::regexp::Scratch,
}

impl std::fmt::Debug for Spare {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Spare")
	}
}

/// Enough to unwind the parser after an error inside a statement it recovers from: what the
/// failed parse built is forgotten with it.
struct Mark<E: Extension> {
	scopes: usize,
	labels: usize,
	private_names: usize,
	depth: u32,
	brackets: (u32, [u32; 3]),
	strict: bool,
	ext: E::Snapshot,
	ast: crate::ast::Mark<<E::Data as Reuse>::Mark>,
}

/// Enough to retry a speculative parse from here: the mark, and the tokens back at it.
pub(crate) struct Snapshot<E: Extension> {
	tokens: TokenSnapshot,
	mark: Mark<E>,
}

pub(crate) struct TokenSnapshot {
	pos: u32,
	in_type: bool,
	depth: u32,
	open: [u32; 3],
	stopped: bool,
	unmatched: bool,
	tok: Token,
	prev_end: u32,
	comments: usize,
	errors: usize,
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
	pub(crate) fn new(
		src: &'a str,
		offset: u32,
		options: Options,
		budget: usize,
		stop: &'a str,
		mut ast: Ast<E::Data>,
	) -> Self {
		let mut lexer = Lexer::with(src, budget, std::mem::take(&mut ast.strings));
		lexer.comments = std::mem::take(&mut ast.comments);
		lexer.set_pos(offset);
		lexer.stops = stop;
		lexer.recover = options.error_recovery;
		let strict = options.module || expression::strict_directive(src, offset);
		lexer.strict = strict;
		lexer.module = options.module;
		let spare = std::mem::take(&mut ast.spare);
		lexer.regexp = spare.regexp;
		let mut parser = Self {
			lexer,
			ast,
			ext: E::default(),
			options,
			tok: Token::eof(offset),
			prev_end: offset,
			strict,
			depth: 0,
			scopes: spare.scopes,
			spare_names: spare.names,
			spare_lists: spare.lists,
			param_names: spare.param_names,
			labels: spare.labels,
			private_names: spare.private_names,
			undeclared_exports: FastMap::default(),
			yield_pos: 0,
			await_pos: 0,
			await_ident_pos: 0,
			potential_arrow_at: u32::MAX,
			potential_arrow_in_for_await: false,
			errors: spare.errors,
			speculating: 0,
			tree_limit: 16 * src.len() + 256,
		};
		E::init(&mut parser);
		parser
	}

	/// The first token, read before anything is parsed.
	pub(crate) fn start(&mut self) -> Result<()> {
		self.lexer.next_token_into(&mut self.tok)?;
		// the host's token first: nothing to read, which is the host's to report
		self.stop_after_operand();
		Ok(())
	}

	pub(crate) fn snapshot(&self) -> Snapshot<E> {
		Snapshot {
			tokens: self.token_snapshot(),
			mark: self.mark(),
		}
	}

	pub(crate) fn restore(&mut self, snapshot: Snapshot<E>) {
		self.restore_tokens(snapshot.tokens);
		self.unwind(snapshot.mark);
	}

	/// The tokenizer alone, enough for a lookahead that parses nothing.
	pub(crate) fn token_snapshot(&self) -> TokenSnapshot {
		TokenSnapshot {
			pos: self.lexer.pos(),
			in_type: self.lexer.in_type,
			depth: self.lexer.depth,
			open: self.lexer.open,
			stopped: self.lexer.stopped,
			unmatched: self.lexer.unmatched,
			tok: self.tok,
			prev_end: self.prev_end,
			comments: self.lexer.comments.len(),
			errors: self.lexer.errors.len(),
		}
	}

	pub(crate) fn restore_tokens(&mut self, snapshot: TokenSnapshot) {
		self.lexer.set_pos(snapshot.pos);
		self.lexer.in_type = snapshot.in_type;
		self.lexer.depth = snapshot.depth;
		self.lexer.open = snapshot.open;
		self.lexer.stopped = snapshot.stopped;
		self.lexer.unmatched = snapshot.unmatched;
		self.tok = snapshot.tok;
		self.prev_end = snapshot.prev_end;
		self.lexer.comments.truncate(snapshot.comments);
		self.lexer.errors.truncate(snapshot.errors);
	}

	/// A tree past what the source can hold is a loop that consumes nothing: it fails here,
	/// cheaply, instead of growing until the machine runs out of memory.
	pub(crate) fn within_limit(&self) -> Result<()> {
		if self.ast.nodes.len() > self.tree_limit {
			return self.error(self.tok.start, Code::TreeSize);
		}
		Ok(())
	}

	pub(crate) fn recovering(&self) -> bool {
		self.options.error_recovery && self.speculating == 0
	}

	/// Runs `f` with recovery off, so it fails where strict parsing would and the caller can try
	/// something else.
	pub(crate) fn speculate<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
		self.speculating += 1;
		let result = f(self);
		self.speculating -= 1;
		result
	}

	/// Under recovery an error is recorded, once per position, and the parse goes on with the
	/// tree it has.
	pub(crate) fn record(&mut self, error: Result<()>) -> Result<()> {
		let Err(error) = error else { return Ok(()) };
		if !self.recovering() {
			return Err(error);
		}
		if self
			.errors
			.last()
			.is_none_or(|last| (last.pos, last.code) != (error.pos, error.code))
		{
			self.errors.push(*error);
		}
		Ok(())
	}

	/// Records what `unexpected` would raise.
	pub(crate) fn report_unexpected(&mut self) {
		let error = self.unexpected();
		self.record(error).unwrap();
	}

	/// Where the grammar needs a token it does not have: an empty identifier of no width there.
	pub(crate) fn placeholder(&mut self) -> Result<NodeId> {
		self.within_limit()?;
		if !self.recovering() {
			return self.unexpected();
		}
		self.report_unexpected();
		let name = self.intern("");
		let at = self.tok.start;
		self.prev_end = at;
		Ok(self.add_with_end(NodeKind::Identifier { name }, at, at))
	}

	fn mark(&self) -> Mark<E> {
		Mark {
			scopes: self.scopes.len(),
			labels: self.labels.len(),
			private_names: self.private_names.len(),
			depth: self.depth,
			brackets: (self.lexer.depth, self.lexer.open),
			strict: self.strict,
			ext: self.ext.save(),
			ast: self.ast.mark(),
		}
	}

	fn unwind(&mut self, mark: Mark<E>) {
		while self.scopes.len() > mark.scopes {
			self.exit_scope();
		}
		self.labels.truncate(mark.labels);
		self.private_names.truncate(mark.private_names);
		self.depth = mark.depth;
		(self.lexer.depth, self.lexer.open) = mark.brackets;
		self.set_strict(mark.strict);
		self.ext.restore(mark.ext);
		self.ast.truncate(mark.ast);
	}

	/// A statement of a list, under recovery: when it fails, what was read is skipped up to the
	/// next `;`, a line that starts with a keyword, or the bracket that closes the list.
	pub(crate) fn statement_recovered(
		&mut self,
		f: impl FnOnce(&mut Self) -> Result<NodeId>,
	) -> Result<Option<NodeId>> {
		if !self.recovering() {
			return f(self).map(Some);
		}
		let mark = self.mark();
		let at = self.tok.start;
		match f(self) {
			Ok(statement) => Ok(Some(statement)),
			Err(error) => {
				self.record(Err(error)).unwrap();
				self.unwind(mark);
				let mut skipped = self.tok.start != at;
				loop {
					if self.is(TokenKind::Eof) || self.is(TokenKind::BraceR) {
						break;
					}
					if self.eat(TokenKind::Semi)? {
						break;
					}
					if skipped && self.tok.newline_before && matches!(self.tok.kind, TokenKind::Keyword(_)) {
						break;
					}
					self.next_liberal()?;
					skipped = true;
				}
				Ok(None)
			}
		}
	}

	/// An element of a list that consumed nothing under recovery would repeat forever: its token
	/// is skipped as unexpected.
	pub(crate) fn ensure_progress(&mut self, at: u32) -> Result<()> {
		if self.recovering() && self.tok.start == at {
			if self.is(TokenKind::Eof) {
				return self.unexpected();
			}
			self.report_unexpected();
			self.next()?;
		}
		Ok(())
	}

	/// Reads one entry other than a program at the current token, in a scope of its own.
	pub(crate) fn read_entry(&mut self, entry: Entry) -> Result<Vec<NodeId>> {
		self.enter_scope(SCOPE_TOP);
		match entry {
			Entry::Expression => self.parse_sequence(ForInit::No, &mut None).map(|id| vec![id]),
			Entry::Pattern => self.parse_pattern_root().map(|id| vec![id]),
			Entry::Params => self.parse_params_root(),
			Entry::Statement => {
				let mut exports = FastSet::default();
				self.parse_statement(statement::Context::None, StatementPlace::TopLevel, Some(&mut exports))
					.map(|id| vec![id])
			}
			Entry::TypeParameters => E::type_parameters(self).map(|id| vec![id]),
			Entry::Program => unreachable!(),
		}
	}

	/// An assignment target on its own: an identifier or a destructuring pattern, as it would
	/// appear on the left of `=`.
	fn parse_pattern_root(&mut self) -> Result<NodeId> {
		let mut errors = Some(DestructuringErrors::default());
		let expression = match self.tok.kind {
			TokenKind::BraceL | TokenKind::BracketL => self.parse_expr_atom(&mut errors, ForInit::No, false)?,
			_ => self.parse_ident(false)?,
		};
		let pattern = self.make_pattern(expression, false, &mut errors)?;
		self.check_lval_pattern(pattern, scope::Binding::None, &mut None)?;
		E::pattern_annotation(self, pattern)?;
		Ok(pattern)
	}

	/// A parenthesized parameter list on its own, read as an arrow function's would be: as
	/// expressions in the enclosing scope, reinterpreted as patterns once the list is complete.
	fn parse_params_root(&mut self) -> Result<Vec<NodeId>> {
		self.expect(TokenKind::ParenL)?;
		let paren = self.parse_paren_items()?;
		self.check_pattern_errors(&paren.errors, false)?;
		self.check_yield_await_in_default_params()?;
		self.enter_scope(scope::function_flags(false, false) | scope::SCOPE_ARROW);
		let params = self.make_patterns(paren.items, true)?;
		let list = self.list(&params);
		self.check_params(list, false)?;
		Ok(params.into_iter().flatten().collect())
	}

	/// Skips to the end of the input, a stop token or a bracket nothing here opened.
	pub(crate) fn skip_to_end(&mut self) {
		while !self.is(TokenKind::Eof) && !self.lexer.unmatched {
			if self.next_liberal().is_err() {
				break;
			}
		}
	}

	/// Runs `f`, undoing it when it fails.
	pub(crate) fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Option<T> {
		let snapshot = self.snapshot();
		match self.speculate(f) {
			Ok(value) => Some(value),
			Err(_) => {
				self.restore(snapshot);
				None
			}
		}
	}

	pub(crate) fn peek_token(&mut self) -> Result<Token> {
		self.lexer.peek_token()
	}

	/// Between the items of a comma list: the comma after the first item, and whether a trailing
	/// comma closed the list.
	pub(crate) fn list_comma(&mut self, close: TokenKind, first: &mut bool, allow_trailing: bool) -> Result<bool> {
		if std::mem::take(first) {
			return Ok(false);
		}
		self.expect(TokenKind::Comma)?;
		Ok(allow_trailing && self.after_trailing_comma(close, false)?)
	}

	/// Clears the pending yield and await positions for a nested parameter list.
	pub(crate) fn take_yield_await(&mut self) -> (u32, u32, u32) {
		let old = (self.yield_pos, self.await_pos, self.await_ident_pos);
		(self.yield_pos, self.await_pos, self.await_ident_pos) = (0, 0, 0);
		old
	}

	pub(crate) fn restore_yield_await(&mut self, (yield_pos, await_pos, await_ident_pos): (u32, u32, u32)) {
		(self.yield_pos, self.await_pos, self.await_ident_pos) = (yield_pos, await_pos, await_ident_pos);
	}

	/// Restores what was set before; what the nested list found stays otherwise.
	pub(crate) fn restore_yield_await_if_set(&mut self, (yield_pos, await_pos, await_ident_pos): (u32, u32, u32)) {
		if yield_pos != 0 {
			self.yield_pos = yield_pos;
		}
		if await_pos != 0 {
			self.await_pos = await_pos;
		}
		if await_ident_pos != 0 {
			self.await_ident_pos = await_ident_pos;
		}
	}

	pub(crate) fn nth(&self, list: List, i: u32) -> Option<NodeId> {
		self.ast.nth(list, i)
	}

	/// Re-reads the current token, after the lexer's mode changed under it.
	pub(crate) fn relex(&mut self) -> Result<()> {
		debug_assert!(!matches!(
			self.tok.kind,
			TokenKind::BraceL
				| TokenKind::ParenL
				| TokenKind::BracketL
				| TokenKind::BraceR
				| TokenKind::ParenR
				| TokenKind::BracketR
		));
		let newline_before = self.tok.newline_before;
		self.lexer.set_pos(self.tok.start);
		self.lexer.next_token_into(&mut self.tok)?;
		self.tok.newline_before = newline_before;
		Ok(())
	}

	/// The offset after the last token read, or after the comments that follow it before the
	/// next token: where a host embedding JavaScript resumes its own syntax.
	pub(crate) fn consumed_end(&self) -> u32 {
		match self.lexer.comments.last() {
			Some(comment) if comment.start >= self.prev_end => comment.end,
			_ => self.prev_end,
		}
	}

	pub(crate) fn finish(self) -> Ast<E::Data> {
		let mut ast = self.ast;
		let mut lexer = self.lexer;
		ast.comments = std::mem::take(&mut lexer.comments);
		ast.strings = std::mem::take(&mut lexer.strings);
		let mut errors = self.errors;
		ast.errors.append(&mut errors);
		ast.errors.append(&mut lexer.errors);
		ast.errors.sort_by_key(|error| error.pos);
		let mut scopes = self.scopes;
		scopes.clear();
		let mut labels = self.labels;
		labels.clear();
		let mut private_names = self.private_names;
		private_names.clear();
		ast.spare = Spare {
			scopes,
			names: self.spare_names,
			lists: self.spare_lists,
			param_names: self.param_names,
			labels,
			private_names,
			errors,
			regexp: std::mem::take(&mut lexer.regexp),
		};
		ast
	}

	pub(crate) fn source(&self) -> &'a str {
		self.lexer.source()
	}

	/// An error at the current token spans it; one elsewhere is a point.
	pub(crate) fn error<T>(&self, pos: u32, code: Code) -> Result<T> {
		self.error_with(pos, code, code.message())
	}

	pub(crate) fn error_with<T>(
		&self,
		pos: u32,
		code: Code,
		message: impl Into<std::borrow::Cow<'static, str>>,
	) -> Result<T> {
		let end = if pos == self.tok.start { self.tok.end } else { pos };
		Err(Box::new(SyntaxError::with(pos, code, message).to(end)))
	}

	/// The error for a code whose message has one placeholder.
	pub(crate) fn error_arg<T>(&self, pos: u32, code: Code, arg: impl std::fmt::Display) -> Result<T> {
		self.error_with(pos, code, code.with(&arg.to_string()))
	}

	/// The same with the name of an interned string.
	pub(crate) fn error_name<T>(&self, pos: u32, code: Code, name: StrId) -> Result<T> {
		self.error_arg(pos, code, self.str(name))
	}

	/// The current token is not what the grammar allows; at the end of the input that is its own error.
	pub(crate) fn unexpected<T>(&self) -> Result<T> {
		let code = if self.is(TokenKind::Eof) && !self.lexer.stopped {
			Code::UnexpectedEof
		} else {
			Code::UnexpectedToken
		};
		self.error(self.tok.start, code)
	}

	pub(crate) fn unexpected_at<T>(&self, pos: u32) -> Result<T> {
		self.error(pos, Code::UnexpectedToken)
	}

	pub(crate) fn str(&self, id: StrId) -> &str {
		self.lexer.strings.get(id)
	}

	pub(crate) fn intern(&mut self, s: &str) -> StrId {
		self.lexer.strings.intern(s)
	}

	pub(crate) fn add(&mut self, kind: NodeKind, start: u32) -> NodeId {
		self.add_with_end(kind, start, self.prev_end)
	}

	pub(crate) fn add_with_end(&mut self, kind: NodeKind, start: u32, end: u32) -> NodeId {
		self.ast.add(kind, start, end)
	}

	pub(crate) fn list(&mut self, items: &[Option<NodeId>]) -> List {
		self.ast.add_list(items)
	}

	pub(crate) fn items(&mut self) -> Vec<Option<NodeId>> {
		self.spare_lists.pop().unwrap_or_default()
	}

	pub(crate) fn recycle(&mut self, mut items: Vec<Option<NodeId>>) {
		items.clear();
		if self.spare_lists.len() < 32 {
			self.spare_lists.push(items);
		}
	}

	pub(crate) fn list_from(&mut self, items: Vec<Option<NodeId>>) -> List {
		let list = self.list(&items);
		self.recycle(items);
		list
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
			return self.error(self.tok.start, Code::NestingDepth);
		}
		Ok(())
	}

	pub(crate) fn leave(&mut self) {
		self.depth -= 1;
	}

	/// Guards a loop that nests each iteration's node inside the previous one.
	pub(crate) fn chain(&self, links: u32) -> Result<()> {
		if links > MAX_CHAIN {
			return self.error(self.tok.start, Code::NestingDepth);
		}
		Ok(())
	}

	// Tokens

	/// Consumes the current token; an escaped keyword is an error unless consumed as a name.
	pub(crate) fn next(&mut self) -> Result<()> {
		if self.tok.escaped
			&& let TokenKind::Keyword(keyword) = self.tok.kind
		{
			let error = self.error_arg(self.tok.start, Code::EscapeInKeyword, keyword.as_str());
			self.record(error)?;
		}
		self.next_liberal()
	}

	pub(crate) fn next_liberal(&mut self) -> Result<()> {
		self.within_limit()?;
		let boundary = self.tok.ends_operand();
		self.prev_end = self.tok.end;
		self.lexer.next_token_into(&mut self.tok)?;
		if boundary {
			self.stop_after_operand();
		}
		Ok(())
	}

	/// Ends the parse when the current token is the host's: one of its stop tokens, after an
	/// operand, that the grammar does not read as its own.
	pub(crate) fn stop_after_operand(&mut self) {
		if self.tok.stop && !E::reads_stop(self) {
			self.stop_here();
		}
	}

	/// Ends the parse at the current token: the host's own syntax starts there.
	pub(crate) fn stop_here(&mut self) {
		self.lexer.set_pos(self.tok.start);
		self.lexer.stopped = true;
		self.tok = Token::eof(self.tok.start);
	}

	pub(crate) fn is(&self, kind: TokenKind) -> bool {
		self.tok.kind == kind
	}

	pub(crate) fn is_keyword(&self, keyword: Keyword) -> bool {
		self.tok.kind == TokenKind::Keyword(keyword)
	}

	pub(crate) fn eat(&mut self, kind: TokenKind) -> Result<bool> {
		self.within_limit()?;
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
			let error = self.unexpected();
			self.record(error)?;
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
			return self.error(pos, Code::ShorthandAssignment);
		}
		if let Some(pos) = errors.double_proto {
			return self.error(pos, Code::DuplicateProto);
		}
		Ok(false)
	}

	pub(crate) fn check_pattern_errors(&self, errors: &Option<DestructuringErrors>, is_assign: bool) -> Result<()> {
		let Some(errors) = errors else { return Ok(()) };
		if let Some(pos) = errors.trailing_comma {
			return self.error(pos, Code::CommaAfterRest);
		}
		let (parens, code) = if is_assign {
			(errors.parenthesized_assign, Code::InvalidAssignmentTarget)
		} else {
			(errors.parenthesized_bind, Code::ParenthesizedPattern)
		};
		match parens {
			Some(pos) => self.error(pos, code),
			None => Ok(()),
		}
	}

	pub(crate) fn check_yield_await_in_default_params(&self) -> Result<()> {
		if self.yield_pos != 0 && (self.await_pos == 0 || self.yield_pos < self.await_pos) {
			return self.error(self.yield_pos, Code::YieldInDefaultValue);
		}
		if self.await_pos != 0 {
			return self.error(self.await_pos, Code::AwaitInDefaultValue);
		}
		Ok(())
	}

	pub(crate) fn is_simple_assign_target(&self, id: NodeId) -> bool {
		match self.kind(id) {
			NodeKind::Identifier { .. } | NodeKind::MemberExpression { .. } => true,
			NodeKind::Extension(_) => {
				E::unwrap(self, id, Unwrap::Simple).is_some_and(|inner| self.is_simple_assign_target(inner))
			}
			_ => false,
		}
	}
}
