//! The JavaScript and TypeScript parser. This file is its lexer, whose token is a span, a kind and
//! flags: no text is kept, nothing is interned, and an error ends the input instead of being
//! returned.

pub(crate) mod checks;
pub(crate) mod entry;
pub(crate) mod parse;
pub(crate) mod regexp;
pub(crate) mod scan;
pub(crate) mod stmt;
pub(crate) mod ts;
pub(crate) mod unicode;
pub(crate) mod values;
#[cfg(test)]
mod lexer_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod ts_tests;

use crate::ast::{Ast, List};
use crate::error::SyntaxError;

/// Errors travel boxed so every `Result` stays two words wide.
pub(crate) type Result<T, E = Box<SyntaxError>> = std::result::Result<T, E>;

pub(crate) use parse::MAX_DEPTH;

/// The switches of a parse as one word, two bits per option in the order of npm/src/options.ts,
/// holding the index of the option's value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options(pub u32);

impl Options {
	const fn at(slot: u32, index: u32) -> u32 {
		index << (2 * slot)
	}
	/// An ES module: strict mode, top-level `await`, `import` and `export`.
	pub const MODULE: u32 = Self::at(0, 1);
	pub const TYPESCRIPT: u32 = Self::at(1, 1);
	/// TypeScript, erased on output: what erasure cannot express is listed as `typescript`.
	pub const ERASE: u32 = Self::at(1, 2);
	pub const COMMENTS: u32 = Self::at(2, 1);
	/// Scope analysis on the answer.
	pub const SCOPES: u32 = Self::at(3, 1);
	/// The fact `parenthesized` on a node the source wraps in parens, instead of a wrapper node.
	pub const PARENTHESIZED: u32 = Self::at(4, 1);
	pub const ALLOW_RETURN_OUTSIDE_FUNCTION: u32 = Self::at(5, 1);
	pub const ALLOW_AWAIT_OUTSIDE_FUNCTION: u32 = Self::at(6, 1);
	pub const ALLOW_SUPER_OUTSIDE_METHOD: u32 = Self::at(7, 1);
	pub const ALLOW_UNDECLARED_EXPORTS: u32 = Self::at(8, 1);
	/// Errors are recorded on the tree instead of ending the parse: a missing operand, name or
	/// pattern is an empty `Identifier` of no width where it was expected, and a statement or
	/// entry that cannot be read is skipped to the next stop token or unmatched closer, an empty
	/// `Identifier` standing for it.
	pub const ERROR_RECOVERY: u32 = Self::at(9, 1);

	/// Whether any of `bits` is on.
	pub const fn has(self, bits: u32) -> bool {
		self.0 & bits != 0
	}
	pub const fn with(self, bits: u32) -> Options {
		Options(self.0 | bits)
	}
	pub const fn without(self, bits: u32) -> Options {
		Options(self.0 & !bits)
	}
	/// On for `typescript: true` and `'erase'` alike.
	pub const fn typescript(self) -> bool {
		self.has(Self::TYPESCRIPT | Self::ERASE)
	}
}

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
	/// A CSS stylesheet on its own, the whole source.
	StyleSheet,
	/// A Svelte component, the whole source.
	Svelte,
}

/// Parses one `entry` starting at `start` of a source, with positions of the whole: a program, a
/// stylesheet, or what a host embedding JavaScript in a larger syntax reads at a point of it. A
/// program reads to the end, where a hashbang or an HTML comment at `start` is what it would be in
/// the middle of a file; anything else stops where its grammar ends, or at one of `stop`, the
/// host's own tokens separated by spaces, read outside every bracket the parse opened where the
/// expression could end (TypeScript may read one as its own, see `Parser::reads_stop`). Returns
/// the tree, its roots (one node, or the patterns of a parameter list) and the offset after
/// everything the parse consumed. `reused` is an emptied tree from an earlier parse, its room kept.
pub(crate) fn parse_at(
	src: &str,
	start: u32,
	entry: Entry,
	options: Options,
	stop: &str,
	reused: Option<Box<Ast>>,
) -> (Box<Ast>, Result<(List, u32)>) {
	if entry == Entry::StyleSheet {
		let (mut ast, root) = crate::svelte::parse_stylesheet(src, options, reused);
		let parsed = root.map(|root| (ast.add_list(&[Some(root)]), src.len() as u32));
		return (ast, parsed);
	}
	if entry == Entry::Svelte {
		let (mut ast, root) = crate::svelte::parse_component(src, options, reused);
		let parsed = root.map(|root| (ast.add_list(&[Some(root)]), src.len() as u32));
		return (ast, parsed);
	}
	entry::parse_at(src, start, entry, options, stop, reused)
}

/// The words that are more than a name somewhere: their facts, as bits.
pub(crate) mod word {
	pub(crate) const YIELD: u8 = 1;
	pub(crate) const AWAIT: u8 = 2;
	pub(crate) const ARGUMENTS: u8 = 4;
	pub(crate) const ENUM: u8 = 8;
	/// Reserved in strict mode.
	pub(crate) const STRICT: u8 = 16;
	/// A keyword.
	pub(crate) const KEYWORD: u8 = 32;
	pub(crate) const EVAL: u8 = 64;

	pub(crate) fn flags(word: &str) -> u8 {
		let mut flags = match word.as_bytes() {
			b"yield" => YIELD | STRICT,
			b"await" => AWAIT,
			b"arguments" => ARGUMENTS,
			b"eval" => EVAL,
			b"enum" => ENUM,
			b"implements" | b"interface" | b"let" | b"package" | b"private" | b"protected" | b"public" | b"static" => {
				STRICT
			}
			_ => 0,
		};
		if super::keyword_spelled(word.as_bytes()) != super::Kind::Ident {
			flags |= KEYWORD;
		}
		flags
	}
}

/// A regular expression's pattern and flags checked: the error, where it starts and ends in the
/// pattern.
pub(crate) fn validate_regexp(
	pattern: &str,
	flags: &str,
	named: bool,
	scratch: &mut regexp::Scratch,
) -> std::result::Result<(), (crate::error::Code, usize, usize)> {
	regexp::validate(pattern, flags, named, scratch).map_err(|regexp::Rejected { code, at, end }| (code, at, end))
}

use scan::{CLASS, DIGIT, ID_CONTINUE, ID_START, NEWLINE, REGEXP, SPACE, find, is_separator, line_end};
use unicode::{is_id_continue, is_id_start};

/// A line break stands between the token before and this one.
pub const NEWLINE_BEFORE: u8 = 1;
/// A word written with a unicode escape, or a string or template chunk whose value is not its
/// source text: whoever builds the node decodes it.
pub const ESCAPED: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
	pub start: u32,
	pub end: u32,
	pub kind: Kind,
	pub flags: u8,
}

