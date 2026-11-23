//! LANE PROTOTYPE, milestone 2c: TypeScript in the new shape. What the parser in use reads in
//! `typescript/{mod,types,declarations}.rs`, read the same way: the same speculations at the
//! same places, so the answer is the same for what parses and for what does not.

use super::checks::{Binding, SCOPE_ARROW, SCOPE_FUNCTION, SCOPE_TS_MODULE, function_flags};
use super::parse::{Ctx, MAX_DEPTH, Parser};
use super::{ESCAPED, Kind, LexError, NEWLINE_BEFORE, Token};
use crate::ast::{
	Accessibility, Clause, Extras, FieldKind, Keyword as TsKeyword, Kind as ImportKind, List, MethodKind, Modifier,
	NodeId, NodeKind, SignatureKind, UnaryOperator, VariableKind,
};
use crate::error::{Code, SyntaxError};
use crate::interner::{FastMap, StrId};

/// What TypeScript keeps across a parse, apart from what a rewind puts back.
#[derive(Default)]
pub struct Ts {
	pub(super) state: State,
	colon_after_body: FastMap<u32, bool>,
	/// How many speculations ran, for the counts.
	pub snapshots: u64,
	pub rewinds: u64,
	pub token_marks: u64,
	pub peeks: u64,
}

impl Ts {
	pub(super) fn clear(&mut self) {
		self.state = State::default();
		// a cleared table writes all of its control bytes, empty or not
		if !self.colon_after_body.is_empty() {
			self.colon_after_body.clear();
		}
	}
}

#[derive(Clone, Default)]
pub(super) struct State {
	disallow_conditional_types: bool,
	infer_depth: u32,
	pub(super) ambient: bool,
	in_namespace: bool,
	global_depth: u32,
	in_abstract_class: bool,
	classes: Vec<ClassFrame>,
	elements: Vec<ElementFrame>,
	pub(super) arrow_return_type: Option<NodeId>,
	pub(super) needs_colon_at: Option<u32>,
	body_needs_colon: bool,
	pub(super) outer_kind: Option<ImportKind>,
	export_kind: Option<ImportKind>,
	next_class: ClassFrame,
	pub(super) module_blocks: u32,
	decorators: Vec<Vec<NodeId>>,
	decorators_before_export: bool,
}

