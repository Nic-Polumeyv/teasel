//! LANE PROTOTYPE, milestone 1: JavaScript expressions read in the new shape into teasel's own
//! tree. Tokens carry no text, parse functions return the node, an error is recorded once and
//! the input ends, binary operators are one precedence loop, and a list in parentheses that may
//! be an arrow function's parameters is read in the role its tokens tell.
//!
//! Not here: functions and arrows with block bodies, classes, statements, TypeScript, recovery,
//! scopes, the `parenthesized` option, stop words. Meeting one sets `unsupported`.

use super::values::{self, Values};
use super::{Arg, BROKEN, ESCAPED, Kind, Lexer, NEWLINE_BEFORE, Token};
use super::checks::{Binding, Checks, Facts, SCOPE_ARROW, function_flags};
use crate::error::{Code, SyntaxError};
use crate::ast::{
	AssignmentOperator, Ast, BinaryOperator, List, LogicalOperator, NodeId, NodeKind, PropertyKind, UnaryOperator,
	UpdateOperator,
};
use crate::interner::{FastMap, FastSet, StrId};

/// The lists being read, innermost last: one buffer for every list of a parse.
#[derive(Default)]
pub struct Scratch {
	pub(super) items: Vec<Option<NodeId>>,
	pub(super) quasis: Vec<Option<NodeId>>,
	pub(super) values: Values,
	pub(super) checks: Checks,
	pub(super) regexp: crate::parser::regexp::Scratch,
	pub ts: super::ts::Ts,
	/// The errors recorded under recovery, in the order they were met.
	pub(super) errors: Vec<SyntaxError>,
	/// The host's stop tokens: where each stands in the text they were given in, and whether it is a word.
	pub(super) stop_ranges: Vec<(u32, u32, bool)>,
	/// The role of each list in parentheses that a scan of its tokens told, by where its `(` is.
	pub(super) roles: FastMap<u32, Role>,
	/// The lists, by their `(` or `<`, whose head failed as an arrow's, on a conditional's spine
	/// or not.
	pub(super) not_arrow: FastSet<(u32, bool)>,
	/// The brackets open during a scan: closer, opener, what a `/` is after the closer.
	frames: Vec<(Kind, u32, Slash)>,
	/// What the parser held when the error it is unwinding from was met: recovery reads on from there.
	pub(super) met: super::entry::Met,
	/// The first bytes the host's stops start with, one bit each: a token starting with none is no stop.
	pub(super) stop_firsts: [u64; 4],
	/// One of the stops is a word TypeScript reads too: the entry may have to be read again.
	pub(super) stop_assertion: bool,
	/// The text the stops above were read from.
	pub(super) stops_of: String,
}

pub(crate) const MAX_DEPTH: u32 = 1000;
/// How many operators or subscripts one loop may chain: each nests the node before it.
// wasm frames sit on the embedder's stack: the scope walk overflowed it past 5,000 links
const MAX_CHAIN: u32 = if cfg!(target_arch = "wasm32") { 4_000 } else { 10_000 };

/// The facts of the literal being read, where it may yet be a pattern.
pub(super) type Errors = Option<Facts>;

/// How a list that may be an arrow's parameters is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Role {
	Parameters,
	Expression,
	/// A `:` at the offset follows the closer: only reading the type after it tells.
	Typed(u32),
	/// The tokens do not tell: the head is tried as an arrow's.
	Unsure,
}

/// What a `/` is after a token.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slash {
	Divide,
	Regex,
	/// Only the parser can tell.
	Unsure,
}

/// The next token of a scan, none where the text does not lex.
fn look(lexer: &mut Lexer) -> Option<Token> {
	let t = lexer.next(false);
	if lexer.error.is_some() {
		return None;
	}
	Some(t)
}

/// What the token after a list's closer makes the list.
fn after_closer<const TS: bool>(follower: Token) -> Role {
	match follower.kind {
		Kind::Arrow if follower.flags & NEWLINE_BEFORE == 0 => Role::Parameters,
		Kind::Colon if TS => Role::Typed(follower.start),
		_ => Role::Expression,
	}
}

/// Whether `next`, the first byte after a list's first name, starts a token that makes the list
/// an expression: not `,`, `)`, `=`, a byte that may start no token, nor with TypeScript `:`, `?`
/// or a word.
#[inline(always)]
fn operator_follows<const TS: bool>(next: Option<u8>) -> bool {
	match next {
		Some(
			b'.' | b'[' | b'(' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'!' | b'~' | b'<' | b'>',
		) => true,
		Some(b';' | b'}' | b']' | b'{' | b'"' | b'\'' | b'`') => true,
		Some(b':' | b'?') => !TS,
		Some(b) => !TS && (b.is_ascii_alphanumeric() || b == b'_' || b == b'$'),
		None => false,
	}
}

/// What a `/` is after a `++`, `--` or TypeScript `!` read where a `/` would be `slash`.
fn postfix(slash: Slash, token: Token) -> Slash {
	match slash {
		Slash::Divide if token.flags & NEWLINE_BEFORE == 0 => Slash::Divide,
		Slash::Unsure => Slash::Unsure,
		_ => Slash::Regex,
	}
}

/// What the function being read is, and whether `in` is an operator where the cursor stands.
#[derive(Clone, Copy, Default)]
pub(super) struct Ctx {
	pub(super) function: bool,
	pub(super) is_async: bool,
	pub(super) generator: bool,
	/// `await` is an operator here.
	pub(super) can_await: bool,
	/// The head of a `for`: `in` ends the expression.
	pub(super) no_in: bool,
	/// The head of a `for await`: `async of` is two names.
	pub(super) for_await: bool,
}

/// `TS`: TypeScript is read. A constant of the type, as in the parser in use, so the JavaScript
/// copy compiles none of TypeScript's tests.
pub struct Parser<'a, const COUNT: bool, const TS: bool = false> {
	pub(super) src: &'a str,
	pub(super) lexer: Lexer<'a>,
	pub(super) tok: Token,
	pub(super) prev_end: u32,
	pub(super) ast: &'a mut Ast,
	pub(super) scratch: &'a mut Scratch,
	/// Where the assignment expression being read starts: an arrow function can start only there.
	pub(super) arrow_at: u32,
	/// That assignment expression is the subject of a `for await`: `async of` is two names.
	pub(super) arrow_for_await: bool,
	/// Where the first `yield`, `await` and name `await` stand in the parameters being read.
	pub(super) yield_pos: u32,
	pub(super) await_pos: u32,
	pub(super) await_ident_pos: u32,
	/// The node just read is an arrow function not in parentheses: nothing continues it.
	pub(super) bare_arrow: bool,
	/// A list's one bracket item is being read once: a list in it that may be parameters gives
	/// that read up.
	pub(super) covering: bool,
	/// The tables of lists' roles and failed heads hold this read's only: what a read before
	/// left in them is forgotten when a list first asks, not when a read starts.
	pub(super) lists_forgotten: bool,
	pub(super) ctx: Ctx,
	pub(super) module: bool,
	pub(super) strict: bool,
	pub(super) depth: u32,
	pub error: bool,
	/// The first error of the grammar, and where the cursor stood when it was met.
	pub(super) err: Option<SyntaxError>,
	pub(super) err_at: u32,
	/// That error was met before the token read ahead was: it comes before the lexer's.
	pub(super) err_first: bool,
	pub unsupported: Option<&'static str>,
	pub tokens: u64,
	/// Of those, the ones not read before: a rewind reads a token again.
	pub fresh: u64,
	high: u32,
	/// The cursor stands in a type: `<` and `>` are tokens of their own.
	pub(super) in_type: bool,
	/// The parameters about to be read are a class method's: they may be parameter properties.
	pub(super) modifiers_ok: bool,
	/// With TypeScript: exports of types are marked, for what erases them.
	pub(super) erase: bool,
	pub(super) options: crate::parser::Options,
	/// Errors are recorded and reading goes on.
	pub(super) recover: bool,
	/// Inside an attempt, where a read fails as a strict one would.
	pub(super) speculating: u32,
	/// Brackets are counted and the host's stops tested at each token: a host's stops, or recovery.
	pub(super) track: bool,
	/// The host's stop tokens, separated by spaces.
	pub(super) stops: &'a str,
	/// How many parentheses, brackets and braces are open, and of each.
	pub(super) nest: u32,
	pub(super) open: [u32; 3],
	/// The token at the cursor closes a bracket nothing here opened.
	pub(super) unmatched: bool,
	/// The token at the cursor is one of the host's, outside every bracket.
	pub(super) stop_hit: bool,
	/// Where TypeScript last read a host stop word as its own; and where the entry, read again, stops.
	pub(super) stop_word_at: Option<u32>,
	pub(super) forced_stop: Option<u32>,
	/// Where the read started: a program starts there.
	pub(super) base: u32,
	/// More nodes than this is a read that stopped consuming input.
	pub(super) tree_limit: usize,
	/// Where the keyword read as a name stands: one spelled with an escape is no error there.
	pub(super) liberal_at: u32,
}

static PREC: [u8; 256] = precedences();

const fn precedences() -> [u8; 256] {
	let mut t = [0u8; 256];
	t[Kind::QuestionQuestion as usize] = 1;
	t[Kind::PipePipe as usize] = 1;
	t[Kind::AmpAmp as usize] = 2;
	t[Kind::Pipe as usize] = 3;
	t[Kind::Caret as usize] = 4;
	t[Kind::Amp as usize] = 5;
	t[Kind::EqEq as usize] = 6;
	t[Kind::BangEq as usize] = 6;
	t[Kind::EqEqEq as usize] = 6;
	t[Kind::BangEqEq as usize] = 6;
	t[Kind::Lt as usize] = 7;
	t[Kind::LtEq as usize] = 7;
	t[Kind::Gt as usize] = 7;
	t[Kind::GtEq as usize] = 7;
	t[Kind::In as usize] = 7;
	t[Kind::Instanceof as usize] = 7;
	t[Kind::LtLt as usize] = 8;
	t[Kind::GtGt as usize] = 8;
	t[Kind::GtGtGt as usize] = 8;
	t[Kind::Plus as usize] = 9;
	t[Kind::Minus as usize] = 9;
	t[Kind::Star as usize] = 10;
	t[Kind::Slash as usize] = 10;
	t[Kind::Percent as usize] = 10;
	t
}

fn binary_node(kind: Kind, left: NodeId, right: NodeId) -> NodeKind {
	use BinaryOperator as B;
	let operator = match kind {
		Kind::QuestionQuestion => {
			return NodeKind::LogicalExpression {
				operator: LogicalOperator::Nullish,
				left,
				right,
			};
		}
		Kind::PipePipe => {
			return NodeKind::LogicalExpression {
				operator: LogicalOperator::Or,
				left,
				right,
			};
		}
		Kind::AmpAmp => {
			return NodeKind::LogicalExpression {
				operator: LogicalOperator::And,
				left,
				right,
			};
		}
		Kind::Pipe => B::BitOr,
		Kind::Caret => B::BitXor,
		Kind::Amp => B::BitAnd,
		Kind::EqEq => B::Eq,
		Kind::BangEq => B::NotEq,
		Kind::EqEqEq => B::StrictEq,
		Kind::BangEqEq => B::StrictNotEq,
		Kind::Lt => B::Lt,
		Kind::LtEq => B::LtEq,
		Kind::Gt => B::Gt,
		Kind::GtEq => B::GtEq,
		Kind::In => B::In,
		Kind::Instanceof => B::Instanceof,
		Kind::LtLt => B::Shl,
		Kind::GtGt => B::Shr,
		Kind::GtGtGt => B::UShr,
		Kind::Plus => B::Add,
		Kind::Minus => B::Sub,
		Kind::Star => B::Mul,
		Kind::Slash => B::Div,
		_ => B::Mod,
	};
	NodeKind::BinaryExpression { operator, left, right }
}

fn assignment(kind: Kind) -> Option<AssignmentOperator> {
	use AssignmentOperator as A;
	Some(match kind {
		Kind::Eq => A::Assign,
		Kind::PlusEq => A::Add,
		Kind::MinusEq => A::Sub,
		Kind::StarEq => A::Mul,
		Kind::SlashEq => A::Div,
		Kind::PercentEq => A::Mod,
		Kind::StarStarEq => A::Exp,
		Kind::LtLtEq => A::Shl,
		Kind::GtGtEq => A::Shr,
		Kind::GtGtGtEq => A::UShr,
		Kind::PipeEq => A::BitOr,
		Kind::CaretEq => A::BitXor,
		Kind::AmpEq => A::BitAnd,
		Kind::PipePipeEq => A::Or,
		Kind::AmpAmpEq => A::And,
		Kind::QuestionQuestionEq => A::Nullish,
		_ => return None,
	})
}