#[rustfmt::skip]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Kind {
	Eof, Ident, PrivateName, Number, BigInt, String, RegExp,
	/// A whole template, a head up to its first `${`, a middle from `}` to `${`, a tail from `}`.
	Template, TemplateHead, TemplateMiddle, TemplateTail,
	Break, Case, Catch, Class, Const, Continue, Debugger, Default, Delete, Do, Else, Export,
	Extends, False, Finally, For, Function, If, Import, In, Instanceof, New, Null, Return, Super,
	Switch, This, Throw, True, Try, Typeof, Var, Void, While, With,
	BraceL, BraceR, ParenL, ParenR, BracketL, BracketR, Semi, Comma, Colon, Tilde, Dot, Ellipsis,
	Question, QuestionQuestion, QuestionQuestionEq, QuestionDot, Eq, EqEq, EqEqEq, Arrow, Bang,
	BangEq, BangEqEq, At, Lt, LtLt, LtLtEq, LtEq, Gt, GtGt, GtGtGt, GtGtEq, GtGtGtEq, GtEq, Plus,
	PlusPlus, PlusEq, Minus, MinusMinus, MinusEq, Star, StarStar, StarStarEq, StarEq, Slash,
	SlashEq, Percent, PercentEq, Amp, AmpAmp, AmpAmpEq, AmpEq, Pipe, PipePipe, PipePipeEq, PipeEq,
	Caret, CaretEq,
}

impl Kind {
	pub fn is_keyword(self) -> bool {
		(Kind::Break as u8..=Kind::With as u8).contains(&(self as u8))
	}
}

pub use crate::error::Code;

/// What an error is about, kept until the tree's strings can hold it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg {
	None,
	Char(char),
	Radix(u32),
}

/// The first error of the lexer: the input ends there.
#[derive(Clone, Copy, Debug)]
pub struct LexError {
	pub code: Code,
	pub pos: u32,
	pub arg: Arg,
}

/// The token is a template the text ends inside: an error once it is read as one.
pub const BROKEN: u8 = 4;

/// Under recovery, no token: the one that did not lex was recorded and skipped, the cursor
/// after it.
const SKIPPED: u8 = 8;

/// Asked of `rest` while tokens that do not lex are skipped: the next skip is the caller's.
const SKIPPING: u8 = 16;

pub use crate::ast::{Comment, CommentKind};

// what a token's first byte starts
#[derive(Clone, Copy)]
#[repr(u8)]
enum Lead {
	Bad,
	Space,
	Newline,
	Word,
	Digit,
	Quote,
	Backquote,
	Single,
	Dot,
	Question,
	Eq,
	Bang,
	Lt,
	Gt,
	Plus,
	Minus,
	Star,
	Slash,
	Percent,
	Amp,
	Pipe,
	Caret,
	Hash,
	At,
	Backslash,
	High,
}

static LEAD: [Lead; 256] = leads();
static SINGLE: [Kind; 256] = singles();

const fn leads() -> [Lead; 256] {
	let mut table = [Lead::Bad; 256];
	let mut b = 0;
	while b < 256 {
		let c = b as u8;
		table[b] = match c {
			b' ' | b'\t' | 0x0b | 0x0c => Lead::Space,
			b'\n' | b'\r' => Lead::Newline,
			b'a'..=b'z' | b'A'..=b'Z' | b'$' | b'_' => Lead::Word,
			b'0'..=b'9' => Lead::Digit,
			b'"' | b'\'' => Lead::Quote,
			b'`' => Lead::Backquote,
			b'{' | b'}' | b'(' | b')' | b'[' | b']' | b';' | b',' | b':' | b'~' => Lead::Single,
			b'.' => Lead::Dot,
			b'?' => Lead::Question,
			b'=' => Lead::Eq,
			b'!' => Lead::Bang,
			b'<' => Lead::Lt,
			b'>' => Lead::Gt,
			b'+' => Lead::Plus,
			b'-' => Lead::Minus,
			b'*' => Lead::Star,
			b'/' => Lead::Slash,
			b'%' => Lead::Percent,
			b'&' => Lead::Amp,
			b'|' => Lead::Pipe,
			b'^' => Lead::Caret,
			b'#' => Lead::Hash,
			b'@' => Lead::At,
			b'\\' => Lead::Backslash,
			0x80..=0xff => Lead::High,
			_ => Lead::Bad,
		};
		b += 1;
	}
	table
}

const fn singles() -> [Kind; 256] {
	let mut table = [Kind::Eof; 256];
	table[b'{' as usize] = Kind::BraceL;
	table[b'}' as usize] = Kind::BraceR;
	table[b'(' as usize] = Kind::ParenL;
	table[b')' as usize] = Kind::ParenR;
	table[b'[' as usize] = Kind::BracketL;
	table[b']' as usize] = Kind::BracketR;
	table[b';' as usize] = Kind::Semi;
	table[b',' as usize] = Kind::Comma;
	table[b':' as usize] = Kind::Colon;
	table[b'~' as usize] = Kind::Tilde;
	table
}

pub struct Lexer<'a> {
	src: &'a str,
	pos: usize,
	/// The first error and where; the input ends there, so every later token is `Eof`.
	pub error: Option<LexError>,
	/// Strict code: a legacy octal number or escape is an error.
	pub strict: bool,
	/// Where the first keyword written with an escape stands, until it is read as a name.
	pub escaped_keyword: Option<(u32, u32)>,
	pub comments: Vec<Comment>,
	keep_comments: bool,
	/// Modules have no HTML-style comments.
	module: bool,
	/// `@` is a token: a decorator's sign, in TypeScript.
	pub at_sign: bool,
	/// Errors are recorded and reading goes on past them.
	pub recover: bool,
	/// What recovery recorded: none until it records something, so a strict lexer stays small.
	pub soft: Option<Box<Soft>>,
}

/// The errors a lexer records under recovery.
#[derive(Default)]
pub struct Soft {
	pub errors: Vec<crate::error::SyntaxError>,
	/// What each recorded error is about, by its place in `errors`, until the tree's strings hold it.
	pub error_args: Vec<(usize, String)>,
	/// Where the read of a token that failed for good stopped.
	pub fail_pos: usize,
}

impl<'a> Lexer<'a> {
	pub fn new(src: &'a str, module: bool, keep_comments: bool) -> Self {
		let mut lexer = Lexer {
			src,
			pos: 0,
			error: None,
			strict: module,
			escaped_keyword: None,
			comments: Vec::new(),
			keep_comments,
			module,
			at_sign: false,
			recover: false,
			soft: None,
		};
		lexer.skip_hashbang();
		lexer
	}

