//! The entries the engine and the Svelte walker call, with the host's stop tokens, the bracket
//! counts they and recovery read, and recovery itself.

use super::parse::{Parser, Scratch};
use super::{ESCAPED, Kind, Lexer, Token};
use crate::ast::{Ast, List, NodeId, NodeKind};
use crate::error::{Code, SyntaxError};
use crate::parser::{Entry, Options, Result};

/// The parser's buffers, kept by the tree between parses.
#[derive(Default)]
pub struct Spare(pub Option<Box<Scratch>>);

impl std::fmt::Debug for Spare {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Spare")
	}
}

/// `parser::parse_at` for a JavaScript entry.
pub(crate) fn parse_at(
	src: &str,
	start: u32,
	entry: Entry,
	options: Options,
	stop: &str,
	reused: Option<Box<Ast>>,
) -> (Box<Ast>, Result<(List, u32)>) {
	let end = src.len() as u32;
	let budget = if entry == Entry::Program {
		(end - start) as usize
	} else {
		0
	};
	let ast = reused.unwrap_or_else(|| Box::new(Ast::sized(budget)));
	if options.typescript() {
		read::<true>(src, start, end, entry, options, stop, ast)
	} else {
		read::<false>(src, start, end, entry, options, stop, ast)
	}
}

fn read<const TS: bool>(
	src: &str,
	start: u32,
	end: u32,
	entry: Entry,
	options: Options,
	stop: &str,
	mut ast: Box<Ast>,
) -> (Box<Ast>, Result<(List, u32)>) {
	let mut scratch = ast.spare.0.take().unwrap_or_default();
	let roots = {
		let mut parser = Parser::<false, TS>::open(src, start, options, stop, &mut ast, &mut scratch);
		let roots = parser.read_roots(entry, false).map(|roots| {
			let end = if entry == Entry::Program {
				end
			} else {
				parser.consumed_end()
			};
			(roots, end)
		});
		parser.close();
		roots
	};
	ast.errors.sort_by_key(|error| error.pos);
	ast.spare.0 = Some(scratch);
	(ast, roots)
}

/// What the Svelte walker's `js_in` does, by the new parser: the roots, where the read ended and
/// whether it ended at one of the walker's stops.
pub(crate) fn walker_entry<const TS: bool>(
	src: &str,
	at: u32,
	options: Options,
	stop: &str,
	entry: Entry,
	ast: &mut Box<Ast>,
) -> (Result<List>, u32, bool) {
	let mut scratch = ast.spare.0.take().unwrap_or_default();
	let out = {
		let mut parser = Parser::<false, TS>::open(src, at, options, stop, ast, &mut scratch);
		let roots = parser.read_roots(entry, true);
		let end = parser.consumed_end();
		let stopped = roots.is_ok() && parser.stopped();
		parser.close();
		(roots, end, stopped)
	};
	ast.spare.0 = Some(scratch);
	out
}

/// What the walker's `program_in` does: the script from `start` to the end of `src`.
pub(crate) fn walker_program<const TS: bool>(src: &str, start: u32, options: Options, ast: &mut Box<Ast>) -> Result<NodeId> {
	let mut scratch = ast.spare.0.take().unwrap_or_default();
	let out = {
		let mut parser = Parser::<false, TS>::open(src, start, options, "", ast, &mut scratch);
		parser.begin();
		let program = parser.program_root();
		parser.close();
		program
	};
	ast.spare.0 = Some(scratch);
	out
}

/// The bracket a kind opens, 1 to 3 (a template's `${` is a brace), or closes, 5 to 7.
static BRACKET: [u8; 256] = {
	let mut table = [0u8; 256];
	table[Kind::ParenL as usize] = 1;
	table[Kind::BracketL as usize] = 2;
	table[Kind::BraceL as usize] = 3;
	table[Kind::TemplateHead as usize] = 3;
	table[Kind::ParenR as usize] = 5;
	table[Kind::BracketR as usize] = 6;
	table[Kind::BraceR as usize] = 7;
	table
};