#[derive(Clone, Copy, Default)]
pub(super) struct FunctionFrame {
	in_class_method: bool,
	type_parameters: Option<NodeId>,
	return_type: Option<NodeId>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ClassFrame {
	is_abstract: bool,
	declare: bool,
	is_declaration: bool,
	decorators: Option<List>,
	start: Option<u32>,
	type_parameters: Option<NodeId>,
	super_type_arguments: Option<NodeId>,
	implements: Option<List>,
	has_super: bool,
	outer_abstract: bool,
}

#[derive(Clone, Copy, Default)]
struct ElementFrame {
	start: u32,
	extras: Extras,
	key: Option<NodeId>,
	computed: bool,
	outer_ambient: Option<bool>,
	accessor: bool,
}

#[derive(Default, PartialEq)]
pub(super) struct Modifiers {
	pub extras: Extras,
	pub accessor: bool,
	pub is_in: bool,
	pub is_out: bool,
	pub is_const: bool,
}

macro_rules! modifiers {
	($($word:literal => $($field:ident).+ = $value:expr),* $(,)?) => {
		const MODIFIERS: &[&str] = &[$($word),*];

		impl Modifiers {
			fn has(&self, modifier: &str) -> bool {
				match modifier {
					$($word => self.$($field).+ == $value,)*
					_ => unreachable!(),
				}
			}

			fn set(&mut self, modifier: &str) {
				match modifier {
					$($word => self.$($field).+ = $value,)*
					_ => unreachable!(),
				}
			}
		}
	};
}

modifiers! {
	"public" => extras.accessibility = Some(Accessibility::Public),
	"private" => extras.accessibility = Some(Accessibility::Private),
	"protected" => extras.accessibility = Some(Accessibility::Protected),
	"declare" => extras.declare = true,
	"abstract" => extras.is_abstract = true,
	"override" => extras.is_override = true,
	"readonly" => extras.readonly = true,
	"accessor" => accessor = true,
	"static" => extras.is_static = true,
	"in" => is_in = true,
	"out" => is_out = true,
	"const" => is_const = true,
}

/// What a function is, for what TypeScript reads around it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FunctionKind {
	Declaration,
	Expression,
	Arrow,
	Method { in_class: bool, kind: MethodKind },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ClassKind {
	Declaration,
	NullableId,
	Expression,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListKind {
	EnumMembers,
	TypeMembers,
	HeritageClause,
	TupleElements,
	TypeParametersOrArguments,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeParameterModifiers {
	InOut,
	Const,
	Class,
}

const IN_OUT: &[&str] = &["in", "out"];
const IN_OUT_CONST: &[&str] = &["in", "out", "const"];
const ACCESSIBILITY_AND_CLASS: &[&str] = &[
	"public",
	"private",
	"protected",
	"readonly",
	"declare",
	"abstract",
	"override",
];

#[derive(Clone, Copy, Default)]
pub(super) struct Opened {
	declare: Option<u32>,
	is_abstract: bool,
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

/// The tokens alone: enough for a look ahead that builds nothing.
#[derive(Clone, Copy)]
pub(super) struct TokSnap {
	tok: Token,
	prev_end: u32,
	pos: usize,
	comments: usize,
	in_type: bool,
	lex_error: Option<LexError>,
	escaped_keyword: Option<(u32, u32)>,
	pub(super) brackets: super::entry::Brackets,
	lexed: usize,
}

/// What a statement's read adds to the parser's state, as it stood before it.
pub(super) struct Kept {
	scopes: usize,
	names: u32,
	labels: usize,
	private_names: usize,
	undeclared_exports: usize,
	depth: u32,
	strict: bool,
	in_type: bool,
	state: Option<State>,
	ast: crate::ast::Mark,
	items: usize,
	ctx: Ctx,
	brackets: super::entry::Brackets,
}

/// Enough to read again from here as if nothing had been read.
pub(super) struct Snap {
	tokens: TokSnap,
	error: bool,
	err: Option<SyntaxError>,
	err_at: u32,
	err_first: bool,
	scopes: usize,
	names: u32,
	labels: usize,
	private_names: usize,
	undeclared_exports: usize,
	depth: u32,
	strict: bool,
	state: Option<State>,
	ast: crate::ast::Mark,
	items: usize,
	ctx: Ctx,
	arrow_at: u32,
	arrow_for_await: bool,
	bare_arrow: bool,
}

impl<'a, const COUNT: bool, const TS: bool> Parser<'a, COUNT, TS> {
	// ── tokens

	#[inline(always)]
	pub(super) fn tnext(&mut self) {
		self.bump(false);
	}

	#[inline(always)]
	pub(super) fn eat(&mut self, kind: Kind) -> bool {
		if self.tok.kind == kind {
			self.bump(false);
			true
		} else {
			false
		}
	}

	pub(super) fn eat_word(&mut self, word: &[u8]) -> bool {
		if self.is_word(word) {
			self.bump(false);
			true
		} else {
			false
		}
	}

	fn expect_contextual(&mut self, word: &[u8]) {
		if !self.eat_word(word) {
			self.fail();
		}
	}

	fn is_ident(&self) -> bool {
		self.tok.kind == Kind::Ident
	}

	fn is_ident_or_keyword(&self) -> bool {
		self.tok.kind == Kind::Ident || self.tok.kind.is_keyword()
	}

	fn is_literal_property_name(&self) -> bool {
		matches!(self.tok.kind, Kind::Ident | Kind::String | Kind::Number | Kind::BigInt) || self.tok.kind.is_keyword()
	}

	fn tok_text(&self) -> &'a str {
		&self.src[self.tok.start as usize..self.tok.end as usize]
	}

	fn can_insert_semi(&self) -> bool {
		matches!(self.tok.kind, Kind::Eof | Kind::BraceR) || self.newline()
	}

	fn tsemicolon(&mut self) {
		if self.tok.kind == Kind::Semi {
			self.bump(true);
		} else if !self.can_insert_semi() {
			self.soft_unexpected();
		}
	}

	fn is_line_terminator(&mut self) -> bool {
		self.eat(Kind::Semi) || self.can_insert_semi()
	}

	fn kind_of(&self, id: NodeId) -> NodeKind {
		self.ast.node(id).kind
	}

	fn start_of(&self, id: NodeId) -> u32 {
		self.ast.node(id).start
	}

	fn end_of(&self, id: NodeId) -> u32 {
		self.ast.node(id).end
	}

	fn extras_mut(&mut self, id: NodeId) -> &mut Extras {
		self.ast.extras.entry(id)
	}

	/// An error with its own arguments, ending as every error does.
	fn raise_with(&mut self, error: SyntaxError) -> NodeId {
		let end = if error.pos == self.tok.start {
			match self.tok.kind {
				Kind::Template | Kind::TemplateHead => self.tok.start + 1,
				_ => self.tok.end,
			}
		} else {
			error.pos
		};
		self.record(error.to(end))
	}

	fn enter(&mut self) -> bool {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			self.raise(self.tok.start, Code::NestingDepth);
			return false;
		}
		true
	}

	/// The read failed: an error of the grammar's or the lexer's, or a keyword spelled with an
	/// escape read as the keyword.
	pub(super) fn failed(&self) -> bool {
		self.error
			|| self.lexer.error.is_some()
			|| self
				.lexer
				.escaped_keyword
				.is_some_and(|(start, _)| start < self.tok.start)
	}

	pub(super) fn limit_hit(&self) -> bool {
		self.err.is_some_and(|e| e.code.is_limit())
	}

	/// The token after the one at the cursor, nothing read. A text that does not lex there ends
	/// the parse with the lexer's error.
	pub(super) fn peek_tok(&mut self) -> Token {
		if COUNT {
			self.scratch.ts.peeks += 1;
		}
		let pos = self.lexer.pos();
		let comments = self.lexer.comments.len();
		let escaped = self.lexer.escaped_keyword;
		let had = self.lexer.error.is_some();
		// under recovery what does not lex is skipped, and recorded when it is read, not here
		let lexed = self.lexer.soft_len();
		let mut token = self.lexer.next(false);
		self.lexer.forget_errors(lexed);
		if self.in_type {
			token = self.split_angle(token);
		}
		if self.lexer.error.is_some() && !had {
			self.tok = self.lexer.end();
			return token;
		}
		self.lexer.rewind(pos);
		self.lexer.comments.truncate(comments);
		self.lexer.escaped_keyword = escaped;
		token
	}

	#[inline(always)]
	pub(super) fn split_angle(&mut self, token: Token) -> Token {
		match token.kind {
			Kind::LtLt
			| Kind::LtLtEq
			| Kind::LtEq
			| Kind::GtGt
			| Kind::GtGtGt
			| Kind::GtGtEq
			| Kind::GtGtGtEq
			| Kind::GtEq => self.lexer.angle(token),
			_ => token,
		}
	}

	/// The byte the next token starts with, in a strict read: a recovering one reads past a
	/// character no token starts with, so its next token is not known by one.
	pub(super) fn next_start(&self) -> Option<u8> {
		if self.lexer.recover {
			return None;
		}
		self.lexer.peek_byte()
	}

	fn peek_is_contextual(&mut self, name: &str) -> bool {
		if self
			.next_start()
			.is_some_and(|next| next != b'\\' && next != name.as_bytes()[0])
		{
			return false;
		}
		let t = self.peek_tok();
		t.kind == Kind::Ident && self.word_of(t) == name
	}

	/// What a word spells, an escape in it decoded.
	fn word_of(&mut self, t: Token) -> &str {
		if t.flags & ESCAPED != 0 {
			let from = if t.kind == Kind::PrivateName { t.start + 1 } else { t.start };
			return self.scratch.values.word(self.src, from as usize, t.end as usize);
		}
		&self.src[t.start as usize..t.end as usize]
	}

	pub(super) fn tok_snap(&mut self) -> TokSnap {
		if COUNT {
			self.scratch.ts.token_marks += 1;
		}
		TokSnap {
			tok: self.tok,
			prev_end: self.prev_end,
			pos: self.lexer.pos(),
			comments: self.lexer.comments.len(),
			in_type: self.in_type,
			lex_error: self.lexer.error,
			escaped_keyword: self.lexer.escaped_keyword,
			brackets: self.brackets(),
			lexed: self.lexer.soft_len(),
		}
	}

	pub(super) fn restore_tokens(&mut self, s: TokSnap) {
		self.lexer.rewind(s.pos);
		self.lexer.comments.truncate(s.comments);
		self.lexer.error = s.lex_error;
		self.lexer.escaped_keyword = s.escaped_keyword;
		self.in_type = s.in_type;
		self.tok = s.tok;
		self.prev_end = s.prev_end;
		self.set_brackets(s.brackets);
		self.lexer.forget_errors(s.lexed);
	}

	pub(super) fn snapshot(&mut self) -> Snap {
		self.scratch.ts.snapshots += 1;
		let tokens = self.tok_snap();
		if COUNT {
			self.scratch.ts.token_marks -= 1;
		}
		Snap {
			tokens,
			error: self.error,
			err: self.err,
			err_at: self.err_at,
			err_first: self.err_first,
			scopes: self.scratch.checks.scopes.len(),
			names: self.scratch.checks.mark_names(),
			labels: self.scratch.checks.labels.len(),
			private_names: self.scratch.checks.private_names.len(),
			undeclared_exports: self.scratch.checks.undeclared_exports.len(),
			depth: self.depth,
			strict: self.strict,
			state: TS.then(|| self.scratch.ts.state.clone()),
			ast: self.ast.mark(),
			items: self.scratch.items.len(),
			ctx: self.ctx,
			arrow_at: self.arrow_at,
			arrow_for_await: self.arrow_for_await,
			bare_arrow: self.bare_arrow,
		}
	}

	/// What a statement that fails under recovery leaves behind, to be forgotten: no cursor.
	pub(super) fn kept(&mut self) -> Kept {
		Kept {
			scopes: self.scratch.checks.scopes.len(),
			names: self.scratch.checks.mark_names(),
			labels: self.scratch.checks.labels.len(),
			private_names: self.scratch.checks.private_names.len(),
			undeclared_exports: self.scratch.checks.undeclared_exports.len(),
			depth: self.depth,
			strict: self.strict,
			in_type: self.in_type,
			state: TS.then(|| self.scratch.ts.state.clone()),
			ast: self.ast.mark(),
			items: self.scratch.items.len(),
			ctx: self.ctx,
			brackets: self.brackets(),
		}
	}

	/// What `s` holds of the parser's state put back, the cursor left where it is: a statement
	/// that failed under recovery is forgotten, what follows it read on.
	pub(super) fn unwind(&mut self, s: Kept) {
		let brackets = s.brackets;
		self.scratch.checks.unwind(s.scopes, s.names);
		self.scratch.checks.labels.truncate(s.labels);
		self.scratch.checks.private_names.truncate(s.private_names);
		self.scratch.checks.undeclared_exports.truncate(s.undeclared_exports);
		self.depth = s.depth;
		self.set_strict(s.strict);
		self.in_type = s.in_type;
		if let Some(state) = s.state {
			self.scratch.ts.state = state;
		}
		self.ast.truncate(s.ast);
		self.scratch.items.truncate(s.items);
		self.ctx = s.ctx;
		let (unmatched, stop_hit) = (self.unmatched, self.stop_hit);
		self.set_brackets(brackets);
		(self.unmatched, self.stop_hit) = (unmatched, stop_hit);
	}

	pub(super) fn restore(&mut self, s: Snap) {
		self.scratch.ts.rewinds += 1;
		self.restore_tokens(s.tokens);
		self.error = s.error;
		self.err = s.err;
		self.err_at = s.err_at;
		self.err_first = s.err_first;
		self.scratch.checks.unwind(s.scopes, s.names);
		self.scratch.checks.labels.truncate(s.labels);
		self.scratch.checks.private_names.truncate(s.private_names);
		self.scratch.checks.undeclared_exports.truncate(s.undeclared_exports);
		self.depth = s.depth;
		self.set_strict(s.strict);
		if let Some(state) = s.state {
			self.scratch.ts.state = state;
		}
		self.ast.truncate(s.ast);
		self.scratch.items.truncate(s.items);
		self.ctx = s.ctx;
		self.arrow_at = s.arrow_at;
		self.arrow_for_await = s.arrow_for_await;
		self.bare_arrow = s.bare_arrow;
	}

	/// `f` read from here; when it fails, the parser back where it was and none, unless a
	/// limit was hit, which stays the parse's error.
	pub(super) fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Option<T> {
		let snapshot = self.snapshot();
		self.speculating += 1;
		let value = f(self);
		self.speculating -= 1;
		if !self.failed() {
			return Some(value);
		}
		if self.limit_hit() {
			return None;
		}
		self.restore(snapshot);
		None
	}

	/// `f` read from here and the parser put back; an error in it is the parse's.
	fn lookahead<T: Default>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
		let snapshot = self.snapshot();
		self.speculating += 1;
		let value = f(self);
		self.speculating -= 1;
		if self.failed() {
			let lexed = self.lexer.error;
			let err = self.err;
			let escaped = self.lexer.escaped_keyword;
			self.restore(snapshot);
			self.reraise(lexed, err, escaped);
			return T::default();
		}
		self.restore(snapshot);
		value
	}

	fn reraise(&mut self, lexed: Option<LexError>, err: Option<SyntaxError>, escaped: Option<(u32, u32)>) {
		if let Some(e) = err {
			self.record(e);
		} else if lexed.is_some() {
			self.lexer.error = lexed;
			self.tok = self.lexer.end();
		} else if let Some((start, end)) = escaped {
			let word = self.scratch.values.word(self.src, start as usize, end as usize);
			let arg = self.ast.strings.intern(word);
			self.record(SyntaxError::new(start, Code::EscapeInKeyword).to(end).arg(arg));
		}
	}

	/// The same, an error in it meaning no: only a limit stays an error.
	fn lookahead_soft(&mut self, f: impl FnOnce(&mut Self) -> bool) -> bool {
		let snapshot = self.snapshot();
		self.speculating += 1;
		let value = f(self);
		self.speculating -= 1;
		if self.failed() {
			let limit = self.limit_hit().then_some(self.err).flatten();
			self.restore(snapshot);
			if let Some(e) = limit {
				self.record(e);
			}
			return false;
		}
		self.restore(snapshot);
		value
	}

	/// Tokens read by `f` and put back; an error in it is the parse's.
	fn peek_tokens<T: Default>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
		let snapshot = self.tok_snap();
		let value = f(self);
		if self.failed() {
			return T::default();
		}
		self.restore_tokens(snapshot);
		value
	}