	/// A lexer at `pos` of a text, with the comments read so far: a hashbang is one only where
	/// the text starts.
	pub fn at(src: &'a str, pos: usize, module: bool, comments: Vec<Comment>) -> Self {
		let mut lexer = Lexer {
			src,
			pos,
			error: None,
			strict: module,
			escaped_keyword: None,
			comments,
			keep_comments: true,
			module,
			at_sign: false,
			recover: false,
			soft: None,
		};
		lexer.skip_hashbang();
		lexer
	}

	fn skip_hashbang(&mut self) {
		if self.pos == 0 && self.src.starts_with("#!") {
			let end = 2 + line_end(&self.src.as_bytes()[2..]);
			self.comment(0, end, CommentKind::Hashbang);
			self.pos = end;
		}
	}

	pub fn pos(&self) -> usize {
		self.pos
	}

	/// The input ended where it stands: every later token is `Eof`.
	pub fn end(&mut self) -> Token {
		let end = self.src.len();
		self.pos = end;
		token(end, end, Kind::Eof, 0)
	}

	/// The cursor moved to `pos`, with nothing read: one lexer reads every expression of a text.
	pub fn set_pos(&mut self, pos: usize) {
		self.pos = pos;
		self.error = None;
		self.escaped_keyword = None;
	}

	/// The cursor back at `pos`, where it stood before. An error met since stays: the same
	/// text is read again, or the look ahead was the first to read it.
	pub fn rewind(&mut self, pos: usize) {
		self.pos = pos;
	}

	/// The byte the next token starts with, past spaces and comments, nothing read.
	pub fn peek_byte(&self) -> Option<u8> {
		self.peek().map(|(b, _)| b)
	}