/// What the parser held when an error was met: under recovery the read goes on from there.
#[derive(Clone, Copy)]
pub(super) struct Met {
	tok: Token,
	prev_end: u32,
	yield_pos: u32,
	await_pos: u32,
	await_ident_pos: u32,
	in_type: bool,
	brackets: Brackets,
	/// Where the lexer stood: after the token at the cursor, or inside a token it gave up on.
	lexed: u32,
}

impl Default for Met {
	fn default() -> Self {
		Met {
			tok: Token {
				start: 0,
				end: 0,
				kind: Kind::Eof,
				flags: 0,
			},
			prev_end: 0,
			yield_pos: 0,
			await_pos: 0,
			await_ident_pos: 0,
			in_type: false,
			brackets: Brackets::default(),
			lexed: 0,
		}
	}
}

/// The bracket counts and what they say of the token at the cursor.
#[derive(Clone, Copy, Default)]
pub(super) struct Brackets {
	nest: u32,
	open: [u32; 3],
	unmatched: bool,
	stop_hit: bool,
}

impl<'a, const COUNT: bool, const TS: bool> Parser<'a, COUNT, TS> {
	/// A parser at `at` of `src`, writing into `ast`, with nothing read.
	pub fn open(
		src: &'a str,
		at: u32,
		options: Options,
		stop: &'a str,
		ast: &'a mut Ast,
		scratch: &'a mut Scratch,
	) -> Self {
		let module = options.has(Options::MODULE);
		let comments = std::mem::take(&mut ast.comments);
		scratch.errors.clear();
		scratch.paren_marks.clear();
		// a walker asks with a few short texts, the same one many times in a row
		if scratch.stops_of != stop {
			scratch.stops_of.clear();
			scratch.stops_of.push_str(stop);
			scratch.stop_ranges.clear();
			scratch.stop_firsts = [0; 4];
			scratch.stop_assertion = false;
			for word in stop.split_ascii_whitespace() {
				let range = stop.substr_range(word).unwrap();
				scratch.stop_ranges.push((
					range.start as u32,
					range.end as u32,
					word.starts_with(super::unicode::is_id_start),
				));
				let first = word.as_bytes()[0];
				scratch.stop_firsts[first as usize >> 6] |= 1 << (first & 63);
				scratch.stop_assertion |= word == "as" || word == "satisfies";
			}
		}
		let mut lexer = Lexer::at(src, at as usize, module, comments);
		lexer.at_sign = TS;
		let mut parser = Self::with_lexer(src, ast, scratch, module, options.has(Options::ERASE), lexer);
		parser.prev_end = at;
		parser.tok = Token {
			start: at,
			end: at,
			kind: Kind::Eof,
			flags: 0,
		};
		parser.base = at;
		parser.options = options;
		parser.recover = options.has(Options::ERROR_RECOVERY);
		parser.lexer.recover = parser.recover;
		parser.stops = stop;
		parser.track = parser.recover || !stop.is_empty();
		parser.tree_limit = 16 * src.len() + 256;
		if options.has(Options::ALLOW_AWAIT_OUTSIDE_FUNCTION) {
			parser.ctx.can_await = true;
		}
		parser
	}

	/// The first token, read before anything is parsed.
	pub fn begin(&mut self) {
		self.error = false;
		self.err = None;
		self.err_first = false;
		self.unsupported = None;
		self.scratch.items.clear();
		self.scratch.checks.clear();
		if TS {
			self.scratch.ts.clear();
			self.in_type = false;
		}
		self.bare_arrow = false;
		self.arrow_at = u32::MAX;
		let before = self.tok;
		self.tok = self.next_token(true);
		if self.track {
			// the host's token first: nothing to read, which is the host's to report
			self.tracked(before, true);
		}
	}

	/// The comments and the errors of the read, back in the tree.
	pub fn close(&mut self) {
		self.ast.comments = std::mem::take(&mut self.lexer.comments);
		self.ast.errors.append(&mut self.scratch.errors);
		if let Some(mut soft) = self.lexer.soft.take() {
			for (at, arg) in std::mem::take(&mut soft.error_args) {
				let arg = self.ast.strings.intern(&arg);
				soft.errors[at] = soft.errors[at].arg(arg);
			}
			self.ast.errors.append(&mut soft.errors);
		}
	}