	/// `f` read from here, kept when it answers; an error in it is the parse's.
	fn try_parse<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
		let snapshot = self.snapshot();
		self.speculating += 1;
		let value = f(self);
		self.speculating -= 1;
		if self.failed() {
			return None;
		}
		if value.is_none() {
			self.restore(snapshot);
		}
		value
	}

	fn split_lt(&mut self) {
		self.tok = self.lexer.angle(self.tok);
	}

	/// At the start of an expression a lone `<` is read whatever follows it.
	pub(super) fn starts_with_lt(&mut self) -> bool {
		match self.tok.kind {
			Kind::Lt => true,
			Kind::LtLt | Kind::LtEq | Kind::LtLtEq => {
				self.split_lt();
				true
			}
			_ => false,
		}
	}

	/// The token at the cursor read again as the place it stands in now reads it.
	fn relex(&mut self) {
		let newline = self.tok.flags & NEWLINE_BEFORE;
		self.lexer.rewind(self.tok.start as usize);
		let mut token = self.lexer.next(false);
		if self.in_type {
			token = self.split_angle(token);
		}
		token.flags |= newline;
		self.tok = token;
		if self.track {
			self.restop();
		}
	}

	fn rescan_lt_gt(&mut self) {
		if matches!(self.tok.kind, Kind::Lt | Kind::Gt) {
			self.relex();
		}
		// a type context ends an operand, so the token after it may be the host's
		if self.track {
			self.stop_after_operand();
		}
	}

	// ── lists

	fn list_end(kind: ListKind) -> Kind {
		match kind {
			ListKind::EnumMembers | ListKind::TypeMembers => Kind::BraceR,
			ListKind::HeritageClause => Kind::BraceL,
			ListKind::TupleElements => Kind::BracketR,
			ListKind::TypeParametersOrArguments => Kind::Gt,
		}
	}

	/// Items with commas between up to the list's end, pushed: where they start.
	fn parse_delimited_list(&mut self, kind: ListKind, mut element: impl FnMut(&mut Self) -> NodeId) -> usize {
		let end = Self::list_end(kind);
		let base = self.scratch.items.len();
		loop {
			if self.tok.kind == end {
				break;
			}
			let item = element(self);
			if self.error {
				break;
			}
			self.scratch.items.push(Some(item));
			if self.eat(Kind::Comma) {
				continue;
			}
			if self.tok.kind == end {
				break;
			}
			self.fail();
			break;
		}
		base
	}

	fn parse_list(&mut self, kind: ListKind, mut element: impl FnMut(&mut Self) -> NodeId) -> usize {
		let end = Self::list_end(kind);
		let base = self.scratch.items.len();
		while self.tok.kind != end {
			let at = self.tok.start;
			let item = element(self);
			if self.error {
				break;
			}
			self.scratch.items.push(Some(item));
			if self.recover && !self.ensure_progress(at) {
				break;
			}
		}
		base
	}

	// ── annotations

	pub(super) fn parse_type_annotation(&mut self, eat_colon: bool, start: Option<u32>) -> NodeId {
		let start = start.unwrap_or(self.tok.start);
		let old = std::mem::replace(&mut self.in_type, true);
		if eat_colon {
			self.expect(Kind::Colon, false);
		}
		let type_annotation = self.parse_type();
		self.in_type = old;
		self.add(NodeKind::TypeAnnotation { type_annotation }, start)
	}

	pub(super) fn try_parse_type_annotation(&mut self) -> Option<NodeId> {
		if self.tok.kind == Kind::Colon {
			Some(self.parse_type_annotation(true, None))
		} else {
			None
		}
	}

	pub(super) fn parse_type_or_type_predicate_annotation(&mut self, return_token: Kind, allow_predicate: bool) -> NodeId {
		let old = std::mem::replace(&mut self.in_type, true);
		let annotation_start = self.tok.start;
		self.expect(return_token, false);
		let start = self.tok.start;
		let asserts =
			self.is_word(b"asserts") && self.try_parse(|p| p.parse_type_predicate_asserts()).unwrap_or(false);
		if self.error {
			return NodeId::at(0);
		}
		let annotation;
		if self.tok.kind == Kind::This && (asserts || self.peek_is_contextual("is")) {
			let parameter_name = self.parse_this_type_node();
			let type_annotation = if self.eat_word(b"is") {
				Some(self.parse_type_annotation(false, None))
			} else {
				None
			};
			let predicate = self.add(
				NodeKind::TypePredicate {
					parameter_name,
					type_annotation,
					asserts,
				},
				start,
			);
			annotation = self.add(
				NodeKind::TypeAnnotation {
					type_annotation: predicate,
				},
				annotation_start,
			);
		} else {
			if self.error {
				return NodeId::at(0);
			}
			let variable = if self.is_ident() && matches!(self.next_start(), None | Some(b'i' | b'\\')) {
				self.try_parse(|p| p.parse_type_predicate_prefix())
			} else {
				None
			};
			if self.error {
				return NodeId::at(0);
			}
			let (parameter_name, type_annotation) = match variable {
				Some(name) => (name, Some(self.parse_type_annotation(false, None))),
				None if asserts => (self.ident(), None),
				None => {
					let node = self.parse_type_annotation(false, Some(annotation_start));
					self.in_type = old;
					return node;
				}
			};
			let predicate = self.add(
				NodeKind::TypePredicate {
					parameter_name,
					type_annotation,
					asserts,
				},
				start,
			);
			annotation = self.add(
				NodeKind::TypeAnnotation {
					type_annotation: predicate,
				},
				annotation_start,
			);
		}
		self.in_type = old;
		if self.error {
			return annotation;
		}
		if !allow_predicate
			&& let NodeKind::TypeAnnotation { type_annotation } = self.kind_of(annotation)
			&& matches!(self.kind_of(type_annotation), NodeKind::TypePredicate { .. })
		{
			let at = self.start_of(type_annotation);
			return self.raise(at, Code::PredicatePlacement);
		}
		annotation
	}

	fn parse_type_predicate_asserts(&mut self) -> Option<bool> {
		if !self.is_word(b"asserts") {
			return None;
		}
		self.tnext();
		if !self.is_ident() && self.tok.kind != Kind::This {
			return None;
		}
		Some(true)
	}

	fn parse_type_predicate_prefix(&mut self) -> Option<NodeId> {
		let id = self.ident();
		if self.is_word(b"is") && !self.newline() {
			self.tnext();
			return Some(id);
		}
		None
	}

	fn parse_this_type_node(&mut self) -> NodeId {
		let start = self.tok.start;
		self.tnext();
		self.add(NodeKind::ThisType, start)
	}

	// ── types

	pub(super) fn parse_type(&mut self) -> NodeId {
		if !self.enter() {
			return NodeId::at(0);
		}
		let result = self.parse_type_inner();
		self.depth -= 1;
		result
	}

	fn next_then_parse_type(&mut self) -> NodeId {
		let old = std::mem::replace(&mut self.in_type, true);
		self.tnext();
		let ty = self.parse_type();
		self.in_type = old;
		ty
	}

	fn eat_then_parse_type(&mut self, kind: Kind) -> Option<NodeId> {
		if self.tok.kind == kind {
			Some(self.next_then_parse_type())
		} else {
			None
		}
	}

	fn parse_type_inner(&mut self) -> NodeId {
		let check_type = self.parse_non_conditional_type();
		if self.error
			|| self.scratch.ts.state.disallow_conditional_types
			|| self.newline()
			|| !self.eat(Kind::Extends)
		{
			return check_type;
		}
		self.scratch.ts.state.infer_depth += 1;
		let old = std::mem::replace(&mut self.scratch.ts.state.disallow_conditional_types, true);
		let extends_type = self.parse_non_conditional_type();
		self.scratch.ts.state.disallow_conditional_types = old;
		self.scratch.ts.state.infer_depth -= 1;
		if self.error {
			return check_type;
		}
		self.expect(Kind::Question, false);
		let old = std::mem::replace(&mut self.scratch.ts.state.disallow_conditional_types, false);
		let true_type = self.parse_type();
		self.scratch.ts.state.disallow_conditional_types = old;
		self.expect(Kind::Colon, false);
		let old = std::mem::replace(&mut self.scratch.ts.state.disallow_conditional_types, false);
		let false_type = self.parse_type();
		self.scratch.ts.state.disallow_conditional_types = old;
		let start = self.start_of(check_type);
		self.add(
			NodeKind::ConditionalType {
				check_type,
				extends_type,
				true_type,
				false_type,
			},
			start,
		)
	}

	fn parse_non_conditional_type(&mut self) -> NodeId {
		if self.is_start_of_function_type() {
			return self.parse_function_or_constructor_type(false, false);
		}
		if self.error {
			return NodeId::at(0);
		}
		if self.tok.kind == Kind::New {
			return self.parse_function_or_constructor_type(true, false);
		}
		if self.is_word(b"abstract") && self.peek_tok().kind == Kind::New {
			return self.parse_function_or_constructor_type(true, true);
		}
		if self.failed() {
			return NodeId::at(0);
		}
		self.parse_union_type_or_higher()
	}

	fn is_start_of_function_type(&mut self) -> bool {
		if self.tok.kind == Kind::Lt {
			return true;
		}
		if self.tok.kind != Kind::ParenL {
			return false;
		}
		self.lookahead(|p| p.is_unambiguously_start_of_function_type())
	}

	fn is_unambiguously_start_of_function_type(&mut self) -> bool {
		self.tnext();
		if matches!(self.tok.kind, Kind::ParenR | Kind::Ellipsis) {
			return true;
		}
		if self.skip_parameter_start() {
			if matches!(self.tok.kind, Kind::Colon | Kind::Comma | Kind::Question | Kind::Eq) {
				return true;
			}
			if self.tok.kind == Kind::ParenR {
				self.tnext();
				if self.tok.kind == Kind::Arrow {
					return true;
				}
			}
		}
		false
	}

	fn skip_parameter_start(&mut self) -> bool {
		if self.is_ident() || self.tok.kind == Kind::This {
			self.tnext();
			return true;
		}
		if self.tok.kind == Kind::BraceL {
			return self.attempt(|p| p.binding_atom()).is_some();
		}
		if self.tok.kind == Kind::BracketL {
			self.tnext();
			return self
				.attempt(|p| {
					let base = p.scratch.items.len();
					p.binding_items(Kind::BracketR, true);
					p.scratch.items.truncate(base);
				})
				.is_some();
		}
		false
	}

	fn parse_function_or_constructor_type(&mut self, constructor: bool, is_abstract: bool) -> NodeId {
		let start = self.tok.start;
		if constructor {
			if is_abstract {
				self.tnext();
			}
			self.tnext();
		}
		let old = std::mem::replace(&mut self.scratch.ts.state.disallow_conditional_types, false);
		let (type_parameters, parameters, type_annotation) = self.fill_signature(Kind::Arrow, !constructor);
		self.scratch.ts.state.disallow_conditional_types = old;
		if self.error {
			return NodeId::at(0);
		}
		let type_annotation = type_annotation.unwrap();
		let kind = if constructor {
			NodeKind::ConstructorType {
				type_parameters,
				parameters,
				type_annotation,
				is_abstract,
			}
		} else {
			NodeKind::FunctionType {
				type_parameters,
				parameters,
				type_annotation,
			}
		};
		self.add(kind, start)
	}

	fn fill_signature(&mut self, return_token: Kind, allow_predicate: bool) -> (Option<NodeId>, List, Option<NodeId>) {
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::Const);
		self.expect(Kind::ParenL, false);
		let base = self.scratch.items.len();
		self.ts_binding_items(Kind::ParenR, false, false);
		let parameters = self.list(base);
		if self.error {
			return (None, List::EMPTY, None);
		}
		self.check_parameter_list(parameters, true);
		if self.error {
			return (None, List::EMPTY, None);
		}
		let type_annotation = if return_token == Kind::Arrow || self.tok.kind == return_token {
			Some(self.parse_type_or_type_predicate_annotation(return_token, allow_predicate))
		} else {
			None
		};
		(type_parameters, parameters, type_annotation)
	}

	fn parse_union_type_or_higher(&mut self) -> NodeId {
		self.parse_union_or_intersection_type(Kind::Pipe, |p| p.parse_intersection_type_or_higher())
	}

	fn parse_intersection_type_or_higher(&mut self) -> NodeId {
		self.parse_union_or_intersection_type(Kind::Amp, |p| p.parse_type_operator_or_higher())
	}

	fn parse_union_or_intersection_type(
		&mut self,
		operator: Kind,
		mut constituent: impl FnMut(&mut Self) -> NodeId,
	) -> NodeId {
		let start = self.tok.start;
		let has_leading_operator = self.eat(operator);
		let first = constituent(self);
		if self.error || (!has_leading_operator && self.tok.kind != operator) {
			return first;
		}
		let base = self.scratch.items.len();
		self.scratch.items.push(Some(first));
		while self.eat(operator) {
			let next = constituent(self);
			if self.error {
				return first;
			}
			self.scratch.items.push(Some(next));
		}
		let types = self.list(base);
		let kind = if operator == Kind::Pipe {
			NodeKind::UnionType { types }
		} else {
			NodeKind::IntersectionType { types }
		};
		self.add(kind, start)
	}

	fn parse_type_operator_or_higher(&mut self) -> NodeId {
		if self.is_ident() && self.tok.flags & ESCAPED == 0 {
			match self.tok_text() {
				"keyof" | "readonly" | "unique" => return self.parse_type_operator(),
				"infer" => return self.parse_infer_type(),
				_ => {}
			}
		}
		let old = std::mem::replace(&mut self.scratch.ts.state.disallow_conditional_types, false);
		let ty = self.parse_array_type_or_higher();
		self.scratch.ts.state.disallow_conditional_types = old;
		ty
	}

	fn parse_type_operator(&mut self) -> NodeId {
		let start = self.tok.start;
		let word = self.tok_text();
		let operator = self.ast.strings.intern(word);
		self.tnext();
		if word == "unique" && !self.is_word(b"symbol") {
			return self.raise_arg(self.tok.start, Code::Expected, "symbol");
		}
		if !self.enter() {
			return NodeId::at(0);
		}
		let type_annotation = self.parse_type_operator_or_higher();
		self.depth -= 1;
		if self.error {
			return type_annotation;
		}
		if word == "readonly"
			&& !matches!(
				self.kind_of(type_annotation),
				NodeKind::TupleType { .. } | NodeKind::ArrayType { .. }
			) {
			return self.raise(start, Code::ReadonlyTypeOperand);
		}
		self.add(
			NodeKind::TypeOperator {
				operator,
				type_annotation,
			},
			start,
		)
	}

	fn parse_infer_type(&mut self) -> NodeId {
		let start = self.tok.start;
		if self.scratch.ts.state.infer_depth == 0 {
			return self.raise(start, Code::InferPlacement);
		}
		self.expect_contextual(b"infer");
		let parameter_start = self.tok.start;
		let name = self.parse_type_parameter_name();
		if self.error {
			return NodeId::at(0);
		}
		let constraint = self.try_parse(|p| p.parse_constraint_for_infer_type());
		let type_parameter = self.add(
			NodeKind::TypeParameter {
				name,
				constraint,
				default: None,
				is_in: false,
				is_out: false,
				is_const: false,
			},
			parameter_start,
		);
		self.add(NodeKind::InferType { type_parameter }, start)
	}

	fn parse_constraint_for_infer_type(&mut self) -> Option<NodeId> {
		if self.eat(Kind::Extends) {
			let old = std::mem::replace(&mut self.scratch.ts.state.disallow_conditional_types, true);
			let constraint = self.parse_type();
			self.scratch.ts.state.disallow_conditional_types = old;
			if self.scratch.ts.state.disallow_conditional_types || self.tok.kind != Kind::Question {
				return Some(constraint);
			}
		}
		None
	}

	fn parse_array_type_or_higher(&mut self) -> NodeId {
		let mut ty = self.parse_non_array_type();
		while !self.error && !self.newline() && self.eat(Kind::BracketL) {
			let start = self.start_of(ty);
			if self.tok.kind == Kind::BracketR {
				self.tnext();
				ty = self.add(NodeKind::ArrayType { element_type: ty }, start);
			} else {
				let index_type = self.parse_type();
				self.expect(Kind::BracketR, false);
				ty = self.add(
					NodeKind::IndexedAccessType {
						object_type: ty,
						index_type,
					},
					start,
				);
			}
		}
		ty
	}

	fn parse_non_array_type(&mut self) -> NodeId {
		let start = self.tok.start;
		match self.tok.kind {
			Kind::String | Kind::Number | Kind::BigInt | Kind::True | Kind::False => {
				let literal = self.atom(&mut None, false);
				self.add(NodeKind::LiteralType { literal }, start)
			}
			Kind::Minus => {
				if !matches!(self.peek_tok().kind, Kind::Number | Kind::BigInt) {
					return self.fail();
				}
				let literal = self.ts_unary();
				self.add(NodeKind::LiteralType { literal }, start)
			}
			Kind::This => self.parse_this_type_node(),
			Kind::Typeof => self.parse_type_query(),
			Kind::Import => self.parse_import_type(),
			Kind::BraceL => {
				if self.peek_tokens(|p| p.is_start_of_mapped_type()) {
					self.parse_mapped_type()
				} else if self.failed() {
					NodeId::at(0)
				} else {
					self.parse_type_literal()
				}
			}
			Kind::BracketL => self.parse_tuple_type(),
			Kind::ParenL => self.parse_parenthesized_type(),
			Kind::Template | Kind::TemplateHead => {
				let literal = self.ts_template();
				self.add(NodeKind::LiteralType { literal }, start)
			}
			Kind::Ident => {
				let escaped = self.tok.flags & ESCAPED != 0;
				let t = self.tok;
				let keyword = TsKeyword::from_name(self.word_of(t));
				if let Some(keyword) = keyword
					&& self.lexer.peek_byte() != Some(b'.')
				{
					if escaped {
						let name = self.word(t);
						return self.raise_name(start, Code::EscapeInKeyword, name);
					}
					self.tnext();
					return self.add(NodeKind::Keyword(keyword), start);
				}
				self.parse_type_reference(false)
			}
			Kind::Void | Kind::Null => {
				let keyword = if self.tok.kind == Kind::Void {
					TsKeyword::Void
				} else {
					TsKeyword::Null
				};
				if self.lexer.peek_byte() != Some(b'.') {
					self.tnext();
					return self.add(NodeKind::Keyword(keyword), start);
				}
				self.parse_type_reference(false)
			}
			_ => self.placeholder(),
		}
	}

	pub(super) fn parse_type_reference(&mut self, allow_reserved_words: bool) -> NodeId {
		let start = self.tok.start;
		let type_name = self.parse_entity_name(allow_reserved_words);
		if self.error {
			return type_name;
		}
		let type_arguments = if !self.newline() && self.tok.kind == Kind::Lt {
			Some(self.parse_type_arguments())
		} else {
			None
		};
		self.add(
			NodeKind::TypeReference {
				type_name,
				type_arguments,
			},
			start,
		)
	}

	fn ident_or_name(&mut self, liberal: bool) -> NodeId {
		if liberal { self.name() } else { self.ident() }
	}

	pub(super) fn parse_entity_name(&mut self, allow_reserved_words: bool) -> NodeId {
		let mut entity = self.ident_or_name(allow_reserved_words);
		while !self.error && self.eat(Kind::Dot) {
			let start = self.start_of(entity);
			let right = self.name();
			entity = self.add(NodeKind::QualifiedName { left: entity, right }, start);
		}
		entity
	}

	fn parse_type_query(&mut self) -> NodeId {
		let start = self.tok.start;
		self.expect(Kind::Typeof, false);
		let expr_name = if self.tok.kind == Kind::Import {
			self.parse_import_type()
		} else if self.tok.kind == Kind::This {
			self.parse_entity_name(true)
		} else {
			self.parse_entity_name(false)
		};
		if self.error {
			return expr_name;
		}
		let type_arguments = if !self.newline() && self.tok.kind == Kind::Lt {
			Some(self.parse_type_arguments())
		} else {
			None
		};
		self.add(
			NodeKind::TypeQuery {
				expr_name,
				type_arguments,
			},
			start,
		)
	}

	fn parse_import_type(&mut self) -> NodeId {
		let start = self.tok.start;
		self.expect(Kind::Import, false);
		self.expect(Kind::ParenL, false);
		if self.error {
			return NodeId::at(0);
		}
		if self.tok.kind != Kind::String {
			return self.raise(self.tok.start, Code::TypeImportArgument);
		}
		let argument = self.atom(&mut None, false);
		self.expect(Kind::ParenR, false);
		let qualifier = if self.eat(Kind::Dot) {
			Some(self.parse_entity_name(true))
		} else {
			None
		};
		if self.error {
			return argument;
		}
		let type_arguments = if self.tok.kind == Kind::Lt {
			Some(self.parse_type_arguments())
		} else {
			None
		};
		self.add(
			NodeKind::ImportType {
				argument,
				qualifier,
				type_arguments,
			},
			start,
		)
	}

	fn is_start_of_mapped_type(&mut self) -> bool {
		self.tnext();
		if self.eat(Kind::Plus) || self.eat(Kind::Minus) {
			return self.is_word(b"readonly");
		}
		if self.is_word(b"readonly") {
			self.tnext();
		}
		if self.tok.kind != Kind::BracketL {
			return false;
		}
		self.tnext();
		if !self.is_ident() {
			return false;
		}
		self.tnext();
		self.tok.kind == Kind::In
	}

	fn parse_mapped_type(&mut self) -> NodeId {
		let start = self.tok.start;
		self.expect(Kind::BraceL, false);
		let readonly = if let Some(sign) = self.eat_sign() {
			self.expect_contextual(b"readonly");
			Some(sign)
		} else if self.eat_word(b"readonly") {
			Some(Modifier::True)
		} else {
			None
		};
		self.expect(Kind::BracketL, false);
		let parameter_start = self.tok.start;
		let name = self.parse_type_parameter_name();
		if self.error {
			return NodeId::at(0);
		}
		let old = std::mem::replace(&mut self.in_type, true);
		self.expect(Kind::In, false);
		let constraint = self.parse_type();
		self.in_type = old;
		let type_parameter = self.add(
			NodeKind::TypeParameter {
				name,
				constraint: Some(constraint),
				default: None,
				is_in: false,
				is_out: false,
				is_const: false,
			},
			parameter_start,
		);
		let name_type = if self.eat_word(b"as") {
			Some(self.parse_type())
		} else {
			None
		};
		self.expect(Kind::BracketR, false);
		let optional = if let Some(sign) = self.eat_sign() {
			self.expect(Kind::Question, false);
			Some(sign)
		} else if self.eat(Kind::Question) {
			Some(Modifier::True)
		} else {
			None
		};
		let type_annotation = self.eat_then_parse_type(Kind::Colon);
		self.tsemicolon();
		self.expect(Kind::BraceR, false);
		self.add(
			NodeKind::MappedType {
				readonly,
				type_parameter,
				name_type,
				optional,
				type_annotation,
			},
			start,
		)
	}

	fn eat_sign(&mut self) -> Option<Modifier> {
		let sign = match self.tok.kind {
			Kind::Plus => Modifier::Plus,
			Kind::Minus => Modifier::Minus,
			_ => return None,
		};
		self.tnext();
		Some(sign)
	}

	fn parse_type_literal(&mut self) -> NodeId {
		let start = self.tok.start;
		let base = self.parse_object_type_members();
		let members = self.list(base);
		self.add(NodeKind::TypeLiteral { members }, start)
	}

	fn parse_tuple_type(&mut self) -> NodeId {
		let start = self.tok.start;
		self.expect(Kind::BracketL, false);
		let base = self.parse_delimited_list(ListKind::TupleElements, |p| p.parse_tuple_element_type());
		self.expect(Kind::BracketR, false);
		if self.error {
			self.scratch.items.truncate(base);
			return NodeId::at(0);
		}
		let mut seen_optional = false;
		for i in base..self.scratch.items.len() {
			let element = self.scratch.items[i].unwrap();
			let kind = self.kind_of(element);
			let optional = matches!(
				kind,
				NodeKind::OptionalType { .. } | NodeKind::NamedTupleMember { optional: true, .. }
			);
			if seen_optional && !optional && !matches!(kind, NodeKind::RestType { .. }) {
				let at = self.start_of(element);
				self.scratch.items.truncate(base);
				return self.raise(at, Code::RequiredAfterOptional);
			}
			seen_optional |= optional;
		}
		let element_types = self.list(base);
		self.add(NodeKind::TupleType { element_types }, start)
	}

	fn parse_tuple_element_type(&mut self) -> NodeId {
		let start = self.tok.start;
		let rest = self.eat(Kind::Ellipsis);
		let mut ty = self.parse_type();
		if self.error {
			return ty;
		}
		let optional = self.eat(Kind::Question);
		if self.eat(Kind::Colon) {
			let label = match self.kind_of(ty) {
				NodeKind::TypeReference {
					type_name,
					type_arguments: None,
				} if matches!(self.kind_of(type_name), NodeKind::Identifier { .. }) => type_name,
				_ => {
					let at = self.start_of(ty);
					return self.raise(at, Code::TupleLabel);
				}
			};
			let element_type = self.parse_type();
			let label_start = self.start_of(ty);
			ty = self.add(
				NodeKind::NamedTupleMember {
					label,
					optional,
					element_type,
				},
				label_start,
			);
		} else if optional {
			let type_start = self.start_of(ty);
			ty = self.add(NodeKind::OptionalType { type_annotation: ty }, type_start);
		}
		if rest {
			ty = self.add(NodeKind::RestType { type_annotation: ty }, start);
		}
		ty
	}

	fn parse_parenthesized_type(&mut self) -> NodeId {
		let start = self.tok.start;
		self.expect(Kind::ParenL, false);
		let type_annotation = self.parse_type();
		self.expect(Kind::ParenR, false);
		self.add(NodeKind::ParenthesizedType { type_annotation }, start)
	}

	// ── type parameters and arguments

	fn parse_type_parameter_name(&mut self) -> StrId {
		let id = self.ident();
		if self.error {
			return self.ast.strings.intern("");
		}
		let NodeKind::Identifier { name } = self.kind_of(id) else {
			unreachable!()
		};
		name
	}

	fn parse_type_parameter(&mut self, modifiers: TypeParameterModifiers) -> NodeId {
		let start = self.tok.start;
		let (allowed, disallowed): (&[&str], &[&str]) = match modifiers {
			TypeParameterModifiers::InOut => (IN_OUT, ACCESSIBILITY_AND_CLASS),
			TypeParameterModifiers::Const => (&["const"], IN_OUT),
			TypeParameterModifiers::Class => (IN_OUT_CONST, ACCESSIBILITY_AND_CLASS),
		};
		let parsed = self.parse_modifiers(allowed, disallowed, false, Some(Code::TypeParameterModifier));
		if self.error {
			return NodeId::at(0);
		}
		let name = self.parse_type_parameter_name();
		if self.error {
			return NodeId::at(0);
		}
		let constraint = self.eat_then_parse_type(Kind::Extends);
		let default = self.eat_then_parse_type(Kind::Eq);
		self.add(
			NodeKind::TypeParameter {
				name,
				constraint,
				default,
				is_in: parsed.is_in,
				is_out: parsed.is_out,
				is_const: parsed.is_const,
			},
			start,
		)
	}

	pub(super) fn parse_type_parameters(&mut self, modifiers: TypeParameterModifiers) -> NodeId {
		let start = self.tok.start;
		if self.tok.kind != Kind::Lt {
			return self.fail();
		}
		// angle brackets count as brackets do: no stop of the host's stands inside them
		self.nest = self.nest.wrapping_add(1);
		self.tnext();
		let base = self.parse_delimited_list(ListKind::TypeParametersOrArguments, |p| p.parse_type_parameter(modifiers));
		self.nest = self.nest.wrapping_sub(1);
		self.expect(Kind::Gt, false);
		if self.error {
			self.scratch.items.truncate(base);
			return NodeId::at(0);
		}
		if self.scratch.items.len() == base {
			return self.raise(self.tok.start, Code::EmptyTypeParameters);
		}
		let params = self.list(base);
		self.add(NodeKind::TypeParameterDeclaration { params }, start)
	}

	pub(super) fn try_parse_type_parameters(&mut self, modifiers: TypeParameterModifiers) -> Option<NodeId> {
		if self.tok.kind == Kind::Lt {
			Some(self.parse_type_parameters(modifiers))
		} else {
			None
		}
	}

	pub(super) fn parse_type_arguments(&mut self) -> NodeId {
		let start = self.tok.start;
		let old = std::mem::replace(&mut self.in_type, true);
		self.nest = self.nest.wrapping_add(1);
		self.expect(Kind::Lt, false);
		let base = self.parse_delimited_list(ListKind::TypeParametersOrArguments, |p| p.parse_type());
		self.in_type = old;
		if self.error {
			self.scratch.items.truncate(base);
			return NodeId::at(0);
		}
		if self.scratch.items.len() == base {
			return self.raise(self.tok.start, Code::EmptyTypeArguments);
		}
		self.nest = self.nest.wrapping_sub(1);
		self.expect(Kind::Gt, false);
		let params = self.list(base);
		self.add(NodeKind::TypeParameterInstantiation { params }, start)
	}

	fn parse_type_arguments_in_expression(&mut self) -> Option<NodeId> {
		if self.tok.kind == Kind::LtLt {
			self.split_lt();
		}
		if self.tok.kind != Kind::Lt {
			return None;
		}
		Some(self.parse_type_arguments())
	}

	fn parse_heritage_clause(&mut self, token: &str) -> usize {
		let start = self.tok.start;
		let clause = if token == "implements" {
			Clause::Implements
		} else {
			Clause::Extends
		};
		let base = self.parse_delimited_list(ListKind::HeritageClause, |p| {
			let start = p.tok.start;
			let expression = p.parse_entity_name(false);
			if p.error {
				return expression;
			}
			let type_arguments = if p.tok.kind == Kind::Lt {
				Some(p.parse_type_arguments())
			} else {
				None
			};
			p.add(
				NodeKind::Heritage {
					expression,
					type_arguments,
					clause,
				},
				start,
			)
		});
		if !self.error && self.scratch.items.len() == base {
			self.raise_arg(start, Code::EmptyList, token);
		}
		base
	}

	// ── type members

	fn parse_object_type_members(&mut self) -> usize {
		self.expect(Kind::BraceL, false);
		let base = self.parse_list(ListKind::TypeMembers, |p| p.parse_type_member());
		self.expect(Kind::BraceR, false);
		base
	}

	fn parse_type_member_semicolon(&mut self) {
		if !self.eat(Kind::Comma) && !self.is_line_terminator() {
			self.expect(Kind::Semi, false);
		}
	}

	fn parse_type_member(&mut self) -> NodeId {
		let start = self.tok.start;
		if matches!(self.tok.kind, Kind::ParenL | Kind::Lt) {
			return self.parse_signature_member(start, false);
		}
		if self.tok.kind == Kind::Eof {
			return self.fail();
		}
		if self.tok.kind == Kind::New {
			self.tnext();
			if matches!(self.tok.kind, Kind::ParenL | Kind::Lt) {
				return self.parse_signature_member(start, true);
			}
			let name = self.ast.strings.intern("new");
			let key = self.add(NodeKind::Identifier { name }, start);
			return self.parse_property_or_method_signature(start, key, false, None, false);
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
			Some(Code::TypeMemberModifier),
		);
		if self.error {
			return NodeId::at(0);
		}
		if let Some(signature) = self.try_parse_index_signature(start) {
			if !self.error {
				self.extras_mut(signature).readonly = modifiers.extras.readonly;
			}
			return signature;
		}
		if self.failed() {
			return NodeId::at(0);
		}
		let (mut key, mut computed) = self.property_name();
		if self.error {
			return key;
		}
		let mut kind = None;
		if !computed && (self.ident_is(key, "get") || self.ident_is(key, "set")) && self.token_can_follow_modifier() {
			kind = Some(if self.ident_is(key, "get") {
				SignatureKind::Get
			} else {
				SignatureKind::Set
			});
			(key, computed) = self.property_name();
			if self.error {
				return key;
			}
		}
		self.parse_property_or_method_signature(start, key, computed, kind, modifiers.extras.readonly)
	}

	fn parse_signature_member(&mut self, start: u32, construct: bool) -> NodeId {
		let (type_parameters, parameters, type_annotation) = self.fill_signature(Kind::Colon, !construct);
		if self.error {
			return NodeId::at(0);
		}
		self.parse_type_member_semicolon();
		let kind = if construct {
			NodeKind::ConstructSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			}
		} else {
			NodeKind::CallSignatureDeclaration {
				type_parameters,
				parameters,
				type_annotation,
			}
		};
		self.add(kind, start)
	}

	fn parse_property_or_method_signature(
		&mut self,
		start: u32,
		key: NodeId,
		computed: bool,
		kind: Option<SignatureKind>,
		readonly: bool,
	) -> NodeId {
		if kind.is_some() && !matches!(self.tok.kind, Kind::ParenL | Kind::Lt) {
			return self.fail();
		}
		let optional = self.eat(Kind::Question);
		if matches!(self.tok.kind, Kind::ParenL | Kind::Lt) {
			if readonly {
				return self.raise(start, Code::ReadonlyPlacement);
			}
			if kind.is_some() && self.tok.kind == Kind::Lt {
				return self.raise(self.tok.start, Code::AccessorTypeParameters);
			}
			let (type_parameters, parameters, type_annotation) =
				self.fill_signature(Kind::Colon, kind != Some(SignatureKind::Get));
			if self.error {
				return NodeId::at(0);
			}
			self.parse_type_member_semicolon();
			if self.error {
				return NodeId::at(0);
			}
			let count = self.ast.list(parameters).len();
			match kind {
				Some(SignatureKind::Get) if count > 0 => {
					return self.raise(self.tok.start, Code::GetterParams);
				}
				Some(SignatureKind::Set) => {
					if count != 1 {
						return self.raise(self.tok.start, Code::SetterParams);
					}
					if let Some(type_annotation) = type_annotation {
						let at = self.start_of(type_annotation);
						return self.raise(at, Code::SetterReturnType);
					}
				}
				_ => {}
			}
			let node = self.add(
				NodeKind::MethodSignature {
					key,
					computed,
					optional,
					kind: kind.unwrap_or(SignatureKind::Method),
					parameters,
					type_annotation,
				},
				start,
			);
			if type_parameters.is_some() {
				self.extras_mut(node).type_parameters = type_parameters;
			}
			return node;
		}
		let type_annotation = self.try_parse_type_annotation();
		self.parse_type_member_semicolon();
		self.add(
			NodeKind::PropertySignature {
				key,
				computed,
				optional,
				readonly,
				type_annotation,
			},
			start,
		)
	}

	fn try_parse_index_signature(&mut self, start: u32) -> Option<NodeId> {
		if self.tok.kind != Kind::BracketL || !self.peek_tokens(|p| p.is_unambiguously_index_signature()) {
			return None;
		}
		self.expect(Kind::BracketL, false);
		let id = self.ident();
		if self.error {
			return Some(id);
		}
		let annotation = self.parse_type_annotation(true, None);
		if self.error {
			return Some(id);
		}
		self.extras_mut(id).type_annotation = Some(annotation);
		self.ast.node_mut(id).end = self.prev_end;
		self.expect(Kind::BracketR, false);
		let parameters = self.ast.add_list(&[Some(id)]);
		let type_annotation = self.try_parse_type_annotation();
		if self.error {
			return Some(id);
		}
		if type_annotation.is_none() {
			return Some(self.raise(self.tok.start, Code::IndexSignatureType));
		}
		self.parse_type_member_semicolon();
		Some(self.add(
			NodeKind::IndexSignature {
				parameters,
				type_annotation,
			},
			start,
		))
	}

	fn is_unambiguously_index_signature(&mut self) -> bool {
		self.tnext();
		if self.is_ident() {
			self.tnext();
			return self.tok.kind == Kind::Colon;
		}
		false
	}

	// ── modifiers

	fn token_can_follow_modifier(&self) -> bool {
		matches!(
			self.tok.kind,
			Kind::BracketL | Kind::BraceL | Kind::Star | Kind::Ellipsis | Kind::PrivateName
		) || self.is_literal_property_name()
	}

	fn parse_modifier(
		&mut self,
		allowed: &[&str],
		disallowed: &[&str],
		stop_on_static_block: bool,
		seen_static: bool,
	) -> Option<(&'static str, u32)> {
		if self.tok.flags & ESCAPED != 0 {
			return None;
		}
		let word = match self.tok.kind {
			Kind::Ident => self.tok_text(),
			Kind::In => "in",
			Kind::Const => "const",
			_ => return None,
		};
		let &modifier = MODIFIERS.iter().find(|m| **m == word)?;
		if !allowed.contains(&modifier) && !disallowed.contains(&modifier) {
			return None;
		}
		if modifier == "static" && (seen_static || (stop_on_static_block && self.lexer.peek_byte() == Some(b'{'))) {
			return None;
		}
		let start = self.tok.start;
		let snapshot = self.tok_snap();
		self.tnext();
		if self.failed() {
			return None;
		}
		if (modifier == "static" || !self.newline()) && self.token_can_follow_modifier() {
			return Some((modifier, start));
		}
		self.restore_tokens(snapshot);
		None
	}

	pub(super) fn parse_modifiers(
		&mut self,
		allowed: &[&str],
		disallowed: &[&str],
		stop_on_static_block: bool,
		disallowed_error: Option<Code>,
	) -> Modifiers {
		let mut modifiers = Modifiers::default();
		while let Some((modifier, start)) =
			self.parse_modifier(allowed, disallowed, stop_on_static_block, modifiers.extras.is_static)
		{
			if let Some((code, args)) = misplaced(&modifiers, modifier) {
				let mut error = SyntaxError::new(start, code);
				for arg in args.into_iter().flatten() {
					error = error.arg(self.ast.strings.intern(arg));
				}
				self.raise_with(error);
				return modifiers;
			}
			modifiers.set(modifier);
			if let Some(code) = disallowed_error.filter(|_| disallowed.contains(&modifier)) {
				self.raise_arg(self.tok.start, code, modifier);
				return modifiers;
			}
		}
		modifiers
	}
}