	/// After a `let`, whether what follows makes it a declaration: a bracket, or a name other
	/// than `in` and `instanceof`; `nested` where only `let [` is told apart from a name.
	pub fn declares_after_let(&self, nested: bool) -> bool {
		let Some((b, pos)) = self.peek() else { return false };
		if b == b'[' || b == b'\\' {
			return true;
		}
		if nested {
			return false;
		}
		if b == b'{' {
			return true;
		}
		// a name of ASCII alone, the usual one, by its bytes
		let bytes = self.src.as_bytes();
		if CLASS[b as usize] & ID_START != 0 {
			let end = ident_end(bytes, pos + 1);
			match bytes.get(end) {
				Some(b'\\') => return true,
				Some(&next) if next >= 0x80 => {}
				_ => {
					let word = &bytes[pos..end];
					return word != b"in" && word != b"instanceof";
				}
			}
		}
		let rest = &self.src[pos..];
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

	fn peek(&self) -> Option<(u8, usize)> {
		let bytes = self.src.as_bytes();
		let mut pos = self.pos;
		let mut broke = pos == 0;
		loop {
			match bytes.get(pos)? {
				b'\n' | b'\r' => {
					broke = true;
					pos += 1;
				}
				b' ' | b'\t' | 0x0b | 0x0c => pos += 1,
				b'/' if bytes.get(pos + 1) == Some(&b'/') => pos += 2 + line_end(&bytes[pos + 2..]),
				b'<' if !self.module && bytes[pos..].starts_with(b"<!--") => pos += 4 + line_end(&bytes[pos + 4..]),
				b'-' if !self.module && broke && bytes[pos..].starts_with(b"-->") => {
					pos += 3 + line_end(&bytes[pos + 3..]);
				}
				b'/' if bytes.get(pos + 1) == Some(&b'*') => {
					let mut at = pos + 2;
					loop {
						at = find(bytes, at, [b'*'], false);
						if at >= bytes.len() {
							return None;
						}
						if bytes.get(at + 1) == Some(&b'/') {
							break;
						}
						at += 1;
					}
					let body = &bytes[pos + 2..at];
					broke |= line_end(body) < body.len();
					pos = at + 2;
				}
				&b if b >= 0x80 => {
					let c = self.src[pos..].chars().next().unwrap();
					if scan::is_new_line(c) {
						broke = true;
					} else if !scan::is_whitespace(c) {
						return Some((b, pos));
					}
					pos += c.len_utf8();
				}
				&b => return Some((b, pos)),
			}
		}
	}

	/// The token at the cursor. `regex` says a `/` here starts a regular expression: what stands
	/// before it is the caller's knowledge.
	// spaces and line breaks, then a word or a one-byte punctuator: four tokens in five, read here
	// so the caller's loop holds them without a call; the rest is `rest`
	#[inline(always)]
	pub fn next(&mut self, regex: bool) -> Token {
		let bytes = self.src.as_bytes();
		let mut pos = self.pos;
		let mut flags = 0u8;
		let (b, class) = loop {
			let Some(&b) = bytes.get(pos) else {
				return unpack(self.rest(pos, flags, regex));
			};
			let class = CLASS[b as usize];
			if class & (SPACE | NEWLINE) == 0 {
				break (b, class);
			}
			flags |= (class & NEWLINE) >> 1;
			pos += 1;
		};
		if class & ID_START != 0 {
			let end = ident_end(bytes, pos + 1);
			if bytes.get(end).is_some_and(|&b| b == b'\\' || b >= 0x80) {
				return unpack(self.word_slow_pair(pos, end, flags, regex));
			}
			self.pos = end;
			return token(pos, end, keyword_spelled(&bytes[pos..end]), flags);
		}
		let kind = SINGLE[b as usize];
		if kind != Kind::Eof {
			self.pos = pos + 1;
			return token(pos, pos + 1, kind, flags);
		}
		let after = bytes.get(pos + 1).copied().unwrap_or(0);
		if b == b'=' && after != b'=' && after != b'>' {
			self.pos = pos + 1;
			return token(pos, pos + 1, Kind::Eq, flags);
		}
		if b == b'.' && after != b'.' && !after.is_ascii_digit() {
			self.pos = pos + 1;
			return token(pos, pos + 1, Kind::Dot, flags);
		}
		unpack(self.rest(pos, flags, regex))
	}

	/// Every token, from `pos` on, `flags` being what the trivia before `pos` gave.
	#[inline(never)]
	fn rest(&mut self, pos: usize, flags: u8, regex: bool) -> (u64, u64) {
		pack(self.rest_token(pos, flags, regex))
	}

	#[cold]
	#[inline(never)]
	fn word_slow_pair(&mut self, start: usize, from: usize, flags: u8, regex: bool) -> (u64, u64) {
		pack(self.word_slow(start, from, Kind::Ident, flags, regex))
	}

	#[inline(always)]
	fn rest_token(&mut self, mut pos: usize, mut flags: u8, regex: bool) -> Token {
		let bytes = self.src.as_bytes();
		loop {
			let Some(&b) = bytes.get(pos) else {
				self.pos = pos;
				return token(pos, pos, Kind::Eof, flags);
			};
			let start = pos;
			let (kind, end) = match LEAD[b as usize] {
				Lead::Space => {
					pos += 1;
					while bytes.get(pos).is_some_and(|&b| CLASS[b as usize] & SPACE != 0) {
						pos += 1;
					}
					continue;
				}
				Lead::Newline => {
					flags |= NEWLINE_BEFORE;
					pos += 1;
					while bytes
						.get(pos)
						.is_some_and(|&b| CLASS[b as usize] & (SPACE | NEWLINE) != 0)
					{
						pos += 1;
					}
					continue;
				}
				Lead::Word => {
					let end = ident_end(bytes, pos + 1);
					if bytes.get(end).is_some_and(|&b| b == b'\\' || b >= 0x80) {
						return self.word_slow(start, end, Kind::Ident, flags, regex);
					}
					(keyword_spelled(&bytes[start..end]), end)
				}
				Lead::Single => (SINGLE[b as usize], pos + 1),
				Lead::Dot => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'.'), Some(b'.')) => (Kind::Ellipsis, pos + 3),
					(Some(b'0'..=b'9'), _) => return self.number(start, true, flags, regex),
					_ => (Kind::Dot, pos + 1),
				},
				Lead::Eq => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'='), Some(b'=')) => (Kind::EqEqEq, pos + 3),
					(Some(b'='), _) => (Kind::EqEq, pos + 2),
					(Some(b'>'), _) => (Kind::Arrow, pos + 2),
					_ => (Kind::Eq, pos + 1),
				},
				Lead::Quote => return self.string(start, b, flags, regex),
				Lead::Digit => return self.number(start, false, flags, regex),
				Lead::Slash => match bytes.get(pos + 1) {
					Some(b'/') => {
						let end = pos + 2 + line_end(&bytes[pos + 2..]);
						self.comment(pos, end, CommentKind::Line);
						pos = end;
						continue;
					}
					Some(b'*') => match self.block_comment(pos) {
						Some((end, broke)) => {
							flags |= broke as u8;
							pos = end;
							continue;
						}
						None if self.recover => {
							let rest = &bytes[pos + 2..];
							if line_end(rest) < rest.len() {
								flags |= NEWLINE_BEFORE;
							}
							self.soft(crate::error::SyntaxError::new(pos as u32, Code::UnterminatedComment), None);
							self.comment(pos, bytes.len(), CommentKind::Unclosed);
							self.pos = bytes.len();
							return token(bytes.len(), bytes.len(), Kind::Eof, flags);
						}
						None => return self.fail(Code::UnterminatedComment, pos),
					},
					_ if regex => return self.regex_at(start, flags, false),
					Some(b'=') => (Kind::SlashEq, pos + 2),
					_ => (Kind::Slash, pos + 1),
				},
				Lead::Question => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'?'), Some(b'=')) => (Kind::QuestionQuestionEq, pos + 3),
					(Some(b'?'), _) => (Kind::QuestionQuestion, pos + 2),
					(Some(b'.'), Some(d)) if !d.is_ascii_digit() => (Kind::QuestionDot, pos + 2),
					_ => (Kind::Question, pos + 1),
				},
				Lead::Bang => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'='), Some(b'=')) => (Kind::BangEqEq, pos + 3),
					(Some(b'='), _) => (Kind::BangEq, pos + 2),
					_ => (Kind::Bang, pos + 1),
				},
				Lead::Lt => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'!'), _) if !self.module && bytes[pos..].starts_with(b"<!--") => {
						let end = pos + 4 + line_end(&bytes[pos + 4..]);
						self.comment(pos, end, CommentKind::HtmlOpen);
						pos = end;
						continue;
					}
					(Some(b'<'), Some(b'=')) => (Kind::LtLtEq, pos + 3),
					(Some(b'<'), _) => (Kind::LtLt, pos + 2),
					(Some(b'='), _) => (Kind::LtEq, pos + 2),
					_ => (Kind::Lt, pos + 1),
				},
				Lead::Gt => match (bytes.get(pos + 1), bytes.get(pos + 2), bytes.get(pos + 3)) {
					(Some(b'>'), Some(b'>'), Some(b'=')) => (Kind::GtGtGtEq, pos + 4),
					(Some(b'>'), Some(b'>'), _) => (Kind::GtGtGt, pos + 3),
					(Some(b'>'), Some(b'='), _) => (Kind::GtGtEq, pos + 3),
					(Some(b'>'), _, _) => (Kind::GtGt, pos + 2),
					(Some(b'='), _, _) => (Kind::GtEq, pos + 2),
					_ => (Kind::Gt, pos + 1),
				},
				Lead::Plus => match bytes.get(pos + 1) {
					Some(b'+') => (Kind::PlusPlus, pos + 2),
					Some(b'=') => (Kind::PlusEq, pos + 2),
					_ => (Kind::Plus, pos + 1),
				},
				Lead::Minus => match bytes.get(pos + 1) {
					Some(b'-')
						if !self.module
							&& (flags & NEWLINE_BEFORE != 0 || self.pos == 0)
							&& bytes[pos..].starts_with(b"-->") =>
					{
						let end = pos + 3 + line_end(&bytes[pos + 3..]);
						self.comment(pos, end, CommentKind::HtmlClose);
						pos = end;
						continue;
					}
					Some(b'-') => (Kind::MinusMinus, pos + 2),
					Some(b'=') => (Kind::MinusEq, pos + 2),
					_ => (Kind::Minus, pos + 1),
				},
				Lead::Star => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'*'), Some(b'=')) => (Kind::StarStarEq, pos + 3),
					(Some(b'*'), _) => (Kind::StarStar, pos + 2),
					(Some(b'='), _) => (Kind::StarEq, pos + 2),
					_ => (Kind::Star, pos + 1),
				},
				Lead::Percent => match bytes.get(pos + 1) {
					Some(b'=') => (Kind::PercentEq, pos + 2),
					_ => (Kind::Percent, pos + 1),
				},
				Lead::Amp => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'&'), Some(b'=')) => (Kind::AmpAmpEq, pos + 3),
					(Some(b'&'), _) => (Kind::AmpAmp, pos + 2),
					(Some(b'='), _) => (Kind::AmpEq, pos + 2),
					_ => (Kind::Amp, pos + 1),
				},
				Lead::Pipe => match (bytes.get(pos + 1), bytes.get(pos + 2)) {
					(Some(b'|'), Some(b'=')) => (Kind::PipePipeEq, pos + 3),
					(Some(b'|'), _) => (Kind::PipePipe, pos + 2),
					(Some(b'='), _) => (Kind::PipeEq, pos + 2),
					_ => (Kind::Pipe, pos + 1),
				},
				Lead::Caret => match bytes.get(pos + 1) {
					Some(b'=') => (Kind::CaretEq, pos + 2),
					_ => (Kind::Caret, pos + 1),
				},
				Lead::Backquote => return self.template(start, pos + 1, Kind::Template, Kind::TemplateHead, flags),
				// a decorator's sign is TypeScript's: here no token starts with it
				Lead::At => {
					if !self.at_sign {
						return self.bad(start, flags, Code::UnexpectedCharacter, pos, pos, Arg::Char('@'), regex);
					}
					(Kind::At, pos + 1)
				}
				Lead::Hash => return self.private_name(start, flags, regex),
				Lead::Backslash => return self.word_slow(start, start, Kind::Ident, flags, regex),
				Lead::High => {
					let c = self.src[pos..].chars().next().unwrap();
					if scan::is_new_line(c) {
						flags |= NEWLINE_BEFORE;
					} else if !scan::is_whitespace(c) {
						if is_id_start(c) {
							return self.word_slow(start, start, Kind::Ident, flags, regex);
						}
						return self.bad(start, flags, Code::UnexpectedCharacter, pos, pos, Arg::Char(c), regex);
					}
					pos += c.len_utf8();
					continue;
				}
				Lead::Bad => return self.bad(start, flags, Code::UnexpectedCharacter, pos, pos, Arg::Char(b as char), regex),
			};
			self.pos = end;
			return token(start, end, kind, flags);
		}
	}

	/// The `/` or `/=` token read again as a regular expression: for a caller that asks for
	/// tokens without saying what a `/` is.
	pub fn regex(&mut self, slash: Token) -> Token {
		self.regex_at(slash.start as usize, slash.flags, true)
	}

	/// The `<` or `>` a longer token starts with, alone: in a type, `>>` closes two lists.
	pub fn angle(&mut self, token: Token) -> Token {
		let start = token.start as usize;
		let kind = if self.src.as_bytes()[start] == b'<' { Kind::Lt } else { Kind::Gt };
		self.pos = start + 1;
		self::token(start, start + 1, kind, token.flags)
	}

	/// The `}` that ends a template's expression, read again as the template's next chunk.
	pub fn template_continue(&mut self, brace: Token) -> Token {
		let start = brace.start as usize;
		self.template(start, start + 1, Kind::TemplateTail, Kind::TemplateMiddle, brace.flags)
	}

	#[cold]
	#[inline(never)]
	fn fail(&mut self, code: Code, at: usize) -> Token {
		self.fail_arg(code, at, Arg::None)
	}

	/// An error recorded under recovery.
	pub fn soft(&mut self, error: crate::error::SyntaxError, arg: Option<String>) {
		let soft = self.soft.get_or_insert_default();
		if let Some(arg) = arg {
			soft.error_args.push((soft.errors.len(), arg));
		}
		soft.errors.push(error);
	}

	/// How many errors recovery recorded.
	pub fn soft_len(&self) -> usize {
		self.soft.as_ref().map_or(0, |soft| soft.errors.len())
	}

	/// Where the read of a regular expression that failed for good stopped.
	pub fn fail_pos(&self) -> usize {
		self.soft.as_ref().map_or(0, |soft| soft.fail_pos)
	}

	/// The errors recorded at `pos` or after are forgotten: the text from there is read again.
	pub fn forget_after(&mut self, pos: u32) {
		let Some(soft) = &self.soft else { return };
		let len = soft.errors.iter().rposition(|e| e.pos < pos).map_or(0, |at| at + 1);
		self.forget_errors(len);
	}

	/// Whether the next `count` tokens lex, read strictly and put back: under recovery a look
	/// ahead that meets an error says nothing.
	pub fn lexes_ahead(&mut self, count: u32) -> bool {
		let (pos, comments, recover, escaped) = (self.pos, self.comments.len(), self.recover, self.escaped_keyword);
		self.recover = false;
		let mut clean = true;
		for _ in 0..count {
			let token = self.next(false);
			if self.error.take().is_some() {
				clean = false;
				break;
			}
			if token.kind == Kind::Eof {
				break;
			}
		}
		self.recover = recover;
		self.pos = pos;
		self.comments.truncate(comments);
		self.escaped_keyword = escaped;
		clean
	}

	/// The recorded errors from `len` on are forgotten: what read them is read again.
	pub fn forget_errors(&mut self, len: usize) {
		let Some(soft) = &mut self.soft else { return };
		soft.errors.truncate(len);
		while soft.error_args.last().is_some_and(|&(at, _)| at >= len) {
			soft.error_args.pop();
		}
	}

	/// The token from `start` does not lex: `code` at `at`. Under recovery it is recorded and
	/// skipped from where its reader stopped, `reach`, or from the next character.
	#[cold]
	#[inline(never)]
	fn bad(&mut self, start: usize, flags: u8, code: Code, at: usize, reach: usize, arg: Arg, regex: bool) -> Token {
		if !self.recover {
			return self.fail_arg(code, at, arg);
		}
		let arg = match arg {
			Arg::None => None,
			Arg::Char(c) => Some(c.to_string()),
			Arg::Radix(radix) => Some(radix.to_string()),
		};
		self.soft(crate::error::SyntaxError::new(at as u32, code), arg);
		self.pos = self.src.ceil_char_boundary(at.max(reach).max(start + 1));
		if flags & SKIPPING != 0 {
			return token(self.pos, self.pos, Kind::Eof, flags | SKIPPED);
		}
		self.skipped(flags, regex)
	}

	/// The tokens after one that was skipped, up to one that lexes: line breaks before any of
	/// them are the token's, and an HTML close comment starts a line only after its own break.
	#[cold]
	#[inline(never)]
	fn skipped(&mut self, flags: u8, regex: bool) -> Token {
		let mut newline = flags & NEWLINE_BEFORE;
		loop {
			let mut t = unpack(self.rest(self.pos, SKIPPING, regex));
			newline |= t.flags & NEWLINE_BEFORE;
			if t.flags & SKIPPED == 0 {
				t.flags = (t.flags & !SKIPPING) | newline;
				return t;
			}
		}
	}

	/// An error in a regular expression's flags, which ends the input under recovery too: the
	/// grammar's to recover from, after the escape.
	#[cold]
	#[inline(never)]
	fn fail_hard(&mut self, code: Code, at: usize, escape: usize) -> Token {
		if self.recover {
			self.soft.get_or_insert_default().fail_pos = word_escape_reach(self.src.as_bytes(), escape);
		}
		self.fail_arg(code, at, Arg::None)
	}

	#[cold]
	#[inline(never)]
	fn fail_arg(&mut self, code: Code, at: usize, arg: Arg) -> Token {
		if self.error.is_none() {
			self.error = Some(LexError {
				code,
				pos: at as u32,
				arg,
			});
		}
		let end = self.src.len();
		self.pos = end;
		token(end, end, Kind::Eof, 0)
	}

	fn comment(&mut self, start: usize, end: usize, kind: CommentKind) {
		if self.keep_comments {
			self.comments.push(Comment {
				start: start as u32,
				end: end as u32,
				kind,
			});
		}
	}

	/// Where the block comment at `pos` ends and whether it holds a line break.
	#[inline(never)]
	fn block_comment(&mut self, pos: usize) -> Option<(usize, bool)> {
		let bytes = self.src.as_bytes();
		let mut at = pos + 2;
		loop {
			at = find(bytes, at, [b'*'], false);
			if at >= bytes.len() {
				return None;
			}
			if bytes.get(at + 1) == Some(&b'/') {
				break;
			}
			at += 1;
		}
		let body = &bytes[pos + 2..at];
		let broke = line_end(body) < body.len();
		self.comment(pos, at + 2, CommentKind::Block);
		Some((at + 2, broke))
	}

	/// A word that holds an escape or a character past ASCII, from `from` on. A keyword spelled
	/// with an escape is that keyword, remembered until something reads it as a name.
	#[cold]
	#[inline(never)]
	fn word_slow(&mut self, start: usize, from: usize, kind: Kind, mut flags: u8, regex: bool) -> Token {
		let bytes = self.src.as_bytes();
		let first_at = if kind == Kind::PrivateName { start + 1 } else { start };
		let mut pos = from;
		loop {
			match bytes.get(pos) {
				Some(b'\\') => match values::word_escape(self.src, pos, pos == first_at) {
					Ok((_, end)) => {
						flags |= ESCAPED;
						pos = end;
					}
					Err((code, at)) => {
						return self.bad(start, flags, code, at, word_escape_reach(bytes, pos), Arg::None, regex);
					}
				},
				Some(&b) if b < 0x80 => {
					if CLASS[b as usize] & ID_CONTINUE == 0 {
						break;
					}
					pos += 1;
				}
				Some(_) => {
					let c = self.src[pos..].chars().next().unwrap();
					let ok = if pos == first_at { is_id_start(c) } else { is_id_continue(c) };
					if !ok {
						break;
					}
					pos += c.len_utf8();
				}
				None => break,
			}
		}
		self.pos = pos;
		let mut kind = kind;
		if kind == Kind::Ident {
			if flags & ESCAPED == 0 {
				kind = keyword_spelled(&bytes[start..pos]);
			} else {
				let mut values = values::Values::default();
				let word = values.word(self.src, start, pos);
				kind = keyword_spelled(word.as_bytes());
				if kind != Kind::Ident && self.escaped_keyword.is_none() && !self.recover {
					self.escaped_keyword = Some((start as u32, pos as u32));
				}
			}
		}
		token(start, pos, kind, flags)
	}

	#[inline(never)]
	fn private_name(&mut self, start: usize, flags: u8, regex: bool) -> Token {
		let bytes = self.src.as_bytes();
		match bytes.get(start + 1) {
			Some(&b) if CLASS[b as usize] & ID_START != 0 => {
				let end = ident_end(bytes, start + 2);
				if bytes.get(end).is_some_and(|&b| b == b'\\' || b >= 0x80) {
					return self.word_slow(start, end, Kind::PrivateName, flags, regex);
				}
				self.pos = end;
				token(start, end, Kind::PrivateName, flags)
			}
			Some(b'\\') => self.word_slow(start, start + 1, Kind::PrivateName, flags, regex),
			Some(&b) if b >= 0x80 && is_id_start(self.src[start + 1..].chars().next().unwrap()) => {
				self.word_slow(start, start + 1, Kind::PrivateName, flags, regex)
			}
			_ => {
				let c = self.src[start + 1..].chars().next().unwrap_or('\u{10000}');
				self.bad(start, flags, Code::UnexpectedCharacter, start + 1, start + 1, Arg::Char(c), regex)
			}
		}
	}

	#[inline(never)]
	fn number(&mut self, start: usize, starts_with_dot: bool, flags: u8, regex: bool) -> Token {
		let bytes = self.src.as_bytes();
		// digits alone, the usual number: nothing after them that a longer one continues with
		if !starts_with_dot && bytes[start] != b'0' {
			let mut end = start + 1;
			while bytes.get(end).is_some_and(|&b| CLASS[b as usize] & DIGIT != 0) {
				end += 1;
			}
			let plain = match bytes.get(end) {
				Some(&b) => b < 0x80 && CLASS[b as usize] & ID_START == 0 && b != b'.',
				None => true,
			};
			if plain {
				self.pos = end;
				return token(start, end, Kind::Number, flags);
			}
		}
		let mut pos = start;
		if !starts_with_dot && bytes[pos] == b'0' {
			let radix = match bytes.get(pos + 1) {
				Some(b'x' | b'X') => 16,
				Some(b'o' | b'O') => 8,
				Some(b'b' | b'B') => 2,
				_ => 0,
			};
			if radix != 0 {
				let from = pos + 2;
				pos = match digits(bytes, from, radix, false) {
					Ok(end) if end > from => end,
					Ok(end) => return self.bad(start, flags, Code::ExpectedNumberInRadix, end, end, Arg::Radix(radix), regex),
					Err((code, at)) => return self.bad(start, flags, code, at, digits_reach(code, at), Arg::None, regex),
				};
				if bytes.get(pos) == Some(&b'n') {
					self.pos = pos + 1;
					return token(start, pos + 1, Kind::BigInt, flags);
				}
				return self.after_number(start, pos, Kind::Number, flags, regex);
			}
		}
		if !starts_with_dot {
			pos = match digits(bytes, pos, 10, true) {
				Ok(end) => end,
				Err((code, at)) => return self.bad(start, flags, code, at, digits_reach(code, at), Arg::None, regex),
			};
		}
		let legacy = pos - start >= 2 && bytes[start] == b'0';
		if legacy && self.strict {
			return self.bad(start, flags, Code::InvalidNumber, start, pos, Arg::None, regex);
		}
		if !legacy && !starts_with_dot && bytes.get(pos) == Some(&b'n') {
			return self.after_number(start, pos + 1, Kind::BigInt, flags, regex);
		}
		let octal = legacy && bytes[start..pos].iter().all(|b| (b'0'..=b'7').contains(b));
		if !octal && bytes.get(pos) == Some(&b'.') {
			pos = match digits(bytes, pos + 1, 10, false) {
				Ok(end) => end,
				Err((code, at)) => return self.bad(start, flags, code, at, digits_reach(code, at), Arg::None, regex),
			};
		}
		if !octal && matches!(bytes.get(pos), Some(b'e' | b'E')) {
			pos += 1;
			if matches!(bytes.get(pos), Some(b'+' | b'-')) {
				pos += 1;
			}
			pos = match digits(bytes, pos, 10, false) {
				Ok(end) if end > pos => end,
				Ok(_) => return self.bad(start, flags, Code::InvalidNumber, start, pos, Arg::None, regex),
				Err((code, at)) => return self.bad(start, flags, code, at, digits_reach(code, at), Arg::None, regex),
			};
		}
		self.after_number(start, pos, Kind::Number, flags, regex)
	}

	/// A number may not run into a name.
	fn after_number(&mut self, start: usize, end: usize, kind: Kind, flags: u8, regex: bool) -> Token {
		match self.src.as_bytes().get(end) {
			Some(&b) if b < 0x80 && CLASS[b as usize] & ID_START == 0 => {}
			None => {}
			Some(&b) if b < 0x80 => return self.bad(start, flags, Code::IdentifierAfterNumber, end, end, Arg::None, regex),
			Some(_) => {
				if is_id_start(self.src[end..].chars().next().unwrap()) {
					return self.bad(start, flags, Code::IdentifierAfterNumber, end, end, Arg::None, regex);
				}
			}
		}
		self.pos = end;
		token(start, end, kind, flags)
	}

	#[inline(never)]
	fn string(&mut self, start: usize, quote: u8, mut flags: u8, regex: bool) -> Token {
		let bytes = self.src.as_bytes();
		let mut pos = start + 1;
		loop {
			pos = string_stop(bytes, pos, if quote == b'"' { 1 } else { 2 } | 4);
			match bytes.get(pos) {
				Some(b'\\') => {
					flags |= ESCAPED;
					let (escape, next) = values::escape(self.src, pos + 1);
					match escape {
						values::Escape::Invalid(code, at) => {
							return self.bad(start, flags, code, at, string_escape_reach(bytes, pos), Arg::None, regex);
						}
						values::Escape::Octal(_, at, is_89) if self.strict => {
							let code = if is_89 { Code::StrictEscape } else { Code::StrictOctal };
							return self.bad(start, flags, code, at, next, Arg::None, regex);
						}
						_ => {}
					}
					pos = next;
				}
				Some(&b) if b == quote => break,
				_ if self.recover => {
					// recorded, and the string ends where its line does
					self.soft(crate::error::SyntaxError::new(start as u32, Code::UnterminatedString), None);
					self.pos = pos;
					return token(start, pos, Kind::String, flags | BROKEN);
				}
				_ => return self.fail(Code::UnterminatedString, start),
			}
		}
		self.pos = pos + 1;
		token(start, pos + 1, Kind::String, flags)
	}

	/// A template chunk from `from` on: `whole` when it ends at the backquote, `open` at `${`.
	#[inline(never)]
	fn template(&mut self, start: usize, from: usize, whole: Kind, open: Kind, mut flags: u8) -> Token {
		let bytes = self.src.as_bytes();
		let mut pos = from;
		loop {
			pos = find(bytes, pos, *b"`$\\\r", false);
			match bytes.get(pos) {
				Some(b'`') => {
					self.pos = pos + 1;
					return token(start, pos + 1, whole, flags);
				}
				Some(b'$') if bytes.get(pos + 1) == Some(&b'{') => {
					self.pos = pos + 2;
					return token(start, pos + 2, open, flags);
				}
				Some(b'$') => pos += 1,
				Some(b'\\') => {
					flags |= ESCAPED;
					pos += 2;
				}
				// a carriage return is a line feed in the cooked value
				Some(_) => {
					flags |= ESCAPED;
					pos += 1;
				}
				None => {
					self.pos = bytes.len();
					return token(start, bytes.len(), whole, flags | BROKEN);
				}
			}
		}
	}

	#[inline(never)]
	fn regex_at(&mut self, start: usize, flags: u8, again: bool) -> Token {
		if !again {
			// a regular expression that does not lex is the grammar's error, met where the
			// operand is read: until then the `/` is the operator it would be
			let errors = self.soft_len();
			let token = self.regex_at(start, flags, true);
			if self.error.take().is_some() {
				self.forget_errors(errors);
				let (kind, end) = if self.src.as_bytes().get(start + 1) == Some(&b'=') {
					(Kind::SlashEq, start + 2)
				} else {
					(Kind::Slash, start + 1)
				};
				self.pos = end;
				return token_at(start, end, kind, flags);
			}
			return token;
		}
		let bytes = self.src.as_bytes();
		let mut pos = start + 1;
		let mut in_class = false;
		loop {
			while bytes
				.get(pos)
				.is_some_and(|&b| CLASS[b as usize] & (NEWLINE | REGEXP) == 0)
			{
				pos += 1;
			}
			let escaped = bytes.get(pos) == Some(&b'\\');
			pos += escaped as usize;
			let cut = match bytes.get(pos..).unwrap_or_default() {
				[] | [b'\n' | b'\r', ..] => true,
				rest @ [0xe2, ..] => is_separator(rest),
				_ => false,
			};
			if cut {
				if self.recover {
					self.soft(crate::error::SyntaxError::new(start as u32 + 1, Code::UnterminatedRegexp), None);
					self.pos = pos;
					return token(start, pos, Kind::RegExp, flags | BROKEN);
				}
				return self.fail(Code::UnterminatedRegexp, start + 1);
			}
			if !escaped {
				match bytes[pos] {
					b'[' => in_class = true,
					b']' => in_class = false,
					b'/' if !in_class => break,
					_ => {}
				}
			}
			pos += 1;
		}
		pos += 1;
		// the flags: a word, and an escape in it is an error where they start
		let flags_start = pos;
		let mut escaped = false;
		loop {
			match bytes.get(pos) {
				Some(&b) if b < 0x80 && CLASS[b as usize] & ID_CONTINUE != 0 => pos += 1,
				Some(b'\\') => match values::word_escape(self.src, pos, pos == flags_start) {
					Ok((_, end)) => {
						escaped = true;
						pos = end;
					}
					Err((code, at)) => return self.fail_hard(code, at, pos),
				},
				Some(&b) if b >= 0x80 => {
					let c = self.src[pos..].chars().next().unwrap();
					if !is_id_continue(c) {
						break;
					}
					pos += c.len_utf8();
				}
				_ => break,
			}
		}
		if escaped {
			if !self.recover {
				return self.fail(Code::UnexpectedToken, flags_start);
			}
			self.soft(crate::error::SyntaxError::new(flags_start as u32, Code::UnexpectedToken), None);
		}
		self.pos = pos;
		token(start, pos, Kind::RegExp, flags)
	}
}