	/// The error a failed strict read ends with.
	fn failure<T>(&mut self) -> Result<T> {
		if self.recover && self.error {
			// the cursor back where the error was met: what follows is skipped from there
			return Err(Box::new(self.caught()));
		}
		let error = self.take_error().expect("a failed read has its error");
		self.error = false;
		Err(Box::new(error))
	}

	/// Whether the read failed: the grammar met an error, the lexer did, or a keyword written
	/// with an escape was read as that keyword.
	fn entry_failed(&self) -> bool {
		self.error
			|| self.lexer.error.is_some()
			|| self
				.lexer
				.escaped_keyword
				.is_some_and(|(start, _)| start < self.prev_end)
	}

	/// The whole text from where the parser was opened, as a program.
	pub fn program_root(&mut self) -> Result<NodeId> {
		match self.program_from(self.base) {
			Some(program) => Ok(program),
			None => self.failure(),
		}
	}

	/// The roots of one `entry`; `walker` says the Svelte walker asks, whose placeholder under
	/// recovery spans what was read.
	pub fn read_roots(&mut self, entry: Entry, walker: bool) -> Result<List> {
		let first = self.tok.start;
		self.begin();
		if entry == Entry::Program {
			let program = self.program_root()?;
			return Ok(self.ast.add_list(&[Some(program)]));
		}
		match self.read_entry(entry) {
			// under recovery, what was read is skipped and an empty identifier stands where it failed
			Err(error) if self.recovering() => {
				let at = error.pos;
				self.record_soft(*error);
				self.skip_to_end();
				self.prev_end = self.prev_end.max(at);
				if entry == Entry::Params {
					Ok(List::EMPTY)
				} else {
					let name = self.ast.strings.intern("");
					let (start, end) = if walker {
						(first, self.consumed_end().max(first))
					} else {
						(at, at)
					};
					let placeholder = self.ast.add(NodeKind::Identifier { name }, start, end);
					Ok(self.ast.add_list(&[Some(placeholder)]))
				}
			}
			result => result,
		}
	}

	/// One entry other than a program at the token at the cursor, in a scope of its own.
	fn read_entry(&mut self, entry: Entry) -> Result<List> {
		self.enter_scope(super::checks::SCOPE_TOP);
		let root = match entry {
			Entry::Expression => {
				// only a word TypeScript reads too can make the entry be read again
				let before = (TS && self.scratch.stop_assertion).then(|| self.snapshot());
				let first = self.sequence();
				match (self.stop_word_at.take(), before) {
					// a word both the host and TypeScript read is the host's at its last use
					(Some(at), Some(before)) => {
						self.restore(before);
						self.forced_stop = Some(at);
						self.sequence()
					}
					_ => first,
				}
			}
			Entry::Pattern => self.pattern_root(),
			Entry::Params => {
				let list = self.params_root();
				if self.entry_failed() {
					return self.failure();
				}
				return Ok(list);
			}
			Entry::Statement => self.statement(0),
			Entry::TypeParameters if TS => self.parse_type_parameters(super::ts::TypeParameterModifiers::Const),
			Entry::TypeParameters => self.raise(self.tok.start, Code::NotTypescript),
			Entry::Program | Entry::StyleSheet | Entry::Svelte => unreachable!(),
		};
		if self.entry_failed() {
			return self.failure();
		}
		Ok(self.ast.add_list(&[Some(root)]))
	}

	/// An assignment target on its own: an identifier or a destructuring pattern, as it would
	/// appear on the left of `=`.
	fn pattern_root(&mut self) -> NodeId {
		let mut errors = Some(super::checks::Facts::default());
		let start = self.tok.start;
		let expression = match self.tok.kind {
			Kind::BraceL => self.object(start, &mut errors),
			Kind::BracketL => self.array(start, &mut errors),
			_ => self.ident(),
		};
		if self.error {
			return expression;
		}
		let pattern = self.make_pattern(expression, errors);
		self.check_lval_pattern(pattern, super::checks::Binding::None, false);
		if TS && !self.error && self.tok.kind == Kind::Colon {
			let annotation = self.parse_type_annotation(true, None);
			if !self.error {
				self.ast.extras.entry(pattern).type_annotation = Some(annotation);
				self.ast.node_mut(pattern).end = self.ast.node(annotation).end;
			}
		}
		pattern
	}