impl<'a, const COUNT: bool, const TS: bool> Parser<'a, COUNT, TS> {
	pub fn new(src: &'a str, ast: &'a mut Ast, scratch: &'a mut Scratch) -> Self {
		Self::with(src, ast, scratch, true)
	}

	pub fn with(src: &'a str, ast: &'a mut Ast, scratch: &'a mut Scratch, module: bool) -> Self {
		Self::with_ts(src, ast, scratch, module, false, false)
	}

	pub fn with_ts(
		src: &'a str,
		ast: &'a mut Ast,
		scratch: &'a mut Scratch,
		module: bool,
		ts: bool,
		erase: bool,
	) -> Self {
		debug_assert_eq!(ts, TS);
		let mut lexer = Lexer::new(src, module, true);
		lexer.at_sign = TS;
		if TS {
			scratch.ts.clear();
		}
		Self::with_lexer(src, ast, scratch, module, erase, lexer)
	}

	/// A parser reading with `lexer`, nothing read; TypeScript's tables are the caller's to clear.
	#[inline(always)]
	pub(super) fn with_lexer(
		src: &'a str,
		ast: &'a mut Ast,
		scratch: &'a mut Scratch,
		module: bool,
		erase: bool,
		lexer: Lexer<'a>,
	) -> Self {
		Parser {
			src,
			lexer,
			tok: Token {
				start: 0,
				end: 0,
				kind: Kind::Eof,
				flags: 0,
			},
			prev_end: 0,
			ast,
			scratch,
			arrow_at: u32::MAX,
			arrow_for_await: false,
			yield_pos: 0,
			await_pos: 0,
			await_ident_pos: 0,
			bare_arrow: false,
			covering: false,
			lists_forgotten: false,
			ctx: Ctx {
				can_await: module,
				..Ctx::default()
			},
			module,
			strict: module,
			depth: 0,
			error: false,
			err: None,
			err_at: 0,
			err_first: false,
			unsupported: None,
			tokens: 0,
			fresh: 0,
			high: 0,
			in_type: false,
			modifiers_ok: false,
			erase,
			options: crate::parser::Options(
				if module { crate::parser::Options::MODULE } else { 0 }
					| if erase { crate::parser::Options::ERASE } else { 0 },
			),
			recover: false,
			speculating: 0,
			track: false,
			stops: "",
			nest: 0,
			open: [0; 3],
			unmatched: false,
			stop_hit: false,
			stop_word_at: None,
			forced_stop: None,
			base: 0,
			tree_limit: usize::MAX,
			liberal_at: u32::MAX,
		}
	}

	pub fn ast(&mut self) -> &mut Ast {
		self.ast
	}

	/// The expression at `at`: its node and where it ends, a comment after it included; none
	/// when it is not one or uses what this prototype does not read.
	pub fn expression_at(&mut self, at: u32) -> Option<(NodeId, u32)> {
		self.error = false;
		self.err = None;
		self.err_first = false;
		self.unsupported = None;
		self.scratch.checks.clear();
		if TS {
			self.scratch.ts.clear();
			self.in_type = false;
		}
		self.enter_scope(super::checks::SCOPE_TOP);
		self.bare_arrow = false;
		self.arrow_at = u32::MAX;
		self.lexer.set_pos(at as usize);
		self.lexer.comments.clear();
		self.prev_end = at;
		self.tok = self.lexer.next(true);
		if COUNT {
			self.tokens += 1;
		}
		let node = self.sequence();
		if self.error {
			return None;
		}
		let end = match self.lexer.comments.last() {
			Some(comment) if comment.start >= self.prev_end => comment.end,
			_ => self.prev_end,
		};
		Some((node, end))
	}

	/// A word that cannot name a binding where the cursor stands, the keywords apart: those are
	/// kinds of token.
	#[inline]
	pub(super) fn reserved(&self, word: &[u8]) -> bool {
		// the words start with one of seven letters and are 3 to 10 long
		const STARTS: u32 = 1 << (b'a' - b'a')
			| 1 << (b'e' - b'a')
			| 1 << (b'i' - b'a')
			| 1 << (b'l' - b'a')
			| 1 << (b'p' - b'a')
			| 1 << (b's' - b'a')
			| 1 << (b'y' - b'a');
		let first = word[0].wrapping_sub(b'a');
		if first >= 26 || STARTS >> first & 1 == 0 {
			return false;
		}
		match word {
			b"enum" => true,
			b"await" => self.module || self.ctx.is_async,
			b"yield" => self.strict || self.ctx.generator,
			b"implements" | b"interface" | b"let" | b"package" | b"private" | b"protected" | b"public"
			| b"static" => self.strict,
			_ => false,
		}
	}

	/// The token at the cursor is the word `word`, written without an escape.
	#[inline(always)]
	pub(super) fn is_word(&self, word: &[u8]) -> bool {
		let t = self.tok;
		t.kind == Kind::Ident
			&& (t.end - t.start) as usize == word.len()
			&& t.flags & ESCAPED == 0
			&& &self.src.as_bytes()[t.start as usize..t.end as usize] == word
	}

	/// The next token of the lexer, split where a type stands.
	pub(super) fn next_token(&mut self, regex: bool) -> Token {
		let token = self.lexer.next(regex);
		if TS && self.in_type {
			return self.split_angle(token);
		}
		token
	}

	/// `bump` where brackets are counted and the host's stops tested: with stops, or under recovery.
	#[cold]
	#[inline(never)]
	fn bump_tracked(&mut self, regex: bool) {
		let prev = self.tok;
		self.prev_end = prev.end;
		self.tok = self.lexer.next(regex);
		if TS && self.in_type {
			self.tok = self.split_angle(self.tok);
		}
		self.tracked(prev, false);
		if COUNT {
			self.tokens += 1;
			if self.tok.start >= self.high {
				self.fresh += 1;
				self.high = self.tok.start + 1;
			}
		}
	}

	/// The next token as the parser in use reads one it skips: a `/` is an operator, and a
	/// backquote a token of its own.
	pub(super) fn bump_raw(&mut self) {
		self.bump(false);
		self.raw_backquote();
	}

	#[inline(always)]
	pub(super) fn bump(&mut self, regex: bool) {
		if self.track {
			return self.bump_tracked(regex);
		}
		self.prev_end = self.tok.end;
		self.tok = self.lexer.next(regex);
		if TS && self.in_type {
			self.tok = self.split_angle(self.tok);
		}
		if COUNT {
			self.tokens += 1;
			if self.tok.start >= self.high {
				self.fresh += 1;
				self.high = self.tok.start + 1;
			}
		}
	}

	#[inline(always)]
	pub(super) fn add(&mut self, kind: NodeKind, start: u32) -> NodeId {
		self.ast.add(kind, start, self.prev_end)
	}

	#[inline(always)]
	pub(super) fn newline(&self) -> bool {
		self.tok.flags & NEWLINE_BEFORE != 0
	}

	#[inline(always)]
	pub(super) fn list(&mut self, base: usize) -> List {
		let list = self.ast.add_list(&self.scratch.items[base..]);
		self.scratch.items.truncate(base);
		list
	}

	/// The token at the cursor is not what the grammar allows; at the end of the input that is
	/// its own error. The error is recorded and the input ends: every caller reads `Eof` from
	/// here and returns.
	#[cold]
	#[inline(never)]
	pub(super) fn fail(&mut self) -> NodeId {
		let code = if self.tok.kind == Kind::Eof && !self.stopped() {
			Code::UnexpectedEof
		} else {
			Code::UnexpectedToken
		};
		self.raise(self.tok.start, code)
	}

	/// Where an error at the token at the cursor ends: a template's is its backquote alone, a
	/// regular expression's its `/` or `/=`, the token the parser in use errs at before it
	/// reads one.
	pub(super) fn tok_end(&self) -> u32 {
		let t = self.tok;
		match t.kind {
			Kind::Template | Kind::TemplateHead => t.start + 1,
			Kind::RegExp => t.start + 1 + (self.src.as_bytes().get(t.start as usize + 1) == Some(&b'=')) as u32,
			_ => t.end,
		}
	}

	/// `code` at `pos`: over the token at the cursor when it starts there, else at a point.
	#[cold]
	#[inline(never)]
	pub(super) fn raise(&mut self, pos: u32, code: Code) -> NodeId {
		let end = if pos == self.tok.start { self.tok_end() } else { pos };
		self.record(SyntaxError::new(pos, code).to(end))
	}

	#[cold]
	#[inline(never)]
	pub(super) fn raise_arg(&mut self, pos: u32, code: Code, arg: &str) -> NodeId {
		let end = if pos == self.tok.start { self.tok_end() } else { pos };
		let arg = self.ast.strings.intern(arg);
		self.record(SyntaxError::new(pos, code).to(end).arg(arg))
	}

	#[cold]
	#[inline(never)]
	pub(super) fn raise_name(&mut self, pos: u32, code: Code, name: StrId) -> NodeId {
		let end = if pos == self.tok.start { self.tok_end() } else { pos };
		self.record(SyntaxError::new(pos, code).to(end).arg(name))
	}

	#[cold]
	#[inline(never)]
	pub(super) fn record(&mut self, error: SyntaxError) -> NodeId {
		if !self.error {
			self.error = true;
			self.err = Some(error);
			self.err_at = self.tok.start;
			if self.recover {
				self.meet();
			}
		}
		self.tok = self.lexer.end();
		NodeId::at(0)
	}

	/// An error of the lexer's, its argument in the tree's strings.
	pub(super) fn lexed_error(&mut self, e: super::LexError) -> SyntaxError {
		let error = SyntaxError::new(e.pos, e.code).to(e.end);
		match e.arg {
			Arg::None => error,
			Arg::Char(c) => error.arg(self.ast.strings.intern(c.encode_utf8(&mut [0; 4]))),
			Arg::Radix(radix) => error.arg(self.ast.strings.intern(&radix.to_string())),
		}
	}

	/// The error the parse ended with: the lexer's when it met one, since the grammar only saw
	/// the end of the input after it; and before either, a keyword spelled with an escape that
	/// was read as that keyword.
	pub fn take_error(&mut self) -> Option<SyntaxError> {
		let lexed = self.lexer.error.take().map(|e| self.lexed_error(e));
		let (chosen, at) = match (lexed, self.err.take()) {
			(Some(_), Some(e)) if self.err_first => (Some(e), self.err_at),
			(Some(e), _) => (Some(e), e.pos),
			(None, Some(e)) => (Some(e), self.err_at),
			(None, None) => (None, u32::MAX),
		};
		if let Some((start, end)) = self.lexer.escaped_keyword.take()
			&& at > start
		{
			let word = self.scratch.values.word(self.src, start as usize, end as usize);
			let arg = self.ast.strings.intern(word);
			return Some(SyntaxError::new(start, Code::EscapeInKeyword).to(end).arg(arg));
		}
		chosen
	}

	#[cold]
	#[inline(never)]
	fn skip(&mut self, why: &'static str) -> NodeId {
		if self.unsupported.is_none() && !self.error {
			self.unsupported = Some(why);
		}
		self.fail()
	}

	#[inline(always)]
	pub(super) fn expect(&mut self, kind: Kind, regex: bool) {
		if self.tok.kind != kind {
			self.fail();
			return;
		}
		self.bump(regex);
	}

	// ── values: a token's text is its value unless it holds an escape

	#[inline(always)]
	pub(super) fn word(&mut self, t: Token) -> StrId {
		if t.flags & ESCAPED != 0 {
			return self.word_slow(t);
		}
		self.ast.strings.intern(&self.src[t.start as usize..t.end as usize])
	}

	/// A word written with an escape: what it spells.
	#[cold]
	#[inline(never)]
	pub(super) fn word_slow(&mut self, t: Token) -> StrId {
		let from = if t.kind == Kind::PrivateName { t.start + 1 } else { t.start };
		let word = self.scratch.values.word(self.src, from as usize, t.end as usize);
		self.ast.strings.intern(word)
	}

	/// The keyword at the cursor, spelled with an escape, is read as a name: no error.
	#[cold]
	#[inline(never)]
	pub(super) fn named(&mut self, t: Token) {
		self.liberal_at = t.start;
		if self.lexer.escaped_keyword.is_some_and(|(start, _)| start == t.start) {
			self.lexer.escaped_keyword = None;
		}
	}

	fn number(&mut self, t: Token) -> f64 {
		values::number(&self.src[t.start as usize..t.end as usize])
	}