#[inline(always)]
fn pack(t: Token) -> (u64, u64) {
	(t.start as u64 | (t.end as u64) << 32, t.kind as u8 as u64 | (t.flags as u64) << 8)
}

#[inline(always)]
fn unpack((span, tag): (u64, u64)) -> Token {
	Token {
		start: span as u32,
		end: (span >> 32) as u32,
		// SAFETY: `pack` wrote a `Kind` there
		kind: unsafe { std::mem::transmute::<u8, Kind>(tag as u8) },
		flags: (tag >> 8) as u8,
	}
}

fn token_at(start: usize, end: usize, kind: Kind, flags: u8) -> Token {
	token(start, end, kind, flags)
}

fn token(start: usize, end: usize, kind: Kind, flags: u8) -> Token {
	Token {
		start: start as u32,
		end: end as u32,
		kind,
		flags,
	}
}

/// Where the run of ASCII identifier bytes from `pos` ends.
// tried: eight bytes behind one bounds check, 0.2 to 3.3% more instructions per parse
#[inline]
fn ident_end(bytes: &[u8], mut pos: usize) -> usize {
	while let Some(&b) = bytes.get(pos) {
		if CLASS[b as usize] & ID_CONTINUE == 0 {
			break;
		}
		pos += 1;
	}
	pos
}