	/// A parenthesized parameter list on its own, read as an arrow function's would be, in the
	/// enclosing scope.
	fn params_root(&mut self) -> List {
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		// a binding follows: no regular expression starts there
		self.expect(Kind::ParenL, false);
		if self.error {
			return List::EMPTY;
		}
		let base = self.scratch.items.len();
		self.binding_items(Kind::ParenR, false);
		if self.error {
			self.scratch.items.truncate(base);
			return List::EMPTY;
		}
		self.check_arrow_head(false, outer);
		self.enter_scope(super::checks::function_flags(false, false) | super::checks::SCOPE_ARROW);
		let list = self.list(base);
		self.check_params(list, false);
		list
	}

	// ── stops and brackets

	pub(super) fn brackets(&self) -> Brackets {
		Brackets {
			nest: self.nest,
			open: self.open,
			unmatched: self.unmatched,
			stop_hit: self.stop_hit,
		}
	}

	pub(super) fn set_brackets(&mut self, b: Brackets) {
		self.nest = b.nest;
		self.open = b.open;
		self.unmatched = b.unmatched;
		self.stop_hit = b.stop_hit;
	}

	/// The token at the cursor, just read after `prev`, counted: the bracket it opens or closes,
	/// whether it is one of the host's, and the read ended there when it is and `prev` ended an
	/// operand (`first`: nothing was read before it).
	#[inline(always)]
	pub(super) fn tracked(&mut self, prev: Token, first: bool) {
		// no word with an escape before it, the tree within its limit, and no host's stop
		// starts with the token's first byte: the brackets alone, as the slow path counts them
		if !first
			&& prev.flags & ESCAPED == 0
			&& self.ast.nodes.len() <= self.tree_limit
			&& (self.stops.is_empty() || self.tok.kind == Kind::Eof || !self.starts_a_stop(self.tok))
		{
			if prev.kind == Kind::ParenL {
				self.mark_paren(prev.start);
			}
			self.unmatched = false;
			self.stop_hit = false;
			match BRACKET[self.tok.kind as usize] {
				0 => {}
				b @ 1..=3 => self.open_bracket(b as usize - 1),
				b => {
					let kind = b as usize - 5;
					self.unmatched = self.open[kind] == 0;
					if !self.unmatched {
						self.open[kind] -= 1;
						self.nest -= 1;
					}
				}
			}
			return;
		}
		self.tracked_slow(prev, first);
	}

	/// The counts after the `(` at `at`: a list read again as parameters starts from them.
	fn mark_paren(&mut self, at: u32) {
		let after = Brackets {
			unmatched: false,
			stop_hit: false,
			..self.brackets()
		};
		self.scratch.paren_marks.push((at, after));
	}

	#[inline(never)]
	fn tracked_slow(&mut self, prev: Token, first: bool) {
		if prev.kind == Kind::ParenL {
			self.mark_paren(prev.start);
		}
		// a keyword spelled with an escape, read as that keyword, is an error where it stood
		if self.recover
			&& prev.flags & ESCAPED != 0
			&& prev.kind.is_keyword()
			&& std::mem::replace(&mut self.liberal_at, u32::MAX) != prev.start
			&& !self.error
		{
			let word = self.scratch.values.word(self.src, prev.start as usize, prev.end as usize);
			let arg = self.ast.strings.intern(word);
			let error = SyntaxError::new(prev.start, Code::EscapeInKeyword).to(prev.end).arg(arg);
			let (tok, prev_end) = (self.tok, self.prev_end);
			if !self.softly(error) {
				return;
			}
			(self.tok, self.prev_end) = (tok, prev_end);
		}
		self.unmatched = false;
		let t = self.tok;
		if self.ast.nodes.len() > self.tree_limit && !self.error {
			self.raise(t.start, Code::TreeSize);
			return;
		}
		if t.kind == Kind::Eof {
			self.stop_hit = false;
			return;
		}
		let closes = match t.kind {
			Kind::ParenR => Some(0),
			Kind::BracketR => Some(1),
			Kind::BraceR => Some(2),
			_ => None,
		};
		let outside = closes.map_or(self.nest == 0, |kind| self.open[kind] == 0);
		self.stop_hit = outside && !self.stops.is_empty() && self.starts_a_stop(t) && self.stops_at(t);
		match t.kind {
			Kind::ParenL => self.open_bracket(0),
			Kind::BracketL => self.open_bracket(1),
			Kind::BraceL | Kind::TemplateHead => self.open_bracket(2),
			_ => {}
		}
		if let Some(kind) = closes {
			self.unmatched = self.open[kind] == 0;
			if !self.unmatched {
				self.open[kind] -= 1;
				self.nest -= 1;
			}
		}
		if (first || self.ends_operand(prev)) && self.stop_hit && !(TS && self.reads_stop()) {
			self.stop_here();
		}
	}