fn misplaced(seen: &Modifiers, modifier: &'static str) -> Option<(Code, [Option<&'static str>; 2])> {
	let order = |before: &'static str, after: &'static str| {
		(modifier == before && seen.has(after)).then_some((Code::ModifierOrder, [Some(before), Some(after)]))
	};
	let conflict = |a: &'static str, b: &'static str| {
		((seen.has(a) && modifier == b) || (seen.has(b) && modifier == a))
			.then_some((Code::ConflictingModifiers, [Some(a), Some(b)]))
	};
	let accessibility = matches!(modifier, "public" | "private" | "protected");
	if accessibility && seen.extras.accessibility.is_some() {
		return Some((Code::DuplicateAccessibility, [None, None]));
	}
	if !accessibility && seen.has(modifier) {
		return Some((Code::DuplicateModifier, [Some(modifier), None]));
	}
	match modifier {
		"public" | "private" | "protected" => ["override", "static", "readonly", "accessor"]
			.into_iter()
			.find_map(|after| order(modifier, after))
			.or_else(|| conflict("private", "abstract")),
		"in" | "out" => order("in", "out"),
		"accessor" => ["readonly", "declare"]
			.into_iter()
			.find_map(|other| conflict("accessor", other)),
		"const" => None,
		_ => [
			("static", "readonly"),
			("static", "override"),
			("static", "accessor"),
			("override", "accessor"),
			("override", "readonly"),
			("abstract", "override"),
		]
		.into_iter()
		.find_map(|(before, after)| order(before, after))
		.or_else(|| {
			[
				("declare", "override"),
				("static", "abstract"),
				("private", "abstract"),
				("accessor", "readonly"),
				("accessor", "declare"),
			]
			.into_iter()
			.find_map(|(a, b)| conflict(a, b))
		}),
	}
}

include!("ts_more.rs");