#[inline(always)]
fn keyword_spelled(word: &[u8]) -> Kind {
	use Kind::*;
	match word {
		b"do" => Do,
		b"if" => If,
		b"in" => In,
		b"for" => For,
		b"new" => New,
		b"try" => Try,
		b"var" => Var,
		b"case" => Case,
		b"else" => Else,
		b"null" => Null,
		b"this" => This,
		b"true" => True,
		b"void" => Void,
		b"with" => With,
		b"break" => Break,
		b"catch" => Catch,
		b"class" => Class,
		b"const" => Const,
		b"false" => False,
		b"super" => Super,
		b"throw" => Throw,
		b"while" => While,
		b"delete" => Delete,
		b"export" => Export,
		b"import" => Import,
		b"return" => Return,
		b"switch" => Switch,
		b"typeof" => Typeof,
		b"default" => Default,
		b"extends" => Extends,
		b"finally" => Finally,
		b"continue" => Continue,
		b"debugger" => Debugger,
		b"function" => Function,
		b"instanceof" => Instanceof,
		_ => Ident,
	}
}

/// The end of the digits from `from`, separators checked; a misplaced separator's rule and place.
fn digits(bytes: &[u8], from: usize, radix: u32, maybe_legacy_octal: bool) -> Result<usize, (Code, usize)> {
	let legacy_octal = maybe_legacy_octal && bytes.get(from) == Some(&b'0');
	let mut pos = from;
	let mut last_was_separator = false;
	while let Some(&b) = bytes.get(pos) {
		if b == b'_' {
			if legacy_octal {
				return Err((Code::NumericSeparatorLegacyOctal, pos));
			}
			if last_was_separator {
				return Err((Code::NumericSeparatorDouble, pos));
			}
			if pos == from {
				return Err((Code::NumericSeparatorFirst, pos));
			}
			last_was_separator = true;
			pos += 1;
			continue;
		}
		if !(b as char).is_digit(radix) {
			break;
		}
		last_was_separator = false;
		pos += 1;
		if radix == 10 {
			while bytes.get(pos).is_some_and(|&b| CLASS[b as usize] & DIGIT != 0) {
				pos += 1;
			}
		}
	}
	if last_was_separator {
		return Err((Code::NumericSeparatorLast, pos - 1));
	}
	Ok(pos)
}