	/// A template's chunk read after the `}` that closed its expression: one that ends in `${`
	/// opens a brace as the head did.
	pub(super) fn tracked_chunk(&mut self, chunk: Token) {
		if self.track && chunk.kind == Kind::TemplateMiddle {
			self.open_bracket(2);
		}
	}

	/// The token at the cursor is given back, the one before it at the cursor again: what it
	/// opened or closed is not.
	pub(super) fn untrack(&mut self) {
		if !self.track {
			return;
		}
		match self.tok.kind {
			Kind::ParenL => self.shut(0),
			Kind::BracketL => self.shut(1),
			Kind::BraceL | Kind::TemplateHead => self.shut(2),
			Kind::ParenR if !self.unmatched => self.open_bracket(0),
			Kind::BracketR if !self.unmatched => self.open_bracket(1),
			Kind::BraceR if !self.unmatched => self.open_bracket(2),
			_ => {}
		}
		self.unmatched = false;
		self.stop_hit = false;
	}

	fn open_bracket(&mut self, kind: usize) {
		self.open[kind] += 1;
		self.nest += 1;
	}

	fn shut(&mut self, kind: usize) {
		self.open[kind] -= 1;
		self.nest -= 1;
	}

	/// Whether an expression could end after `t`: it is an operand or closes one.
	fn ends_operand(&mut self, t: Token) -> bool {
		match t.kind {
			// a keyword written with an escape is that keyword here
			Kind::Ident if t.flags & ESCAPED != 0 => {
				let word = self.scratch.values.word(self.src, t.start as usize, t.end as usize);
				matches!(
					super::keyword_spelled(word.as_bytes()),
					Kind::Ident | Kind::This | Kind::Null | Kind::True | Kind::False | Kind::Super
				)
			}
			Kind::Ident | Kind::PrivateName | Kind::Number | Kind::BigInt | Kind::String | Kind::RegExp => true,
			Kind::ParenR | Kind::BracketR | Kind::BraceR | Kind::PlusPlus | Kind::MinusMinus => true,
			Kind::This | Kind::Null | Kind::True | Kind::False | Kind::Super => true,
			_ => false,
		}
	}

	/// Whether a stop starts with the byte `t` starts with: most tokens start with none.
	#[inline(always)]
	fn starts_a_stop(&self, t: Token) -> bool {
		let first = self.src.as_bytes()[t.start as usize];
		self.scratch.stop_firsts[first as usize >> 6] >> (first & 63) & 1 != 0
	}

	/// A word stop is the token's text; a punctuation stop is what the source continues with.
	#[inline(never)]
	fn stops_at(&self, t: Token) -> bool {
		let word = (t.kind == Kind::Ident || t.kind.is_keyword()) && t.flags & ESCAPED == 0;
		let literal = matches!(
			t.kind,
			Kind::Ident | Kind::PrivateName | Kind::Number | Kind::BigInt | Kind::String | Kind::RegExp
		) || t.kind.is_keyword();
		let text = &self.src[t.start as usize..t.end as usize];
		let rest = &self.src[t.start as usize..];
		self.scratch.stop_ranges.iter().any(|&(from, to, is_word)| {
			let stop = &self.stops[from as usize..to as usize];
			if is_word {
				word && text == stop
			} else {
				!literal && rest.starts_with(stop)
			}
		})
	}