	#[inline(always)]
	fn string(&mut self, t: Token) -> StrId {
		if t.flags & (ESCAPED | BROKEN) != 0 {
			return self.string_slow(t);
		}
		self.ast.strings.intern(&self.src[t.start as usize + 1..t.end as usize - 1])
	}

	#[cold]
	#[inline(never)]
	fn string_slow(&mut self, t: Token) -> StrId {
		// one its line ends inside, under recovery, has no closing quote
		let end = t.end as usize + (t.flags & BROKEN != 0) as usize;
		self.scratch
			.values
			.string(self.src, t.start as usize, end, &mut self.ast.strings)
	}

	/// One chunk of a template, `trail` the bytes of its token after its text; `tagged` takes
	/// an escape that says nothing.
	fn quasi(&mut self, t: Token, trail: u32, tail: bool, tagged: bool) -> NodeId {
		let start = t.start + 1;
		let mut trail = trail;
		if t.flags & BROKEN != 0 {
			if !self.recover {
				return self.raise(start, Code::UnterminatedTemplate);
			}
			// under recovery the chunk ends with the text, as if its backquote stood there
			self.lexer.soft(SyntaxError::new(start, Code::UnterminatedTemplate), None);
			trail = 0;
		}
		let end = t.end - trail;
		if t.flags & ESCAPED == 0 {
			let raw = self.ast.strings.intern(&self.src[start as usize..end as usize]);
			return self.ast.add(
				NodeKind::TemplateElement {
					cooked: Some(raw),
					raw,
					tail,
				},
				start,
				end,
			);
		}
		let chunk = self
			.scratch
			.values
			.template(self.src, start as usize, end as usize, &mut self.ast.strings);
		if chunk.cooked.is_none() && !tagged {
			let error = self.error_at(start, Code::BadTemplateEscape);
			if !self.softly(error) {
				return NodeId::at(0);
			}
		}
		self.ast.add(
			NodeKind::TemplateElement {
				cooked: chunk.cooked,
				raw: chunk.raw,
				tail,
			},
			start,
			end,
		)
	}

	// ── expressions

	pub(super) fn sequence(&mut self) -> NodeId {
		self.sequence_with(&mut None)
	}

	pub(super) fn sequence_with(&mut self, errors: &mut Errors) -> NodeId {
		let start = self.tok.start;
		let first = self.assign_with(errors);
		if self.tok.kind != Kind::Comma {
			return first;
		}
		let base = self.scratch.items.len();
		self.scratch.items.push(Some(first));
		while self.tok.kind == Kind::Comma {
			self.bump(true);
			let next = self.element(errors);
			self.scratch.items.push(Some(next));
		}
		if let Some(e) = errors {
			e.settle_as_operand();
		}
		let expressions = self.list(base);
		self.add(NodeKind::SequenceExpression { expressions }, start)
	}

	/// An element of a list read with facts of its own, which join the list's.
	#[inline(always)]
	fn element(&mut self, list: &mut Errors) -> NodeId {
		// only a literal or a list in parentheses has facts; anything else hands up at most an
		// operand's error from the right of an assignment, straight into the list's
		if list.is_none() || !matches!(self.tok.kind, Kind::BraceL | Kind::BracketL | Kind::ParenL) {
			return self.assign_with(list);
		}
		let mut facts = Some(Facts::default());
		let node = self.assign_with(&mut facts);
		if let (Some(list), Some(facts)) = (list.as_mut(), facts) {
			list.merge(facts);
		}
		node
	}

	pub(super) fn assign(&mut self) -> NodeId {
		self.assign_with(&mut None)
	}