/// Where the reader of a number's digits stood when it failed with `code` at `at`.
fn digits_reach(code: Code, at: usize) -> usize {
	at + (code == Code::NumericSeparatorLast) as usize
}

/// Where the hex digits from `at` end, `len` of them at most.
fn hex_reach(bytes: &[u8], at: usize, len: usize) -> usize {
	let mut pos = at;
	while pos < at + len && bytes.get(pos).is_some_and(u8::is_ascii_hexdigit) {
		pos += 1;
	}
	pos
}

/// Where the reader stopped in the code point after a `\u` that ends at `at`.
fn code_point_reach(bytes: &[u8], at: usize) -> usize {
	if bytes.get(at) != Some(&b'{') {
		return hex_reach(bytes, at, 4);
	}
	let mut pos = at + 1;
	while bytes.get(pos).is_some_and(u8::is_ascii_hexdigit) {
		pos += 1;
	}
	if pos == at + 1 || bytes.get(pos) != Some(&b'}') {
		return pos;
	}
	pos + 1
}

/// Where the reader stopped in the word escape at `pos`, a backslash, that failed.
fn word_escape_reach(bytes: &[u8], pos: usize) -> usize {
	match bytes.get(pos + 1) {
		Some(b'u') => code_point_reach(bytes, pos + 2),
		_ => pos + 1,
	}
}