	/// The token at the cursor was read again: whether it is the host's, as the counts stand.
	pub(super) fn restop(&mut self) {
		let t = self.tok;
		self.stop_hit =
			t.kind != Kind::Eof && self.nest == 0 && !self.stops.is_empty() && self.starts_a_stop(t) && self.stops_at(t);
	}

	/// Whether the token at the cursor, one of the host's stop tokens, is one TypeScript reads
	/// after an expression; it then decides where the host's syntax starts.
	pub(super) fn reads_stop(&self) -> bool {
		(self.is_word(b"as") || self.is_word(b"satisfies")) && self.forced_stop != Some(self.tok.start)
	}

	/// Ends the read when the token at the cursor is the host's.
	pub(super) fn stop_after_operand(&mut self) {
		if self.stop_hit && !(TS && self.reads_stop()) {
			self.stop_here();
		}
	}

	/// Ends the read at the token at the cursor: the host's own syntax starts there.
	pub(super) fn stop_here(&mut self) {
		let at = self.tok.start;
		self.lexer.rewind(at as usize);
		self.tok = Token {
			start: at,
			end: at,
			kind: Kind::Eof,
			flags: 0,
		};
	}

	/// Whether the read ended before the source did, at the host's own syntax.
	pub fn stopped(&self) -> bool {
		self.tok.kind == Kind::Eof && (self.tok.start as usize) < self.src.len()
	}

	/// The offset after the last token read, or after the comments that follow it before the
	/// next token: where a host embedding JavaScript resumes its own syntax.
	pub fn consumed_end(&self) -> u32 {
		match self.lexer.comments.last() {
			Some(comment) if comment.start >= self.prev_end => comment.end,
			_ => self.prev_end,
		}
	}

	// ── recovery

	pub(super) fn recovering(&self) -> bool {
		self.recover && self.speculating == 0
	}

	/// An error recorded under recovery, once per position and code.
	pub(super) fn record_soft(&mut self, error: SyntaxError) {
		if self
			.scratch
			.errors
			.last()
			.is_none_or(|last| (last.pos, last.code) != (error.pos, error.code))
		{
			self.scratch.errors.push(error);
		}
	}

	/// The cursor as it stands, kept for the recovery that will read on from it.
	pub(super) fn meet(&mut self) {
		self.scratch.met = Met {
			tok: self.tok,
			prev_end: self.prev_end,
			yield_pos: self.yield_pos,
			await_pos: self.await_pos,
			await_ident_pos: self.await_ident_pos,
			in_type: self.in_type,
			brackets: self.brackets(),
			lexed: self.lexer.pos() as u32,
		};
	}

	/// The error the read is unwinding from, taken: the cursor is back where it was met.
	fn caught(&mut self) -> SyntaxError {
		let error = self.err.take().expect("an error was met");
		self.error = false;
		self.err_first = false;
		let met = self.scratch.met;
		self.tok = met.tok;
		self.prev_end = met.prev_end;
		// what is read next starts where the lexer stood, and the token at the cursor ends there
		self.lexer.rewind(met.lexed as usize);
		if self.tok.kind != Kind::Eof {
			self.tok.end = met.lexed;
		}
		self.raw_backquote();
		(self.yield_pos, self.await_pos, self.await_ident_pos) = (met.yield_pos, met.await_pos, met.await_ident_pos);
		self.in_type = met.in_type;
		self.set_brackets(met.brackets);
		error
	}

	/// `error` ending as every error does: over the token at the cursor when it starts there.
	pub(super) fn ended(&self, error: SyntaxError) -> SyntaxError {
		let end = if error.pos == self.tok.start {
			self.tok_end()
		} else {
			error.pos
		};
		error.to(end)
	}

	pub(super) fn error_at(&self, pos: u32, code: Code) -> SyntaxError {
		self.ended(SyntaxError::new(pos, code))
	}

	pub(super) fn error_arg(&mut self, pos: u32, code: Code, arg: &str) -> SyntaxError {
		let arg = self.ast.strings.intern(arg);
		self.ended(SyntaxError::new(pos, code).arg(arg))
	}