	pub(super) fn assign_with(&mut self, errors: &mut Errors) -> NodeId {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			return self.raise(self.tok.start, Code::NestingDepth);
		}
		if TS && (Kind::Lt as u8..=Kind::LtEq as u8).contains(&(self.tok.kind as u8)) {
			let arrow = self.maybe_assign_ts();
			if self.error {
				return NodeId::at(0);
			}
			if let Some(arrow) = arrow {
				self.bare_arrow = false;
				self.depth -= 1;
				return arrow;
			}
		}
		if self.ctx.generator && self.is_word(b"yield") {
			let node = self.yield_expression();
			self.depth -= 1;
			return node;
		}
		let start = self.tok.start;
		self.arrow_at = start;
		self.arrow_for_await = self.ctx.for_await;
		if matches!(self.tok.kind, Kind::BraceL | Kind::BracketL | Kind::ParenL) {
			return self.assign_literal(errors, start);
		}
		// what starts with anything else has no facts of its own
		let mut left = self.unary(true, &mut None);
		if !std::mem::take(&mut self.bare_arrow) {
			left = self.binary(left, start, 0);
			if self.tok.kind == Kind::Question {
				left = self.conditional(left, start);
			}
		}
		if let Some(operator) = assignment(self.tok.kind) {
			if operator == AssignmentOperator::Assign {
				left = self.make_pattern(left, None);
				self.check_lval_pattern(left, Binding::None, false);
			} else {
				self.check_lval_simple(left, Binding::None, false);
			}
			if self.error {
				return left;
			}
			self.bump(true);
			let right = match errors {
				None => self.assign_with(&mut None),
				Some(list) => {
					let mut right_facts = Some(Facts::default());
					let right = self.assign_with(&mut right_facts);
					let mut right_facts = right_facts.unwrap();
					right_facts.settle_as_operand();
					list.operand = list.operand.or(right_facts.operand);
					right
				}
			};
			self.depth -= 1;
			return self.add(NodeKind::AssignmentExpression { operator, left, right }, start);
		}
		self.depth -= 1;
		left
	}

	#[inline(always)]
	fn conditional(&mut self, test: NodeId, start: u32) -> NodeId {
		self.bump(true);
		let no_in = std::mem::replace(&mut self.ctx.no_in, false);
		let consequent = if TS {
			self.conditional_branch(start, false)
		} else {
			self.assign()
		};
		self.ctx.no_in = no_in;
		self.expect(Kind::Colon, true);
		let alternate = if TS {
			self.conditional_branch(start, true)
		} else {
			self.assign()
		};
		self.add(
			NodeKind::ConditionalExpression {
				test,
				consequent,
				alternate,
			},
			start,
		)
	}

	/// An assignment expression that starts with a literal or a list in parentheses: what may
	/// yet be a pattern, with its facts.
	#[inline(never)]
	fn assign_literal(&mut self, errors: &mut Errors, start: u32) -> NodeId {
		let handing = errors.is_some();
		let mut own: Errors;
		let facts = if handing {
			errors
		} else {
			own = Some(Facts::default());
			&mut own
		};
		let mut left = self.unary(true, facts);
		let arrow = std::mem::take(&mut self.bare_arrow);
		if !arrow && !facts.as_ref().unwrap().pending() {
			left = self.binary(left, start, 0);
			if self.tok.kind == Kind::Question {
				left = self.conditional(left, start);
			}
		}
		let e = facts.as_mut().unwrap();
		let target = e.owner == Some(left);
		if !target && e.owner.is_some() {
			e.settle_as_operand();
		}
		if let Some(operator) = assignment(self.tok.kind) {
			if operator == AssignmentOperator::Assign {
				let held = *facts;
				left = self.make_pattern(left, held);
				self.check_lval_pattern(left, Binding::None, false);
			} else {
				self.check_lval_simple(left, Binding::None, false);
			}
			if self.error {
				return left;
			}
			self.bump(true);
			if target {
				*facts = Some(Facts::default());
			}
			let mut right_facts = handing.then(Facts::default);
			let right = self.assign_with(&mut right_facts);
			if let Some(mut right_facts) = right_facts {
				right_facts.settle_as_operand();
				facts.as_mut().unwrap().merge(right_facts);
			}
			if !handing && facts.as_ref().unwrap().pending() {
				let held = facts.unwrap();
				self.raise_expression_errors(&held);
			}
			self.depth -= 1;
			return self.add(NodeKind::AssignmentExpression { operator, left, right }, start);
		}
		if !handing && facts.as_ref().unwrap().pending() {
			let held = facts.unwrap();
			self.raise_expression_errors(&held);
		}
		self.depth -= 1;
		left
	}

	/// The operators after `left`, which starts at `left_start`, that bind tighter than `min`.
	fn binary(&mut self, mut left: NodeId, left_start: u32, min: u8) -> NodeId {
		let mut links = 0;
		loop {
			links += 1;
			if links > MAX_CHAIN {
				return self.raise(self.tok.start, Code::NestingDepth);
			}
			let kind = self.tok.kind;
			let prec = PREC[kind as usize];
			if prec <= min || (kind == Kind::In && self.ctx.no_in) {
				if kind == Kind::Ident && TS && min < 7 && !self.newline() {
					let is_as = self.is_word(b"as");
					if is_as || self.is_word(b"satisfies") {
						if self.stop_hit {
							if self.forced_stop == Some(self.tok.start) {
								self.stop_here();
								return left;
							}
							// read as the assertion for now; the entry decides afterwards which one was the host's
							self.stop_word_at = Some(self.tok.start);
						}
						left = self.assertion(left, left_start, is_as);
						if self.error {
							return left;
						}
						continue;
					}
				}
				return left;
			}
			self.bump(true);
			let right_start = self.tok.start;
			let operand = self.unary(true, &mut None);
			self.depth += 1;
			if self.depth > MAX_DEPTH {
				return self.raise(self.tok.start, Code::NestingDepth);
			}
			let right = self.binary(operand, right_start, prec);
			self.depth -= 1;
			if matches!(self.ast.node(right).kind, NodeKind::PrivateIdentifier { .. }) && !self.error {
				let at = self.ast.node(right).start;
				return self.raise(at, Code::PrivateNameOutsideIn);
			}
			left = self.add(binary_node(kind, left, right), left_start);
			let next = self.tok.kind;
			if (matches!(kind, Kind::PipePipe | Kind::AmpAmp) && next == Kind::QuestionQuestion)
				|| (kind == Kind::QuestionQuestion && matches!(next, Kind::PipePipe | Kind::AmpAmp))
			{
				return self.raise(self.tok.start, Code::MixedCoalesce);
			}
		}
	}

	/// A prefix operator's expression, or an atom with what follows it; `exp` says a `**` may
	/// follow, which no operator before it allows.
	#[inline(always)]
	pub(super) fn unary(&mut self, exp: bool, errors: &mut Errors) -> NodeId {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			return self.raise(self.tok.start, Code::NestingDepth);
		}
		let node = self.unary_inner(exp, errors);
		self.depth -= 1;
		node
	}

	fn unary_inner(&mut self, exp: bool, errors: &mut Errors) -> NodeId {
		let start = self.tok.start;
		let operator = match self.tok.kind {
			Kind::Minus => UnaryOperator::Minus,
			Kind::Plus => UnaryOperator::Plus,
			Kind::Bang => UnaryOperator::Not,
			Kind::Tilde => UnaryOperator::BitNot,
			Kind::Typeof => UnaryOperator::Typeof,
			Kind::Void => UnaryOperator::Void,
			Kind::Delete => UnaryOperator::Delete,
			kind => {
				if TS && (Kind::Lt as u8..=Kind::LtEq as u8).contains(&(kind as u8)) && self.starts_with_lt() {
					return self.parse_type_assertion();
				}
				return self.operand(start, exp, errors);
			}
		};
		self.bump(true);
		let argument = self.unary(false, &mut None);
		if operator == UnaryOperator::Delete && !self.error {
			self.check_delete(start, argument);
		}
		self.add(NodeKind::UnaryExpression { operator, argument }, start)
	}

	#[cold]
	#[inline(never)]
	fn check_delete(&mut self, start: u32, argument: NodeId) {
		if self.strict && matches!(self.ast.node(argument).kind, NodeKind::Identifier { .. }) {
			self.raise(start, Code::StrictDelete);
		} else if self.is_private_field_access(argument) {
			self.raise(start, Code::DeletePrivate);
		}
	}

	fn is_private_field_access(&self, id: NodeId) -> bool {
		match self.ast.node(id).kind {
			NodeKind::MemberExpression { property, .. } => {
				matches!(self.ast.node(property).kind, NodeKind::PrivateIdentifier { .. })
			}
			NodeKind::ChainExpression { expression } => self.is_private_field_access(expression),
			_ => false,
		}
	}

	#[inline(always)]
	fn operand(&mut self, start: u32, exp: bool, errors: &mut Errors) -> NodeId {
		let kind = self.tok.kind;
		let mut e;
		if kind == Kind::PlusPlus || kind == Kind::MinusMinus {
			let operator = if kind == Kind::PlusPlus {
				UpdateOperator::Increment
			} else {
				UpdateOperator::Decrement
			};
			self.bump(true);
			let argument = self.unary(false, &mut None);
			self.check_lval_simple(argument, Binding::None, false);
			e = self.add(
				NodeKind::UpdateExpression {
					operator,
					prefix: true,
					argument,
				},
				start,
			);
		} else if kind == Kind::Ident && self.at_await() {
			if self.await_pos == 0 {
				self.await_pos = start;
			}
			self.bump(true);
			let argument = self.unary(false, &mut None);
			return self.add(NodeKind::AwaitExpression { argument }, start);
		} else if kind == Kind::PrivateName && exp {
			return self.private_in();
		} else {
			e = self.atom(errors, false);
			if self.bare_arrow {
				return e;
			}
			e = self.suffixes(e, start, true);
			if self.bare_arrow {
				return e;
			}
			if errors.as_ref().is_some_and(|facts| facts.pending()) {
				return e;
			}
			while (self.tok.kind == Kind::PlusPlus || self.tok.kind == Kind::MinusMinus) && !self.newline() {
				let operator = if self.tok.kind == Kind::PlusPlus {
					UpdateOperator::Increment
				} else {
					UpdateOperator::Decrement
				};
				self.check_lval_simple(e, Binding::None, false);
				if self.error {
					return e;
				}
				self.bump(false);
				e = self.add(
					NodeKind::UpdateExpression {
						operator,
						prefix: false,
						argument: e,
					},
					start,
				);
			}
		}
		if exp && self.tok.kind == Kind::StarStar {
			self.bump(true);
			let right = self.unary(true, &mut None);
			if matches!(self.ast.node(right).kind, NodeKind::PrivateIdentifier { .. }) && !self.error {
				let at = self.ast.node(right).start;
				return self.raise(at, Code::PrivateNameOutsideIn);
			}
			return self.add(
				NodeKind::BinaryExpression {
					operator: BinaryOperator::Exp,
					left: e,
					right,
				},
				start,
			);
		}
		e
	}

	/// `#name` where an operand starts: the left of an `in`, inside a class.
	#[cold]
	#[inline(never)]
	fn private_in(&mut self) -> NodeId {
		if self.ctx.no_in || self.scratch.checks.private_names.is_empty() {
			return self.fail();
		}
		let name = self.private_name();
		if self.tok.kind != Kind::In {
			return self.fail();
		}
		name
	}

	/// `await` where it is the operator: at the top of a module or in an async function.
	#[inline(always)]
	fn at_await(&self) -> bool {
		let t = self.tok;
		t.end - t.start == 5
			&& t.flags & ESCAPED == 0
			&& &self.src.as_bytes()[t.start as usize..t.end as usize] == b"await"
			&& self.ctx.can_await
	}

	/// What follows an atom: members, calls and tagged templates, `calls` unless it is what
	/// `new` constructs.
	#[inline(always)]
	pub(super) fn suffixes(&mut self, mut base: NodeId, start: u32, calls: bool) -> NodeId {
		let mut chained = false;
		// decided once, for the name `async` with its `(` on the same line; the parser in use
		// keeps the answer for every call of the chain, so `async.x(a) => b` is an arrow function
		let maybe_arrow = calls
			&& self.arrow_at == start
			&& !self.newline()
			&& !matches!(self.tok.kind, Kind::Eof | Kind::BraceR)
			&& self.prev_end - start == 5
			&& matches!(self.tok.kind, Kind::ParenL | Kind::Dot | Kind::BracketL | Kind::QuestionDot | Kind::Template | Kind::TemplateHead)
			&& self.is_async(base);
		let mut links = 0;
		loop {
			links += 1;
			if links > MAX_CHAIN {
				return self.raise(self.tok.start, Code::NestingDepth);
			}
			if TS && matches!(self.tok.kind, Kind::Bang | Kind::Lt | Kind::LtLt | Kind::QuestionDot) {
				let answer = self.subscript_ts(base, start, !calls, chained);
				if self.error {
					return base;
				}
				if let Some((node, optional)) = answer {
					if optional {
						chained = true;
					}
					if node == base {
						break;
					}
					if matches!(self.ast.node(node).kind, NodeKind::ArrowFunctionExpression { .. }) {
						return node;
					}
					base = node;
					continue;
				}
			}
			match self.tok.kind {
				Kind::Dot => {
					self.bump(false);
					let property = self.member_name(base);
					base = self.add(
						NodeKind::MemberExpression {
							object: base,
							property,
							computed: false,
							optional: false,
						},
						start,
					);
				}
				Kind::BracketL => {
					self.bump(true);
					let no_in = std::mem::replace(&mut self.ctx.no_in, false);
					let property = self.sequence();
					self.ctx.no_in = no_in;
					self.expect(Kind::BracketR, false);
					base = self.add(
						NodeKind::MemberExpression {
							object: base,
							property,
							computed: true,
							optional: false,
						},
						start,
					);
				}
				Kind::ParenL if calls => {
					let open = self.tok.start;
					self.bump(true);
					if maybe_arrow {
						if let Some(arrow) = self.decided_arrow(start, open, true) {
							return arrow;
						}
						if self.error {
							return base;
						}
					}
					let items = self.arguments();
					let arguments = self.list(items);
					base = self.add(
						NodeKind::CallExpression {
							callee: base,
							arguments,
							optional: false,
						},
						start,
					);
				}
				Kind::QuestionDot => {
					self.bump(true);
					if !calls {
						return self.raise(self.prev_end - 2, Code::OptionalChainInNew);
					}
					chained = true;
					match self.tok.kind {
						Kind::ParenL => {
							self.bump(true);
							let items = self.arguments();
							let arguments = self.list(items);
							base = self.add(
								NodeKind::CallExpression {
									callee: base,
									arguments,
									optional: true,
								},
								start,
							);
						}
						Kind::BracketL => {
							self.bump(true);
							let no_in = std::mem::replace(&mut self.ctx.no_in, false);
							let property = self.sequence();
							self.ctx.no_in = no_in;
							self.expect(Kind::BracketR, false);
							base = self.add(
								NodeKind::MemberExpression {
									object: base,
									property,
									computed: true,
									optional: true,
								},
								start,
							);
						}
						Kind::Template | Kind::TemplateHead => {
							return self.raise(self.tok.start, Code::OptionalChainInTaggedTemplate);
						}
						_ => {
							let property = self.member_name(base);
							base = self.add(
								NodeKind::MemberExpression {
									object: base,
									property,
									computed: false,
									optional: true,
								},
								start,
							);
						}
					}
				}
				Kind::Template | Kind::TemplateHead => {
					if chained {
						return self.raise(self.tok.start, Code::OptionalChainInTaggedTemplate);
					}
					let quasi = self.template(true);
					base = self.add(NodeKind::TaggedTemplateExpression { tag: base, quasi }, start);
				}
				_ => break,
			}
			if self.error {
				return base;
			}
		}
		if chained {
			base = self.add(NodeKind::ChainExpression { expression: base }, start);
		}
		base
	}

	#[cold]
	#[inline(never)]
	pub(super) fn is_async(&self, id: NodeId) -> bool {
		let node = self.ast.node(id);
		matches!(node.kind, NodeKind::Identifier { .. })
			&& node.end == self.prev_end
			&& &self.src.as_bytes()[node.start as usize..node.end as usize] == b"async"
	}

	/// A member's name after `.`: any word, or a private name unless it is `super`'s.
	#[inline(always)]
	fn member_name(&mut self, base: NodeId) -> NodeId {
		if self.tok.kind == Kind::PrivateName && !matches!(self.ast.node(base).kind, NodeKind::Super) {
			return self.private_name();
		}
		self.name()
	}

	/// A property's name: any word, a keyword too.
	#[inline(always)]
	pub(super) fn name(&mut self) -> NodeId {
		let t = self.tok;
		if t.kind == Kind::Ident || t.kind.is_keyword() {
			let name = if t.flags & ESCAPED != 0 {
				self.named(t);
				self.word_slow(t)
			} else {
				self.ast.strings.intern(&self.src[t.start as usize..t.end as usize])
			};
			self.bump(false);
			return self.ast.add(NodeKind::Identifier { name }, t.start, t.end);
		}
		self.placeholder()
	}

	/// `#name` at the cursor, used: a class around it declares it, now or later.
	#[cold]
	#[inline(never)]
	pub(super) fn private_name(&mut self) -> NodeId {
		let t = self.tok;
		if t.kind != Kind::PrivateName {
			return self.fail();
		}
		let name = if t.flags & ESCAPED != 0 {
			self.word_slow(t)
		} else {
			self.ast.strings.intern(&self.src[t.start as usize + 1..t.end as usize])
		};
		self.bump(false);
		let id = self.ast.add(NodeKind::PrivateIdentifier { name }, t.start, t.end);
		match self.scratch.checks.private_names.last_mut() {
			Some(scope) => scope.used.push((name, t.start)),
			None => {
				let text = format!("#{}", self.ast.strings.get(name));
				return self.raise_arg(t.start, Code::UndeclaredPrivateName, &text);
			}
		}
		id
	}

	#[inline(always)]
	pub(super) fn atom(&mut self, errors: &mut Errors, for_new: bool) -> NodeId {
		let t = self.tok;
		let start = t.start;
		match t.kind {
			Kind::Ident => self.identifier(t),
			Kind::String => {
				let value = self.string(t);
				self.bump(false);
				self.ast.add(NodeKind::StringLiteral { value }, start, t.end)
			}
			Kind::Number => {
				let value = self.number(t);
				let value = self.ast.number(value);
				self.bump(false);
				self.ast.add(NodeKind::NumberLiteral { value }, start, t.end)
			}
			Kind::ParenL => {
				let expr = self.paren(start);
				if let Some(e) = errors
					&& !self.error
				{
					if e.parenthesized_assign.is_none() && !self.is_simple_assign_target(expr) {
						e.parenthesized_assign = Some(start);
					}
					e.owner = Some(expr);
				}
				expr
			}
			Kind::BraceL => {
				let object = self.object(start, errors);
				if let Some(e) = errors {
					e.owner = Some(object);
				}
				object
			}
			Kind::BracketL => {
				let array = self.array(start, errors);
				if let Some(e) = errors {
					e.owner = Some(array);
				}
				array
			}
			Kind::Template | Kind::TemplateHead => self.template(false),
			Kind::True | Kind::False => {
				self.bump(false);
				self.ast.add(
					NodeKind::BooleanLiteral {
						value: t.kind == Kind::True,
					},
					start,
					t.end,
				)
			}
			Kind::Null => {
				self.bump(false);
				self.ast.add(NodeKind::NullLiteral, start, t.end)
			}
			Kind::This => {
				self.bump(false);
				self.ast.add(NodeKind::ThisExpression, start, t.end)
			}
			Kind::BigInt => {
				self.bump(false);
				self.ast.add(NodeKind::BigIntLiteral, start, t.end)
			}
			Kind::RegExp => self.regexp(t),
			Kind::New => self.construct(start),
			Kind::Function => {
				self.bump(false);
				self.function(start, false, false, false, false)
			}
			Kind::Class => self.class(false, false),
			Kind::Import => self.import_expression(start, for_new),
			Kind::Super => self.super_atom(t),
			// a `/` where an operand stands starts a regular expression, whatever was said when it was read
			Kind::Slash | Kind::SlashEq => {
				let t = self.lexer.regex(t);
				if self.lexer.error.is_some() {
					if self.recover {
						// what a regular expression's flags hold wrongly is the grammar's error, met here
						let lexed = self.lexer.error.take().unwrap();
						let error = self.lexed_error(lexed);
						self.lexer.rewind(self.lexer.fail_pos());
						return self.record(error);
					}
					// the lexer's error is the parse's
					self.tok = t;
					return self.fail();
				}
				self.tok = t;
				self.regexp(t)
			}
			Kind::At if TS => {
				self.parse_decorators(false);
				if self.failed() {
					if !self.error {
						self.fail();
					}
					return NodeId::at(0);
				}
				self.atom(errors, for_new)
			}
			_ => self.placeholder(),
		}
	}

	#[cold]
	#[inline(never)]
	fn super_atom(&mut self, t: Token) -> NodeId {
		let start = t.start;
		if !self.allow_super() {
			return self.raise(start, Code::SuperOutsideMethod);
		}
		self.bump(false);
		if self.tok.kind == Kind::ParenL && !self.allow_direct_super() {
			return self.raise(start, Code::SuperCallOutsideConstructor);
		}
		if !matches!(self.tok.kind, Kind::Dot | Kind::BracketL | Kind::ParenL) {
			return self.fail();
		}
		self.ast.add(NodeKind::Super, start, t.end)
	}

	/// A word that may be one the grammar reads its own way: by its first letter.
	#[inline(always)]
	pub(super) fn special(&self, t: Token) -> bool {
		// yield, await, arguments, enum, the strict-mode reserved words, a keyword spelled with an escape
		const STARTS: u32 = 1 << (b'a' - b'a')
			| 1 << (b'e' - b'a')
			| 1 << (b'i' - b'a')
			| 1 << (b'l' - b'a')
			| 1 << (b'p' - b'a')
			| 1 << (b's' - b'a')
			| 1 << (b'y' - b'a');
		let first = self.src.as_bytes()[t.start as usize].wrapping_sub(b'a');
		t.flags & ESCAPED != 0 || (first < 26 && STARTS >> first & 1 != 0)
	}

	/// A name where an identifier is needed: a keyword is read as one, and is an error after.
	#[inline(always)]
	pub(super) fn ident(&mut self) -> NodeId {
		let t = self.tok;
		if t.kind != Kind::Ident {
			return self.keyword_ident(t);
		}
		let name = self.word(t);
		self.bump(false);
		let id = self.ast.add(NodeKind::Identifier { name }, t.start, t.end);
		if self.special(t) {
			self.check_unreserved(id);
		}
		id
	}

	#[cold]
	#[inline(never)]
	fn keyword_ident(&mut self, t: Token) -> NodeId {
		if !t.kind.is_keyword() {
			return self.placeholder();
		}
		let name = self.word(t);
		self.bump(false);
		let id = self.ast.add(NodeKind::Identifier { name }, t.start, t.end);
		self.check_unreserved(id);
		id
	}

	#[inline(always)]
	fn identifier(&mut self, t: Token) -> NodeId {
		let start = t.start;
		let id = self.ident();
		if self.tok.kind == Kind::Function && !self.newline() && self.is_async(id) {
			self.bump(false);
			return self.function(start, true, false, false, false);
		}
		if self.arrow_at == start && !self.newline() {
			if self.tok.kind == Kind::Arrow {
				self.bump(true);
				let base = self.scratch.items.len();
				self.scratch.items.push(Some(id));
				return self.arrow(start, base, false);
			}
			if self.tok.kind == Kind::Ident
				&& self.is_async(id)
				&& t.flags & ESCAPED == 0
				&& !(self.arrow_for_await && self.is_word(b"of"))
			{
				return self.async_arrow(start);
			}
		}
		id
	}

	/// `async x => …` after `async`.
	#[cold]
	#[inline(never)]
	fn async_arrow(&mut self, start: u32) -> NodeId {
		let param = self.ident();
		if self.error {
			return param;
		}
		if matches!(self.tok.kind, Kind::Eof | Kind::BraceR) || self.newline() || self.tok.kind != Kind::Arrow {
			return self.fail();
		}
		self.bump(true);
		if self.ident_is(param, "await") {
			let at = self.ast.node(param).start;
			return self.raise(at, Code::AwaitAsIdentifier);
		}
		let base = self.scratch.items.len();
		self.scratch.items.push(Some(param));
		self.arrow(start, base, true)
	}

	// ── lists that may be an arrow function's parameters

	/// A list in parentheses, its `(` at the cursor: an arrow function's parameters when it is
	/// one, else the expression inside.
	pub(super) fn paren(&mut self, start: u32) -> NodeId {
		let can_arrow = self.arrow_at == start;
		self.bump(true);
		if can_arrow
			&& self.may_be_parameters()
			&& let Some(read) = self.arrow_list(start)
		{
			return read;
		}
		let no_in = std::mem::replace(&mut self.ctx.no_in, false);
		let base = self.scratch.items.len();
		let first = self.tok.start;
		let mut errors = Some(Facts::default());
		let mut more = false;
		loop {
			let item = if more {
				self.element(&mut errors)
			} else {
				self.assign_with(&mut errors)
			};
			self.scratch.items.push(Some(item));
			if self.tok.kind != Kind::Comma || self.error {
				break;
			}
			self.bump(true);
			more = true;
		}
		if more && let Some(e) = &mut errors {
			e.settle_as_operand();
		}
		let last = self.prev_end;
		self.ctx.no_in = no_in;
		if !self.error && self.tok.kind != Kind::ParenR {
			self.fail();
		}
		if self.error {
			self.scratch.items.truncate(base);
			return NodeId::at(0);
		}
		self.bump(false);
		let held = errors.unwrap();
		if held.pending() {
			self.raise_expression_errors(&held);
			self.scratch.items.truncate(base);
			return NodeId::at(0);
		}
		let value = if more {
			let expressions = self.list(base);
			self.ast.add(NodeKind::SequenceExpression { expressions }, first, last)
		} else {
			self.scratch.items.pop().unwrap().unwrap()
		};
		if self.options.has(crate::parser::Options::PARENTHESIZED) {
			self.ast.set_parenthesized(value);
		}
		value
	}

	/// A list that may be an arrow's parameters, its `(` read: the arrow, or the value in
	/// parentheses where one read tells which; none for a list read next as an expression.
	#[inline(always)]
	fn arrow_list(&mut self, start: u32) -> Option<NodeId> {
		if self.covering {
			return Some(self.fail());
		}
		// a list of names holds a level while it is read, before its role is asked
		if self.tok.kind == Kind::Ident && self.depth + 1 > MAX_DEPTH {
			return Some(self.raise(self.tok.start, Code::NestingDepth));
		}
		let read = match self.tok.kind {
			Kind::ParenR => return Some(self.empty_list(start)),
			Kind::Ident => {
				let next = self.lexer.peek_byte();
				if operator_follows::<TS>(next) {
					return None;
				}
				if matches!(next, Some(b',' | b')')) && self.plain_name(true) {
					self.name_list(start, next == Some(b')'))
				} else {
					None
				}
			}
			Kind::BraceL | Kind::BracketL => {
				if self.lists().roles.contains_key(&start) {
					None
				} else {
					self.bracket_item(start)
				}
			}
			_ => None,
		};
		if read.is_some() || self.error {
			return Some(read.unwrap_or(NodeId::at(0)));
		}
		self.decided_arrow(start, start, false)
	}

	/// `()`, its `)` at the cursor: an arrow function's parameters when `=>` follows, or with
	/// TypeScript a return type or a body; else an error at the `)`.
	fn empty_list(&mut self, start: u32) -> NodeId {
		let close = self.tok;
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		self.bump(false);
		if self.tok.kind == Kind::Arrow || (TS && matches!(self.tok.kind, Kind::Colon | Kind::BraceL)) {
			(self.yield_pos, self.await_pos) = (0, 0);
			let base = self.scratch.items.len();
			self.close_arrow_head();
			if self.error {
				return NodeId::at(0);
			}
			self.check_arrow_head(base, false, outer);
			if self.error {
				return NodeId::at(0);
			}
			return self.arrow(start, base, false);
		}
		// a token that does not lex after it: the parser in use reads that token for the arrow
		if !self.lexer.recover && self.lexer.error.is_some() {
			return self.fail();
		}
		let error = SyntaxError::new(close.start, Code::UnexpectedToken).to(close.end);
		if !self.softly(error) {
			return NodeId::at(0);
		}
		// under recovery an empty identifier stands in the parentheses
		let name = self.ast.strings.intern("");
		let value = self.ast.add(NodeKind::Identifier { name }, close.start, close.start);
		if self.options.has(crate::parser::Options::PARENTHESIZED) {
			self.ast.set_parenthesized(value);
		}
		value
	}

	/// A list of plain names, the first followed by `,` or `)` (`one`: it is the only one),
	/// read once: a name is the same `Identifier` as a parameter and as an operand, so only what
	/// follows the list is left to decide. The arrow or the value in parentheses; none for any
	/// other list, the parser back after its `(`.
	fn name_list(&mut self, start: u32, one: bool) -> Option<NodeId> {
		let back = (TS || !one).then(|| (self.tok_snap(), self.ast.mark()));
		let base = self.scratch.items.len();
		self.depth += 1;
		let plain = loop {
			if !self.plain_name(self.scratch.items.len() == base) {
				break false;
			}
			let name = self.ident();
			if self.error {
				return Some(name);
			}
			self.scratch.items.push(Some(name));
			if self.tok.kind == Kind::ParenR {
				break true;
			}
			if self.tok.kind != Kind::Comma {
				break false;
			}
			self.bump(true);
		};
		self.depth -= 1;
		if plain {
			self.bump(false);
			if self.tok.kind == Kind::Arrow && !self.newline() {
				let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
				(self.yield_pos, self.await_pos) = (0, 0);
				self.close_arrow_head();
				if self.error {
					return Some(NodeId::at(0));
				}
				self.check_arrow_head(base, false, outer);
				if self.error {
					return Some(NodeId::at(0));
				}
				return Some(self.arrow(start, base, false));
			}
			if !(TS && self.tok.kind == Kind::Colon) {
				let value = if self.scratch.items.len() - base > 1 {
					let first = self.ast.node(self.scratch.items[base].unwrap()).start;
					let last = self.ast.node(self.scratch.items.last().unwrap().unwrap()).end;
					let expressions = self.list(base);
					self.ast.add(NodeKind::SequenceExpression { expressions }, first, last)
				} else {
					self.scratch.items.pop().unwrap().unwrap()
				};
				if self.options.has(crate::parser::Options::PARENTHESIZED) {
					self.ast.set_parenthesized(value);
				}
				return Some(value);
			}
		}
		let (back, mark) = back.unwrap();
		self.restore_tokens(back);
		self.ast.truncate(mark);
		self.scratch.items.truncate(base);
		None
	}

	/// The token at the cursor is a name that reads the same as a parameter and as an operand,
	/// and a `,` or `)` follows it (`seen`: that is known).
	#[inline(always)]
	fn plain_name(&self, seen: bool) -> bool {
		let t = self.tok;
		t.kind == Kind::Ident
			&& t.flags & ESCAPED == 0
			&& !matches!(
				&self.src.as_bytes()[t.start as usize..t.end as usize],
				b"yield" | b"await"
			) && (seen || matches!(self.lexer.peek_byte(), Some(b',' | b')')))
	}

	/// A list whose first item starts with a bracket, read once when that item is all it holds
	/// and holds no list that may be parameters: as an expression its text is also a pattern's,
	/// so what follows the `)` tells which it is. None for any other list, the parser back after
	/// its `(`.
	// the tokens alone, a scan to the `)` of every such list: test262 266.5 -> 283.6 per token
	fn bracket_item(&mut self, start: u32) -> Option<NodeId> {
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		(self.yield_pos, self.await_pos) = (0, 0);
		let base = self.scratch.items.len();
		let read = self.attempt(|p| {
			let covering = std::mem::replace(&mut p.covering, true);
			let no_in = std::mem::replace(&mut p.ctx.no_in, false);
			let mut errors = Some(Facts::default());
			let item = p.assign_with(&mut errors);
			p.ctx.no_in = no_in;
			p.covering = covering;
			if p.error {
				return (item, false);
			}
			if p.tok.kind != Kind::ParenR {
				p.fail();
				return (item, false);
			}
			p.bump(false);
			let facts = errors.unwrap();
			if p.tok.kind == Kind::Arrow && !p.newline() {
				if facts.trailing_comma.is_some() || !p.binding_shaped(item, true) {
					p.fail();
				}
				return (item, true);
			}
			if TS && p.tok.kind == Kind::Colon || facts.pending() {
				p.fail();
			}
			(item, false)
		});
		let Some((item, arrow)) = read else {
			(self.yield_pos, self.await_pos) = (outer.0, outer.1);
			return self.error.then_some(NodeId::at(0));
		};
		if arrow {
			self.rebind(item);
			self.scratch.items.push(Some(item));
			self.close_arrow_head();
			if self.error {
				return Some(NodeId::at(0));
			}
			self.check_arrow_head(base, false, outer);
			if self.error {
				return Some(NodeId::at(0));
			}
			return Some(self.arrow(start, base, false));
		}
		// not an arrow: the list's yields and awaits are its surroundings'
		let inner = (self.yield_pos, self.await_pos);
		self.yield_pos = if outer.0 != 0 { outer.0 } else { inner.0 };
		self.await_pos = if outer.1 != 0 { outer.1 } else { inner.1 };
		if self.options.has(crate::parser::Options::PARENTHESIZED) {
			self.ast.set_parenthesized(item);
		}
		Some(item)
	}

	/// Whether what was read as an expression is written as a binding is: the same text read as
	/// a parameter gives the same nodes.
	fn binding_shaped(&self, id: NodeId, top: bool) -> bool {
		match self.ast.node(id).kind {
			NodeKind::Identifier { .. } => true,
			NodeKind::ObjectExpression { properties } => (0..properties.len).all(|i| {
				let property = self.ast.nth(properties, i).unwrap();
				match self.ast.node(property).kind {
					NodeKind::Property {
						value,
						kind: PropertyKind::Init,
						method: false,
						..
					} => self.binding_shaped(value, false),
					NodeKind::SpreadElement { argument } => {
						i + 1 == properties.len && matches!(self.ast.node(argument).kind, NodeKind::Identifier { .. })
					}
					_ => false,
				}
			}),
			NodeKind::ArrayExpression { elements } => (0..elements.len).all(|i| match self.ast.nth(elements, i) {
				None => true,
				Some(element) => match self.ast.node(element).kind {
					NodeKind::SpreadElement { argument } => i + 1 == elements.len && self.binding_atom_shaped(argument),
					_ => self.binding_shaped(element, false),
				},
			}),
			NodeKind::AssignmentExpression {
				operator: AssignmentOperator::Assign,
				left,
				..
			} => self.binding_atom_shaped(left),
			NodeKind::AssignmentPattern { left, .. } => !top && matches!(self.ast.node(left).kind, NodeKind::Identifier { .. }),
			_ => false,
		}
	}

	fn binding_atom_shaped(&self, id: NodeId) -> bool {
		matches!(
			self.ast.node(id).kind,
			NodeKind::Identifier { .. } | NodeKind::ObjectExpression { .. } | NodeKind::ArrayExpression { .. }
		) && self.binding_shaped(id, false)
	}

	/// The kinds of a binding-shaped expression changed to a pattern's.
	fn rebind(&mut self, id: NodeId) {
		match self.ast.node(id).kind {
			NodeKind::ObjectExpression { properties } => {
				self.ast.node_mut(id).kind = NodeKind::ObjectPattern { properties };
				for i in 0..properties.len {
					let property = self.ast.nth(properties, i).unwrap();
					self.rebind(property);
				}
			}
			NodeKind::Property { value, .. } => self.rebind(value),
			NodeKind::ArrayExpression { elements } => {
				self.ast.node_mut(id).kind = NodeKind::ArrayPattern { elements };
				for i in 0..elements.len {
					if let Some(element) = self.ast.nth(elements, i) {
						self.rebind(element);
					}
				}
			}
			NodeKind::SpreadElement { argument } => {
				self.rebind(argument);
				self.ast.node_mut(id).kind = NodeKind::RestElement { argument };
			}
			NodeKind::AssignmentExpression { left, right, .. } => {
				self.rebind(left);
				self.ast.node_mut(id).kind = NodeKind::AssignmentPattern { left, right };
			}
			_ => {}
		}
	}

	/// A list that may be an arrow's parameters, its `(` at `at` read: the arrow when it is one.
	/// `is_async`: the list follows `async`, where it may be a call's arguments.
	pub(super) fn decided_arrow(&mut self, start: u32, at: u32, is_async: bool) -> Option<NodeId> {
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		let colon = match self.list_role(at, is_async) {
			Role::Parameters => None,
			Role::Expression => return None,
			Role::Typed(colon) => Some(Some(colon)),
			Role::Unsure => Some(None),
		};
		if let Some(colon) = colon {
			let arrow = self.try_arrow_head(start, at, colon, is_async, false);
			if self.error {
				return Some(NodeId::at(0));
			}
			return arrow;
		}
		let base = self.read_arrow_params(is_async);
		if self.error {
			return Some(NodeId::at(0));
		}
		self.check_arrow_head(base, is_async, outer);
		if self.error {
			return Some(NodeId::at(0));
		}
		Some(self.arrow(start, base, is_async))
	}

	/// An arrow's head tried at `<` (`generic`) or after a list's `(` at `at`: a list whose role
	/// the type after the `:` at `colon` tells, or one its tokens do not tell.
	pub(super) fn try_arrow_head(
		&mut self,
		start: u32,
		at: u32,
		colon: Option<u32>,
		is_async: bool,
		generic: bool,
	) -> Option<NodeId> {
		let spine = TS && self.scratch.ts.state.needs_colon_at == Some(start);
		if self.lists().not_arrow.contains(&(at, spine)) {
			return None;
		}
		if TS
			&& let Some(colon) = colon
			&& !self.arrow_after_type(colon, is_async, spine)
		{
			if self.error {
				return None;
			}
			self.scratch.not_arrow.insert((at, spine));
			return None;
		}
		let before = spine.then(|| self.snapshot());
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		let mut head = self.attempt(|p| {
			if !(TS && generic) {
				return (None, p.read_arrow_params(is_async));
			}
			let type_parameters = p.parse_type_parameters(super::ts::TypeParameterModifiers::Const);
			p.expect(Kind::ParenL, true);
			(Some(type_parameters), p.read_arrow_params(is_async))
		});
		if self.error {
			return None;
		}
		if TS
			&& head.is_some()
			&& self.scratch.ts.state.arrow_return_type.is_some()
			&& let Some(before) = before
			&& !self.colon_after_body(is_async)
		{
			if self.error {
				return None;
			}
			if let Some((_, base)) = head {
				self.scratch.items.truncate(base);
			}
			self.restore(before);
			head = None;
		}
		let Some((type_parameters, base)) = head else {
			self.restore_yield_await(outer);
			self.scratch.not_arrow.insert((at, spine));
			return None;
		};
		self.check_arrow_head(base, is_async, outer);
		if self.error {
			return None;
		}
		let arrow = self.arrow(start, base, is_async);
		if self.error {
			return Some(arrow);
		}
		if type_parameters.is_some() {
			self.ast.extras.entry(arrow).type_parameters = type_parameters;
		}
		Some(arrow)
	}

	/// An arrow's parameters after their `(`, through `=>`, read as bindings: where they start
	/// among the items.
	fn read_arrow_params(&mut self, is_async: bool) -> usize {
		(self.yield_pos, self.await_pos) = (0, 0);
		if is_async {
			self.await_ident_pos = 0;
		}
		let base = self.scratch.items.len();
		let held = !matches!(self.tok.kind, Kind::ParenR | Kind::Ellipsis);
		if held {
			self.depth += 1;
			if self.depth > MAX_DEPTH {
				self.raise(self.tok.start, Code::NestingDepth);
				return base;
			}
		}
		self.modifiers_ok = false;
		self.binding_items(Kind::ParenR, false);
		if held {
			self.depth -= 1;
		}
		if self.error {
			return base;
		}
		self.close_arrow_head();
		base
	}

	/// After an arrow's parameters: with TypeScript its return type, then `=>` on the same line.
	#[inline(always)]
	fn close_arrow_head(&mut self) {
		if TS {
			self.arrow_return_type();
			if self.error {
				return;
			}
		}
		if matches!(self.tok.kind, Kind::Eof | Kind::BraceR) || self.newline() {
			self.fail();
			return;
		}
		self.expect(Kind::Arrow, true);
	}

	/// What an arrow's parameters, the items from `base` on, may not hold, checked once its head
	/// is read.
	pub(super) fn check_arrow_head(
		&mut self,
		base: usize,
		is_async: bool,
		(yield_pos, await_pos, await_ident_pos): (u32, u32, u32),
	) {
		self.check_yield_await_in_default_params();
		if self.error {
			return;
		}
		if is_async && self.await_ident_pos > 0 {
			self.raise(self.await_ident_pos, Code::AwaitAsIdentifier);
			return;
		}
		(self.yield_pos, self.await_pos) = (yield_pos, await_pos);
		if is_async {
			self.await_ident_pos = await_ident_pos;
		}
		if TS {
			for i in base..self.scratch.items.len() {
				if let Some(param) = self.scratch.items[i]
					&& self.ident_is(param, "this")
				{
					let at = self.ast.node(param).start;
					self.raise(at, Code::InvalidAssignmentTarget);
					return;
				}
			}
		}
	}

	/// The arrow function whose parameters, already patterns, are the items from `base` on; its
	/// `=>` is read.
	pub(super) fn arrow(&mut self, start: u32, base: usize, is_async: bool) -> NodeId {
		let params = self.list(base);
		let old = self.take_yield_await();
		self.enter_scope(function_flags(is_async, false) | SCOPE_ARROW);
		let frame = if TS {
			self.arrow_start(start);
			Some(self.function_start(super::ts::FunctionKind::Arrow))
		} else {
			None
		};
		let outer = self.ctx;
		let expression = self.tok.kind != Kind::BraceL;
		self.ctx = Ctx {
			function: true,
			is_async,
			generator: false,
			can_await: is_async,
			no_in: expression && outer.no_in,
			for_await: expression && outer.for_await,
		};
		let body = self.function_body(start, None, params, true, false);
		self.ctx = outer;
		self.restore_yield_await(old);
		let node = self.add(
			NodeKind::ArrowFunctionExpression {
				params,
				body,
				expression,
				is_async,
			},
			start,
		);
		if let Some(frame) = frame {
			self.function_end(node, frame);
		}
		self.bare_arrow = true;
		node
	}

	pub(super) fn take_yield_await(&mut self) -> (u32, u32, u32) {
		let old = (self.yield_pos, self.await_pos, self.await_ident_pos);
		(self.yield_pos, self.await_pos, self.await_ident_pos) = (0, 0, 0);
		old
	}

	pub(super) fn restore_yield_await(&mut self, (yield_pos, await_pos, await_ident_pos): (u32, u32, u32)) {
		(self.yield_pos, self.await_pos, self.await_ident_pos) = (yield_pos, await_pos, await_ident_pos);
	}

	// ── the role of a list, told by its tokens

	/// Whether the first token of a list, at the cursor, may start an arrow's parameters.
	#[inline(always)]
	fn may_be_parameters(&self) -> bool {
		matches!(
			self.tok.kind,
			Kind::ParenR | Kind::Ident | Kind::BraceL | Kind::BracketL | Kind::Ellipsis
		) || (TS && self.tok.kind == Kind::This)
	}

	/// How the list whose `(` is at `open` is read, its first token at the cursor; `call` when
	/// it follows `async`, where it may be arguments. The tokens are read by a lexer of their own.
	fn list_role(&mut self, open: u32, call: bool) -> Role {
		let first = self.tok;
		match first.kind {
			Kind::ParenR | Kind::Ident => {}
			Kind::This if TS => {}
			Kind::Ellipsis if !call => return Role::Parameters,
			Kind::Ellipsis | Kind::BracketL | Kind::BraceL => return self.scanned_role(open),
			_ => return Role::Expression,
		}
		let mut lexer = self.scout();
		let Some(token) = look(&mut lexer) else {
			return Role::Unsure;
		};
		if first.kind == Kind::ParenR {
			return match token.kind {
				Kind::Arrow => Role::Parameters,
				Kind::Colon if TS && call => Role::Typed(token.start),
				Kind::Colon | Kind::BraceL if TS && !call => Role::Parameters,
				_ => Role::Expression,
			};
		}
		match token.kind {
			Kind::ParenR => match look(&mut lexer) {
				Some(token) => after_closer::<TS>(token),
				None => Role::Unsure,
			},
			Kind::Comma | Kind::Eq => self.scanned_role(open),
			Kind::Colon if TS => Role::Parameters,
			Kind::Question if TS => match look(&mut lexer) {
				Some(token) => match token.kind {
					Kind::Colon | Kind::Comma | Kind::Eq | Kind::ParenR => Role::Parameters,
					_ => Role::Expression,
				},
				None => Role::Unsure,
			},
			Kind::Ident if TS && self.is_modifier_word(first) => {
				match &self.src[token.start as usize..token.end as usize] {
					"as" | "satisfies" if token.flags & ESCAPED == 0 => Role::Expression,
					_ => Role::Parameters,
				}
			}
			_ => Role::Expression,
		}
	}

	/// The tables of lists' roles and failed heads, with what a read before left in them forgotten.
	#[inline(always)]
	fn lists(&mut self) -> &mut Scratch {
		if !self.lists_forgotten {
			self.forget_lists();
		}
		self.scratch
	}

	// forgotten by `begin` for every read: test262 +0.15%, its files 15 tokens each
	#[cold]
	#[inline(never)]
	fn forget_lists(&mut self) {
		self.lists_forgotten = true;
		// a cleared table writes all of its control bytes, empty or not
		if !self.scratch.roles.is_empty() {
			self.scratch.roles.clear();
		}
		if !self.scratch.not_arrow.is_empty() {
			self.scratch.not_arrow.clear();
		}
	}

	/// A lexer of its own after the token at the cursor, reading as the parser's does: a
	/// recovering one reads past what it records.
	fn scout(&self) -> Lexer<'a> {
		let mut lexer = Lexer::new(self.src, self.module, false);
		lexer.strict = self.lexer.strict;
		lexer.at_sign = self.lexer.at_sign;
		lexer.recover = self.lexer.recover;
		lexer.set_pos(self.lexer.pos());
		lexer
	}

	fn is_modifier_word(&self, t: Token) -> bool {
		t.kind == Kind::Ident
			&& matches!(
				&self.src[t.start as usize..t.end as usize],
				"public" | "private" | "protected" | "readonly" | "static" | "abstract" | "declare" | "override" | "accessor"
			)
	}

	/// The role of the list whose `(` is at `open` that its first tokens leave open: what follows
	/// its closer, as a scan through it tells, kept for every list it closes on the way.
	#[cold]
	#[inline(never)]
	fn scanned_role(&mut self, open: u32) -> Role {
		if let Some(&role) = self.lists().roles.get(&open) {
			return role;
		}
		let mut lexer = self.scout();
		self.scan(&mut lexer, open, self.tok).unwrap_or(Role::Unsure)
	}

	/// Reads through the closer of the list whose `(` is at `open` and the token after it,
	/// keeping the role of every list closed on the way, and of that one when a read that may go
	/// back asks: no other read asks it again. None when a token cannot be read without
	/// parsing: a `/` only the parser can place, or text that does not lex.
	fn scan(&mut self, lexer: &mut Lexer<'a>, open: u32, first: Token) -> Option<Role> {
		let mut frames = std::mem::take(&mut self.scratch.frames);
		frames.clear();
		frames.push((Kind::ParenR, open, Slash::Divide));
		let mut slash = Slash::Regex;
		let mut closed: Option<u32> = None;
		let (mut prev, mut prev2) = (Kind::ParenL, Kind::Eof);
		let mut prev_word_await = false;
		let mut unread = Some(first);
		let role = loop {
			let mut token = match unread.take() {
				Some(first) => first,
				None => match look(lexer) {
					Some(t) => t,
					None => break None,
				},
			};
			if let Some(at) = closed.take() {
				let role = after_closer::<TS>(token);
				if !frames.is_empty() || self.speculating > 0 {
					self.scratch.roles.insert(at, role);
				}
				if frames.is_empty() {
					break Some(role);
				}
			}
			let mut word_await = false;
			match token.kind {
				Kind::Slash | Kind::SlashEq => match slash {
					Slash::Regex => {
						token = lexer.regex(token);
						if lexer.error.is_some() {
							break None;
						}
						slash = Slash::Divide;
					}
					Slash::Divide => slash = Slash::Regex,
					Slash::Unsure => break None,
				},
				Kind::Template => {
					if token.flags & BROKEN != 0 {
						break None;
					}
					slash = Slash::Divide;
				}
				Kind::TemplateHead => {
					frames.push((Kind::Template, token.start, Slash::Divide));
					slash = Slash::Regex;
				}
				Kind::ParenL => {
					let statement = matches!(
						prev,
						Kind::If | Kind::While | Kind::For | Kind::With | Kind::Switch | Kind::Catch
					) && !matches!(prev2, Kind::Dot | Kind::QuestionDot)
						|| (prev2 == Kind::For && prev_word_await);
					frames.push((
						Kind::ParenR,
						token.start,
						if statement { Slash::Regex } else { Slash::Divide },
					));
					slash = Slash::Regex;
				}
				Kind::BracketL => {
					frames.push((Kind::BracketR, token.start, Slash::Divide));
					slash = Slash::Regex;
				}
				Kind::BraceL => {
					frames.push((Kind::BraceR, token.start, Slash::Unsure));
					slash = Slash::Regex;
				}
				Kind::ParenR | Kind::BracketR | Kind::BraceR => {
					let Some(frame) = frames.pop() else { break None };
					if frame.0 == Kind::Template && token.kind == Kind::BraceR {
						let chunk = lexer.template_continue(token);
						if chunk.flags & BROKEN != 0 || lexer.error.is_some() {
							break None;
						}
						if chunk.kind == Kind::TemplateTail {
							slash = Slash::Divide;
						} else {
							frames.push(frame);
							slash = Slash::Regex;
						}
					} else if frame.0 != token.kind {
						break None;
					} else {
						if token.kind == Kind::ParenR {
							closed = Some(frame.1);
						}
						slash = frame.2;
					}
				}
				Kind::Eof => break None,
				kind if (kind == Kind::Ident || kind.is_keyword()) && matches!(prev, Kind::Dot | Kind::QuestionDot) => {
					slash = Slash::Divide;
				}
				Kind::Void | Kind::Const if TS => slash = Slash::Unsure,
				Kind::Ident => {
					let word = &self.src.as_bytes()[token.start as usize..token.end as usize];
					word_await = word == b"await" && token.flags & ESCAPED == 0;
					slash = match word {
						b"yield" | b"await" | b"of" | b"let" if token.flags & ESCAPED == 0 => Slash::Unsure,
						_ => Slash::Divide,
					};
				}
				Kind::PlusPlus | Kind::MinusMinus => slash = postfix(slash, token),
				Kind::Bang if TS => slash = postfix(slash, token),
				Kind::Gt | Kind::GtGt | Kind::GtGtGt if TS => slash = Slash::Unsure,
				kind => {
					let ends = matches!(
						kind,
						Kind::PrivateName
							| Kind::Number | Kind::BigInt
							| Kind::String | Kind::RegExp
							| Kind::This | Kind::Null
							| Kind::True | Kind::False
							| Kind::Super
					);
					slash = if ends { Slash::Divide } else { Slash::Regex };
				}
			}
			prev2 = prev;
			prev = token.kind;
			prev_word_await = word_await;
		};
		if role.is_none() {
			for frame in &frames {
				if frame.0 == Kind::ParenR {
					self.scratch.roles.insert(frame.1, Role::Unsure);
				}
			}
		}
		self.scratch.frames = frames;
		role
	}

	/// A call's arguments after their `(`, through the `)`: where they start in the scratch
	/// list. A literal among them that only a pattern may be is an error where they close.
	pub(super) fn arguments(&mut self) -> usize {
		let no_in = std::mem::replace(&mut self.ctx.no_in, false);
		let mut errors = Some(Facts::default());
		let base = self.arguments_inside(&mut errors);
		self.ctx.no_in = no_in;
		let held = errors.unwrap();
		if held.pending() && !self.error {
			self.raise_expression_errors(&held);
		}
		base
	}

	#[inline(always)]
	fn arguments_inside(&mut self, errors: &mut Errors) -> usize {
		let base = self.scratch.items.len();
		let mut first = true;
		while self.tok.kind != Kind::ParenR {
			if !std::mem::take(&mut first) {
				if self.tok.kind != Kind::Comma {
					self.fail();
					return base;
				}
				self.bump(true);
				if self.tok.kind == Kind::ParenR {
					break;
				}
			}
			let item = if self.tok.kind == Kind::Ellipsis {
				let at = self.tok.start;
				self.bump(true);
				let argument = self.element(errors);
				let spread = self.add(NodeKind::SpreadElement { argument }, at);
				if self.tok.kind == Kind::Comma
					&& let Some(e) = errors
					&& e.trailing_comma.is_none()
				{
					e.trailing_comma = Some(self.tok.start);
				}
				spread
			} else {
				self.element(errors)
			};
			self.scratch.items.push(Some(item));
			if self.error {
				return base;
			}
		}
		self.bump(false);
		base
	}

	pub(super) fn array(&mut self, start: u32, errors: &mut Errors) -> NodeId {
		let no_in = std::mem::replace(&mut self.ctx.no_in, false);
		let node = self.array_inside(start, errors);
		self.ctx.no_in = no_in;
		node
	}

	#[inline(always)]
	fn array_inside(&mut self, start: u32, errors: &mut Errors) -> NodeId {
		self.bump(true);
		let base = self.scratch.items.len();
		let mut first = true;
		while self.tok.kind != Kind::BracketR {
			if !std::mem::take(&mut first) {
				if self.tok.kind != Kind::Comma {
					return self.fail();
				}
				self.bump(true);
				if self.tok.kind == Kind::BracketR {
					break;
				}
			}
			let item = if self.tok.kind == Kind::Comma {
				None
			} else if self.tok.kind == Kind::Ellipsis {
				let at = self.tok.start;
				self.bump(true);
				let argument = self.element(errors);
				let spread = self.add(NodeKind::SpreadElement { argument }, at);
				if self.tok.kind == Kind::Comma
					&& let Some(e) = errors
					&& e.trailing_comma.is_none()
				{
					e.trailing_comma = Some(self.tok.start);
				}
				Some(spread)
			} else {
				Some(self.element(errors))
			};
			self.scratch.items.push(item);
			if self.error {
				return NodeId::at(0);
			}
		}
		self.bump(false);
		let elements = self.list(base);
		self.add(NodeKind::ArrayExpression { elements }, start)
	}

	pub(super) fn object(&mut self, start: u32, errors: &mut Errors) -> NodeId {
		let no_in = std::mem::replace(&mut self.ctx.no_in, false);
		let node = self.object_inside(start, errors);
		self.ctx.no_in = no_in;
		node
	}

	#[inline(always)]
	fn object_inside(&mut self, start: u32, errors: &mut Errors) -> NodeId {
		self.bump(false);
		let base = self.scratch.items.len();
		let mut first = true;
		let mut has_proto = false;
		while self.tok.kind != Kind::BraceR {
			if !std::mem::take(&mut first) {
				if self.tok.kind != Kind::Comma {
					return self.fail();
				}
				self.bump(false);
				if self.tok.kind == Kind::BraceR {
					break;
				}
			}
			let property = self.property(errors, &mut has_proto);
			self.scratch.items.push(Some(property));
			if self.error {
				return NodeId::at(0);
			}
		}
		self.bump(false);
		let properties = self.list(base);
		self.add(NodeKind::ObjectExpression { properties }, start)
	}

	/// A property's key: a name, a literal, or an expression in brackets, and whether it is one.
	pub(super) fn property_name(&mut self) -> (NodeId, bool) {
		match self.tok.kind {
			Kind::BracketL => {
				self.bump(true);
				let no_in = std::mem::replace(&mut self.ctx.no_in, false);
				let key = self.assign();
				self.ctx.no_in = no_in;
				self.expect(Kind::BracketR, false);
				(key, true)
			}
			Kind::String | Kind::Number | Kind::BigInt => (self.atom(&mut None, false), false),
			_ => (self.name(), false),
		}
	}

	/// The token at the cursor can start a property's key.
	#[inline(always)]
	pub(super) fn at_property_name(&self) -> bool {
		let kind = self.tok.kind;
		matches!(kind, Kind::Ident | Kind::Number | Kind::BigInt | Kind::String | Kind::BracketL) || kind.is_keyword()
	}

	fn property(&mut self, errors: &mut Errors, has_proto: &mut bool) -> NodeId {
		let t = self.tok;
		let start = t.start;
		if t.kind == Kind::Ellipsis {
			self.bump(true);
			let argument = self.element(errors);
			if self.tok.kind == Kind::Comma
				&& let Some(e) = errors
				&& e.trailing_comma.is_none()
			{
				e.trailing_comma = Some(self.tok.start);
			}
			return self.add(NodeKind::SpreadElement { argument }, start);
		}
		let mut generator = false;
		if t.kind == Kind::Star {
			self.bump(false);
			generator = true;
		}
		let first = self.tok;
		let escaped = first.flags & ESCAPED != 0;
		let word = if first.kind == Kind::Ident && !escaped {
			&self.src.as_bytes()[first.start as usize..first.end as usize]
		} else {
			b""
		};
		let (mut key, mut computed) = self.property_name();
		let mut is_async = false;
		if word == b"async"
			&& !generator
			&& (self.at_property_name() || self.tok.kind == Kind::Star)
			&& !self.newline()
		{
			is_async = true;
			if self.tok.kind == Kind::Star {
				self.bump(false);
				generator = true;
			}
			(key, computed) = self.property_name();
		}
		if self.error {
			return key;
		}
		let plain = !generator && !is_async;
		let (value, method, shorthand) = match self.tok.kind {
			Kind::Colon => {
				if !plain {
					return self.fail();
				}
				self.bump(true);
				let value = self.element(errors);
				if !computed && !self.error {
					self.proto_clash(key, has_proto, errors);
				}
				(value, false, false)
			}
			Kind::ParenL => (self.method(generator, is_async, false, false), true, false),
			Kind::Lt if TS => (self.method(generator, is_async, false, false), true, false),
			kind if (word == b"get" || word == b"set")
				&& !computed && !matches!(kind, Kind::Comma | Kind::BraceR | Kind::Eq) =>
			{
				if !plain {
					return self.fail();
				}
				let getter = word == b"get";
				let (key, computed) = self.property_name();
				let value = if TS {
					self.method_ts(
						false,
						false,
						false,
						false,
						if getter { crate::ast::MethodKind::Get } else { crate::ast::MethodKind::Set },
					)
				} else {
					self.method(false, false, false, false)
				};
				if TS {
					self.check_accessor_params_ts(value, getter, false);
				} else {
					self.check_accessor_params(value, getter);
				}
				return self.add(
					NodeKind::Property {
						key,
						value,
						kind: if getter { PropertyKind::Get } else { PropertyKind::Set },
						computed,
						method: false,
						shorthand: false,
					},
					start,
				);
			}
			_ if !computed && matches!(self.ast.node(key).kind, NodeKind::Identifier { .. }) => {
				if !plain {
					return self.fail();
				}
				if first.kind != Kind::Ident || self.special(first) {
					self.check_unreserved(key);
					if self.error {
						return key;
					}
				}
				let NodeKind::Identifier { name } = self.ast.node(key).kind else {
					unreachable!()
				};
				// the key's own span: under recovery it may be a name of no width
				let copy = self.ast.add(NodeKind::Identifier { name }, start, self.ast.node(key).end);
				if self.tok.kind == Kind::Eq && errors.is_some() {
					// only a pattern holds a default: what reads this object decides
					let e = errors.as_mut().unwrap();
					if e.shorthand_assign.is_none() {
						e.shorthand_assign = Some(self.tok.start);
					}
					self.bump(true);
					let right = self.assign();
					(self.add(NodeKind::AssignmentPattern { left: copy, right }, start), false, true)
				} else {
					(copy, false, true)
				}
			}
			_ => return self.fail(),
		};
		self.add(
			NodeKind::Property {
				key,
				value,
				kind: PropertyKind::Init,
				computed,
				method,
				shorthand,
			},
			start,
		)
	}

	/// A second `__proto__: value` in one literal: an error unless the literal is a pattern.
	#[inline(always)]
	fn proto_clash(&mut self, key: NodeId, has_proto: &mut bool, errors: &mut Errors) {
		let node = *self.ast.node(key);
		let name = match node.kind {
			NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } => name,
			_ => return,
		};
		if node.end - node.start < 9 || !self.ast.strings.is(name, "__proto__") {
			return;
		}
		if *has_proto {
			match errors {
				Some(e) => {
					if e.double_proto.is_none() {
						e.double_proto = Some(node.start);
					}
				}
				None => {
					self.raise(node.start, Code::DuplicateProto);
				}
			}
		}
		*has_proto = true;
	}

	/// A getter takes nothing, a setter one parameter that is not a rest.
	pub(super) fn check_accessor_params(&mut self, function: NodeId, getter: bool) {
		if self.error {
			return;
		}
		let node = *self.ast.node(function);
		let NodeKind::FunctionExpression { function } = node.kind else {
			unreachable!()
		};
		let params = function.params;
		if params.len != if getter { 0 } else { 1 } {
			self.raise(node.start, if getter { Code::GetterParams } else { Code::SetterParams });
			return;
		}
		if !getter {
			let param = *self.ast.node(self.ast.nth(params, 0).unwrap());
			if matches!(param.kind, NodeKind::RestElement { .. }) {
				self.raise(param.start, Code::SetterRestParam);
			}
		}
	}

	#[cold]
	#[inline(never)]
	fn regexp(&mut self, t: Token) -> NodeId {
		if t.flags & BROKEN != 0 {
			// under recovery, one its line ends inside: what was read of it, with no flags
			let pattern = self.ast.strings.intern(&self.src[t.start as usize + 1..t.end as usize]);
			let flags = self.ast.strings.intern("");
			self.bump(false);
			return self.ast.add(NodeKind::RegExpLiteral { pattern, flags }, t.start, t.end);
		}
		let text = &self.src[t.start as usize..t.end as usize];
		let close = text.rfind('/').unwrap();
		let (body, flags_text) = (&text[1..close], &text[close + 1..]);
		// a `(?<` outside a class that is no lookbehind names a group
		let bytes = body.as_bytes();
		let (mut named, mut in_class, mut i) = (false, false, 0);
		while i < bytes.len() {
			match bytes[i] {
				b'\\' => i += 1,
				b'[' => in_class = true,
				b']' => in_class = false,
				b'(' if !in_class && bytes[i + 1..].starts_with(b"?<") => {
					named |= !matches!(bytes.get(i + 3), Some(b'=' | b'!'));
				}
				_ => {}
			}
			i += 1;
		}
		if let Err((code, at, end)) = crate::parser::validate_regexp(body, flags_text, named, &mut self.scratch.regexp) {
			let within = |offset: usize| t.start + 1 + offset as u32;
			let error = SyntaxError::new(within(at), code).to(within(end));
			if !self.recover {
				return self.record(error);
			}
			self.lexer.soft(error, None);
		}
		let pattern = self.ast.strings.intern(body);
		let flags = self.ast.strings.intern(flags_text);
		self.bump(false);
		self.ast.add(NodeKind::RegExpLiteral { pattern, flags }, t.start, t.end)
	}

	pub(super) fn template(&mut self, tagged: bool) -> NodeId {
		let no_in = std::mem::replace(&mut self.ctx.no_in, false);
		let node = self.template_inside(tagged);
		self.ctx.no_in = no_in;
		node
	}

	fn template_inside(&mut self, tagged: bool) -> NodeId {
		if self.recover && self.tok.end == self.tok.start + 1 {
			self.whole_template();
		}
		let t = self.tok;
		let start = t.start;
		let quasis_base = self.scratch.quasis.len();
		let base = self.scratch.items.len();
		if t.kind == Kind::Template {
			let quasi = self.quasi(t, 1, true, tagged);
			self.scratch.quasis.push(Some(quasi));
			self.bump(false);
		} else {
			let quasi = self.quasi(t, 2, false, tagged);
			self.scratch.quasis.push(Some(quasi));
			loop {
				self.bump(true);
				let expression = if TS && self.in_type {
					self.parse_type()
				} else {
					self.sequence()
				};
				self.scratch.items.push(Some(expression));
				if self.tok.kind != Kind::BraceR {
					self.fail();
					break;
				}
				let chunk = self.lexer.template_continue(self.tok);
				if COUNT {
					self.tokens += 1;
				}
				self.tok = chunk;
				self.tracked_chunk(chunk);
				match chunk.kind {
					Kind::TemplateMiddle => {
						let quasi = self.quasi(chunk, 2, false, tagged);
						self.scratch.quasis.push(Some(quasi));
					}
					Kind::TemplateTail => {
						let quasi = self.quasi(chunk, 1, true, tagged);
						self.scratch.quasis.push(Some(quasi));
						self.bump(false);
						break;
					}
					_ => {
						self.fail();
						break;
					}
				}
			}
		}
		let quasis = self.ast.add_list(&self.scratch.quasis[quasis_base..]);
		self.scratch.quasis.truncate(quasis_base);
		let expressions = self.list(base);
		self.add(NodeKind::TemplateLiteral { quasis, expressions }, start)
	}

	/// `import(…)` or `import.meta`, `import` at the cursor; what `new` constructs is not a call.
	#[cold]
	#[inline(never)]
	fn import_expression(&mut self, start: u32, for_new: bool) -> NodeId {
		self.bump(false);
		match self.tok.kind {
			Kind::ParenL if !for_new => {
				self.bump(true);
				let no_in = std::mem::replace(&mut self.ctx.no_in, false);
				let source = self.assign();
				let mut options = None;
				if self.tok.kind != Kind::ParenR {
					self.expect(Kind::Comma, true);
					if self.tok.kind != Kind::ParenR {
						options = Some(self.assign());
						if self.tok.kind != Kind::ParenR {
							self.expect(Kind::Comma, true);
						}
					}
				}
				self.ctx.no_in = no_in;
				self.expect(Kind::ParenR, false);
				self.add(NodeKind::ImportExpression { source, options }, start)
			}
			Kind::Dot => {
				let name = self.ast.strings.intern("import");
				let meta = self.ast.add(NodeKind::Identifier { name }, start, start + 6);
				self.bump(false);
				let escaped = self.tok.flags & ESCAPED != 0;
				let property = self.name();
				if self.error {
					return property;
				}
				if !self.ident_is(property, "meta") {
					let at = self.ast.node(property).start;
					return self.raise(at, Code::InvalidImportMeta);
				}
				if escaped {
					return self.raise(start, Code::ImportMetaEscaped);
				}
				if !self.module {
					return self.raise(start, Code::ImportMetaOutsideModule);
				}
				self.add(NodeKind::MetaProperty { meta, property }, start)
			}
			_ => self.fail(),
		}
	}

	/// `yield` at the cursor, in a generator.
	#[cold]
	#[inline(never)]
	fn yield_expression(&mut self) -> NodeId {
		let start = self.tok.start;
		if self.yield_pos == 0 {
			self.yield_pos = start;
		}
		self.bump(true);
		let kind = self.tok.kind;
		let starts = matches!(
			kind,
			Kind::Ident
				| Kind::PrivateName
				| Kind::Number | Kind::BigInt
				| Kind::String | Kind::RegExp
				| Kind::ParenL | Kind::BracketL
				| Kind::BraceL | Kind::Template
				| Kind::TemplateHead
				| Kind::Slash | Kind::SlashEq
				| Kind::Bang | Kind::Tilde
				| Kind::Plus | Kind::Minus
				| Kind::PlusPlus
				| Kind::MinusMinus
				| Kind::This | Kind::Super
				| Kind::Function
				| Kind::Class | Kind::New
				| Kind::Typeof | Kind::Void
				| Kind::Delete | Kind::Import
				| Kind::Null | Kind::True
				| Kind::False
		);
		let (argument, delegate) = if kind == Kind::Semi
			|| kind == Kind::Eof
			|| kind == Kind::BraceR
			|| self.newline()
			|| (kind != Kind::Star && !starts)
		{
			(None, false)
		} else {
			let delegate = kind == Kind::Star;
			if delegate {
				self.bump(true);
			}
			(Some(self.assign()), delegate)
		};
		self.add(NodeKind::YieldExpression { argument, delegate }, start)
	}

	/// `new` at the cursor: what it constructs takes members but no call, then its arguments.
	#[cold]
	#[inline(never)]
	fn construct(&mut self, start: u32) -> NodeId {
		// `new` is read as a name is: spelled with an escape it is no error
		if self.tok.flags & ESCAPED != 0 {
			self.named(self.tok);
		}
		self.bump(true);
		if self.tok.kind == Kind::Dot {
			let name = self.ast.strings.intern("new");
			let meta = self.ast.add(NodeKind::Identifier { name }, start, start + 3);
			self.bump(false);
			let escaped = self.tok.flags & ESCAPED != 0;
			let property = self.name();
			if self.error {
				return property;
			}
			if !self.ident_is(property, "target") {
				let at = self.ast.node(property).start;
				return self.raise(at, Code::InvalidNewTarget);
			}
			if escaped {
				return self.raise(start, Code::NewTargetEscaped);
			}
			if !self.allow_new_target() {
				return self.raise(start, Code::NewTargetOutsideFunction);
			}
			return self.add(NodeKind::MetaProperty { meta, property }, start);
		}
		let callee_start = self.tok.start;
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			return self.raise(self.tok.start, Code::NestingDepth);
		}
		self.arrow_at = u32::MAX;
		let atom = self.atom(&mut None, true);
		self.depth -= 1;
		if self.error {
			return atom;
		}
		let callee = if std::mem::take(&mut self.bare_arrow) {
			atom
		} else {
			self.suffixes(atom, callee_start, false)
		};
		if self.error {
			return callee;
		}
		if matches!(self.ast.node(callee).kind, NodeKind::Super) {
			let at = self.ast.node(callee).start;
			return self.raise(at, Code::InvalidSuper);
		}
		let arguments = if self.tok.kind == Kind::ParenL {
			self.bump(true);
			let items = self.arguments();
			self.list(items)
		} else {
			List::EMPTY
		};
		let node = self.add(NodeKind::NewExpression { callee, arguments }, start);
		if TS {
			self.new_expression_ts(node);
		}
		node
	}
}