/// Where the reader stopped in the string escape at `pos`, a backslash, that failed.
fn string_escape_reach(bytes: &[u8], pos: usize) -> usize {
	match bytes.get(pos + 1) {
		Some(b'x') => hex_reach(bytes, pos + 2, 2),
		_ => code_point_reach(bytes, pos + 2),
	}
}

/// What may end a string or change its value: bit 1 a double quote, 2 a single one, 4 a
/// backslash or a line break.
static STRING: [u8; 256] = {
	let mut table = [0u8; 256];
	table[b'"' as usize] = 1;
	table[b'\'' as usize] = 2;
	table[b'\\' as usize] = 4;
	table[b'\n' as usize] = 4;
	table[b'\r' as usize] = 4;
	table
};

/// Where the first byte of `mask` stands from `pos` on.
#[inline(always)]
fn string_stop(bytes: &[u8], mut pos: usize, mask: u8) -> usize {
	while let Some(chunk) = bytes.get(pos..pos + 8) {
		let chunk: &[u8; 8] = chunk.try_into().unwrap();
		let mut i = 0;
		while i < 8 {
			if STRING[chunk[i] as usize] & mask != 0 {
				return pos + i;
			}
			i += 1;
		}
		pos += 8;
	}
	while let Some(&b) = bytes.get(pos) {
		if STRING[b as usize] & mask != 0 {
			break;
		}
		pos += 1;
	}
	pos
}