	/// An error the read goes on past under recovery: whether it did. Otherwise it is the
	/// parse's, and the caller returns.
	pub(super) fn softly(&mut self, error: SyntaxError) -> bool {
		if self.error {
			return false;
		}
		if !self.recovering() {
			self.record(error);
			return false;
		}
		self.record_soft(error);
		true
	}

	/// The template at the cursor, which the grammar does not take as one, is its backquote
	/// alone: the parser in use reads a template only where one is read, and what stands in
	/// it as tokens otherwise.
	pub(super) fn raw_backquote(&mut self) {
		let t = self.tok;
		if !matches!(t.kind, Kind::Template | Kind::TemplateHead) || t.end == t.start + 1 {
			return;
		}
		if t.kind == Kind::TemplateHead && self.track && self.open[2] > 0 {
			self.shut(2);
		}
		self.lexer.rewind(t.start as usize + 1);
		self.tok = Token {
			start: t.start,
			end: t.start + 1,
			kind: Kind::Template,
			flags: t.flags & super::NEWLINE_BEFORE,
		};
	}

	/// A backquote read alone, where a template is read after all: the template from it.
	pub(super) fn whole_template(&mut self) {
		let t = self.tok;
		self.lexer.rewind(t.start as usize);
		let mut token = self.lexer.next(false);
		token.flags |= t.flags & super::NEWLINE_BEFORE;
		self.tok = token;
		if token.kind == Kind::TemplateHead && self.track {
			self.open_bracket(2);
		}
	}

	/// What `fail` would raise, recorded under recovery.
	pub(super) fn soft_unexpected(&mut self) -> bool {
		if self.error {
			return false;
		}
		if !self.recovering() {
			self.fail();
			return false;
		}
		let code = if self.tok.kind == Kind::Eof && !self.stopped() {
			Code::UnexpectedEof
		} else {
			Code::UnexpectedToken
		};
		let error = self.error_at(self.tok.start, code);
		self.record_soft(error);
		self.raw_backquote();
		true
	}

	/// Where the grammar needs a token it does not have: under recovery an empty identifier of
	/// no width there.
	#[cold]
	#[inline(never)]
	pub(super) fn placeholder(&mut self) -> NodeId {
		if self.error {
			return NodeId::at(0);
		}
		if self.ast.nodes.len() > self.tree_limit {
			return self.raise(self.tok.start, Code::TreeSize);
		}
		if !self.soft_unexpected() {
			return NodeId::at(0);
		}
		let name = self.ast.strings.intern("");
		let at = self.tok.start;
		self.prev_end = at;
		self.ast.add(NodeKind::Identifier { name }, at, at)
	}

	/// An element of a list that consumed nothing under recovery would repeat forever: its token
	/// is skipped as unexpected. False when the read cannot go on.
	pub(super) fn ensure_progress(&mut self, at: u32) -> bool {
		if self.error {
			return false;
		}
		if self.recovering() && self.tok.start == at {
			if self.tok.kind == Kind::Eof {
				self.fail();
				return false;
			}
			self.soft_unexpected();
			self.bump(true);
		}
		!self.error
	}

	/// A statement of a list, under recovery: when it fails, what was read is skipped up to the
	/// next `;`, a line that starts with a keyword, or the bracket that closes the list.
	pub(super) fn statement_recovered(&mut self, place: u8) -> Option<NodeId> {
		if !self.recovering() {
			let statement = self.statement(place);
			return (!self.error).then_some(statement);
		}
		let mark = self.kept();
		let at = self.tok.start;
		let exported = self.scratch.checks.exports.len();
		let statement = self.statement(place);
		if !self.error {
			return Some(statement);
		}
		let error = self.caught();
		self.record_soft(error);
		self.unwind(mark);
		if place == 0 {
			self.scratch.checks.exports.truncate(exported);
		}
		let mut skipped = self.tok.start != at;
		loop {
			if matches!(self.tok.kind, Kind::Eof | Kind::BraceR) {
				break;
			}
			if self.tok.kind == Kind::Semi {
				self.bump(true);
				break;
			}
			if skipped && self.newline() && self.tok.kind.is_keyword() {
				break;
			}
			self.bump_raw();
			if self.error {
				break;
			}
			skipped = true;
		}
		None
	}

	/// The statements up to `close`, pushed, each recovered from when it fails.
	#[cold]
	#[inline(never)]
	pub(super) fn statements_recovering(&mut self, close: Kind, place: u8) {
		while self.tok.kind != close {
			if close == Kind::Eof && self.recovering() && self.unmatched {
				self.soft_unexpected();
				self.bump(true);
				continue;
			}
			let at = self.tok.start;
			if let Some(statement) = self.statement_recovered(place) {
				self.scratch.items.push(Some(statement));
			}
			if self.error || !self.ensure_progress(at) {
				return;
			}
		}
	}

	/// A program's body or a function's, read as the parser in use reads one under recovery:
	/// the directive looked for ahead, then each statement recovered from.
	#[cold]
	#[inline(never)]
	pub(super) fn body_recovering(&mut self, close: Kind, place: u8, head: Option<super::stmt::Head>) {
		let base = self.scratch.items.len();
		match head {
			Some(head) => {
				let use_strict = (!head.old_strict || !head.simple) && self.strict_directive();
				if self.error {
					return;
				}
				self.check_head(head, use_strict);
				if self.error {
					return;
				}
			}
			None => {
				if !self.strict {
					self.strict_directive();
					if self.error {
						return;
					}
				}
			}
		}
		self.statements_recovering(close, place);
		if self.error {
			return;
		}
		self.adapt_directive_prologue(base);
	}

	/// Whether the body at the cursor opens with directives that hold `"use strict"`: read ahead
	/// and put back, and the body strict from its first token when it does.
	fn strict_directive(&mut self) -> bool {
		if self.tok.kind != Kind::String {
			return false;
		}
		let snapshot = self.snapshot();
		self.speculating += 1;
		let mut found = false;
		while self.tok.kind == Kind::String {
			let (start, end) = (self.tok.start, self.tok.end);
			let statement = self.statement(1);
			if self.error {
				break;
			}
			let NodeKind::ExpressionStatement { expression, .. } = self.ast.node(statement).kind else {
				break;
			};
			let node = *self.ast.node(expression);
			if !matches!(node.kind, NodeKind::StringLiteral { .. }) || node.end != end {
				break;
			}
			// under recovery the string may be a lone quote, its range empty backwards
			if self.src.get(start as usize + 1..end as usize - 1) == Some("use strict") {
				found = true;
				break;
			}
		}
		self.speculating -= 1;
		self.restore(snapshot);
		if found {
			self.set_strict(true);
			// the first string was read as sloppy code, legacy octal escapes allowed
			let newline = self.tok.flags & super::NEWLINE_BEFORE;
			self.lexer.rewind(self.tok.start as usize);
			let mut token = self.next_token(false);
			token.flags |= newline;
			self.tok = token;
		}
		found
	}

	/// The string statements a body opens with, from `base` of the scratch list, are directives.
	fn adapt_directive_prologue(&mut self, base: usize) {
		for i in base..self.scratch.items.len() {
			let statement = self.scratch.items[i].unwrap();
			let NodeKind::ExpressionStatement {
				expression,
				directive: None,
			} = self.ast.node(statement).kind
			else {
				break;
			};
			let bytes = self.src.as_bytes();
			let quoted = matches!(bytes.get(self.ast.node(statement).start as usize), Some(b'"' | b'\''));
			if !quoted || !matches!(self.ast.node(expression).kind, NodeKind::StringLiteral { .. }) {
				break;
			}
			let (start, end) = (self.ast.node(expression).start, self.ast.node(expression).end);
			// under recovery the string may end with the input, its quote never closed
			if end < start + 2 || bytes[end as usize - 1] != bytes[start as usize] {
				break;
			}
			let raw = &self.src[start as usize + 1..end as usize - 1];
			let directive = self.ast.strings.intern(raw);
			self.ast.node_mut(statement).kind = NodeKind::ExpressionStatement {
				expression,
				directive: Some(directive),
			};
		}
	}

	/// Skips to the end of the input, a stop token or a bracket nothing here opened.
	pub(super) fn skip_to_end(&mut self) {
		while self.tok.kind != Kind::Eof && !self.unmatched {
			self.bump_raw();
			if self.error {
				break;
			}
		}
	}
}
