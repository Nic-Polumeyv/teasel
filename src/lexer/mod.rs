mod regexp;
mod regexp_data;
pub(crate) mod scan;
pub(crate) mod token;
pub(crate) mod unicode;

#[cfg(test)]
mod tests;

use crate::ast::{Comment, CommentKind};
use crate::error::{Code, SyntaxError};
use crate::interner::Interner;
use token::{Keyword, Token, TokenKind};
use unicode::{is_id_continue, is_id_start};

type Result<T> = std::result::Result<T, Box<SyntaxError>>;

/// Positions are byte offsets into the source.
pub(crate) struct Lexer<'a> {
	src: &'a str,
	pos: usize,
	buf: String,
	escaped: bool,
	/// Strict mode rejects legacy octal literals and escapes while scanning, as acorn does.
	pub(crate) strict: bool,
	/// Modules have no HTML-style comments.
	pub(crate) module: bool,
	/// Reads `@` as a token instead of rejecting it.
	pub(crate) at_sign: bool,
	/// Reads `<` and `>` as single characters, so `>>` closes two type argument lists.
	pub(crate) in_type: bool,
	pub(crate) comments: Vec<Comment>,
	pub(crate) strings: Interner,
}

impl<'a> Lexer<'a> {
	pub(crate) fn new(src: &'a str) -> Self {
		Self {
			src,
			pos: 0,
			buf: String::new(),
			escaped: false,
			strict: false,
			module: false,
			at_sign: false,
			in_type: false,
			comments: Vec::new(),
			strings: Interner::default(),
		}
	}

	pub(crate) fn source(&self) -> &'a str {
		self.src
	}

	pub(crate) fn set_pos(&mut self, pos: u32) {
		self.pos = pos as usize;
	}

	pub(crate) fn pos(&self) -> u32 {
		self.pos as u32
	}

	pub(crate) fn escaped(&self) -> bool {
		self.escaped
	}

	pub(crate) fn set_escaped(&mut self, escaped: bool) {
		self.escaped = escaped;
	}

	/// The token after the current one, leaving the lexer where it was.
	pub(crate) fn peek_token(&mut self) -> Result<Token> {
		let pos = self.pos;
		let comments = self.comments.len();
		let token = self.next_token();
		self.pos = pos;
		self.comments.truncate(comments);
		token
	}

	/// The next significant character, whether a line break precedes it, and its position.
	pub(crate) fn peek_char(&self) -> (Option<char>, bool, usize) {
		let mut pos = self.pos;
		let mut newline = false;
		let bytes = self.src.as_bytes();
		loop {
			let Some(&b) = bytes.get(pos) else {
				return (None, newline, pos);
			};
			match b {
				b' ' | b'\t' | 0x0b | 0x0c => pos += 1,
				b'\n' | b'\r' => {
					pos += 1;
					newline = true;
				}
				b'/' if bytes.get(pos + 1) == Some(&b'/') => pos += line_end(&bytes[pos..]),
				b'/' if bytes.get(pos + 1) == Some(&b'*') => {
					let Some((len, broke)) = comment_end(&self.src[pos + 2..]) else {
						return (None, newline, self.src.len());
					};
					newline |= broke;
					pos += len + 4;
				}
				_ => {
					let c = self.src[pos..].chars().next().unwrap();
					if is_new_line(c) {
						newline = true;
					} else if !is_whitespace(c) {
						return (Some(c), newline, pos);
					}
					pos += c.len_utf8();
				}
			}
		}
	}

	fn byte(&self) -> Option<u8> {
		self.src.as_bytes().get(self.pos).copied()
	}

	fn byte_at(&self, offset: usize) -> Option<u8> {
		self.src.as_bytes().get(self.pos + offset).copied()
	}

	fn char(&self) -> Option<char> {
		self.src[self.pos..].chars().next()
	}

	fn error<T>(&self, pos: usize, code: Code) -> Result<T> {
		Err(Box::new(SyntaxError::new(pos as u32, code)))
	}

	fn error_with<T>(&self, pos: usize, code: Code, message: impl Into<String>) -> Result<T> {
		Err(Box::new(SyntaxError::with(pos as u32, code, message)))
	}

	pub(crate) fn next_token(&mut self) -> Result<Token> {
		let mut token = Token {
			kind: TokenKind::Eof,
			start: 0,
			end: 0,
			newline_before: false,
			escaped: false,
		};
		self.next_token_into(&mut token)?;
		Ok(token)
	}

	// in place: a `Result<Token>` is five words, moved at every `?`, and was 16% of a parse
	pub(crate) fn next_token_into(&mut self, token: &mut Token) -> Result<()> {
		token.newline_before = self.skip_space()?;
		let start = self.pos;
		self.escaped = false;
		token.start = start as u32;
		let Some(b) = self.byte() else {
			token.kind = TokenKind::Eof;
			token.end = start as u32;
			token.escaped = false;
			return Ok(());
		};

		token.kind = match b {
			b'0'..=b'9' => self.read_number(false)?,
			b'.' if self.byte_at(1).is_some_and(|b| b.is_ascii_digit()) => self.read_number(true)?,
			b'"' | b'\'' => self.read_string(b)?,
			b'`' => {
				self.pos += 1;
				TokenKind::Backquote
			}
			b'#' => self.read_private_name()?,
			b'\\' | b'$' | b'_' | b'a'..=b'z' | b'A'..=b'Z' => self.read_word()?,
			_ if b < 0x80 => self.read_punctuator(b)?,
			_ => {
				let c = self.char().unwrap();
				if is_id_start(c) {
					self.read_word()?
				} else {
					return self.error_with(start, Code::UnexpectedCharacter, format!("Unexpected character '{c}'"));
				}
			}
		};

		token.end = self.pos as u32;
		token.escaped = self.escaped;
		Ok(())
	}

	fn skip_space(&mut self) -> Result<bool> {
		let mut newline = false;
		let last_end = self.pos;
		if self.pos == 0 && self.src.starts_with("#!") {
			self.skip_line_comment(CommentKind::Hashbang);
		}
		let src = self.src;
		let bytes = src.as_bytes();
		while let Some(&b) = bytes.get(self.pos) {
			let class = scan::class(b);
			if class & scan::SPACE != 0 {
				self.pos = scan::run_of(bytes, self.pos + 1, scan::SPACE);
				continue;
			}
			if class & scan::NEWLINE != 0 {
				self.pos += 1;
				newline = true;
				continue;
			}
			match b {
				b'<' if !self.module && src[self.pos..].starts_with("<!--") => {
					self.skip_line_comment(CommentKind::HtmlOpen)
				}
				b'-' if !self.module && (last_end == 0 || newline) && src[self.pos..].starts_with("-->") => {
					self.skip_line_comment(CommentKind::HtmlClose)
				}
				b'/' => match bytes.get(self.pos + 1) {
					Some(b'/') => self.skip_line_comment(CommentKind::Line),
					Some(b'*') => newline |= self.skip_block_comment()?,
					_ => break,
				},
				_ if b < 0x80 => break,
				_ => {
					let c = self.char().unwrap();
					if is_new_line(c) {
						newline = true;
					} else if !is_whitespace(c) {
						break;
					}
					self.pos += c.len_utf8();
				}
			}
		}
		Ok(newline)
	}

	fn skip_line_comment(&mut self, kind: CommentKind) {
		let start = self.pos;
		self.pos += match kind {
			CommentKind::HtmlOpen => 4,
			CommentKind::HtmlClose => 3,
			_ => 2,
		};
		self.pos += line_end(&self.src.as_bytes()[self.pos..]);
		self.comments.push(Comment {
			kind,
			start: start as u32,
			end: self.pos as u32,
		});
	}

	fn skip_block_comment(&mut self) -> Result<bool> {
		let start = self.pos;
		let Some((len, newline)) = comment_end(&self.src[start + 2..]) else {
			return self.error(start, Code::UnterminatedComment);
		};
		let end = start + 2 + len + 2;
		self.pos = end;
		self.comments.push(Comment {
			kind: CommentKind::Block,
			start: start as u32,
			end: end as u32,
		});
		Ok(newline)
	}

	fn read_punctuator(&mut self, b: u8) -> Result<TokenKind> {
		use TokenKind::*;
		let next = self.byte_at(1);
		let next2 = self.byte_at(2);
		let next3 = self.byte_at(3);
		let (kind, len) = match b {
			b'{' => (BraceL, 1),
			b'}' => (BraceR, 1),
			b'(' => (ParenL, 1),
			b')' => (ParenR, 1),
			b'[' => (BracketL, 1),
			b']' => (BracketR, 1),
			b';' => (Semi, 1),
			b',' => (Comma, 1),
			b':' => (Colon, 1),
			b'~' => (Tilde, 1),
			b'.' => match (next, next2) {
				(Some(b'.'), Some(b'.')) => (Ellipsis, 3),
				_ => (Dot, 1),
			},
			b'?' => match (next, next2) {
				(Some(b'?'), Some(b'=')) => (QuestionQuestionEq, 3),
				(Some(b'?'), _) => (QuestionQuestion, 2),
				(Some(b'.'), Some(d)) if !d.is_ascii_digit() => (QuestionDot, 2),
				_ => (Question, 1),
			},
			b'=' => match (next, next2) {
				(Some(b'='), Some(b'=')) => (EqEqEq, 3),
				(Some(b'='), _) => (EqEq, 2),
				(Some(b'>'), _) => (Arrow, 2),
				_ => (Eq, 1),
			},
			b'!' => match (next, next2) {
				(Some(b'='), Some(b'=')) => (BangEqEq, 3),
				(Some(b'='), _) => (BangEq, 2),
				_ => (Bang, 1),
			},
			b'@' if self.at_sign => (At, 1),
			b'<' | b'>' if self.in_type => (if b == b'<' { Lt } else { Gt }, 1),
			b'<' => match (next, next2) {
				(Some(b'<'), Some(b'=')) => (LtLtEq, 3),
				(Some(b'<'), _) => (LtLt, 2),
				(Some(b'='), _) => (LtEq, 2),
				_ => (Lt, 1),
			},
			b'>' => match (next, next2, next3) {
				(Some(b'>'), Some(b'>'), Some(b'=')) => (GtGtGtEq, 4),
				(Some(b'>'), Some(b'>'), _) => (GtGtGt, 3),
				(Some(b'>'), Some(b'='), _) => (GtGtEq, 3),
				(Some(b'>'), _, _) => (GtGt, 2),
				(Some(b'='), _, _) => (GtEq, 2),
				_ => (Gt, 1),
			},
			b'+' => match next {
				Some(b'+') => (PlusPlus, 2),
				Some(b'=') => (PlusEq, 2),
				_ => (Plus, 1),
			},
			b'-' => match next {
				Some(b'-') => (MinusMinus, 2),
				Some(b'=') => (MinusEq, 2),
				_ => (Minus, 1),
			},
			b'*' => match (next, next2) {
				(Some(b'*'), Some(b'=')) => (StarStarEq, 3),
				(Some(b'*'), _) => (StarStar, 2),
				(Some(b'='), _) => (StarEq, 2),
				_ => (Star, 1),
			},
			b'/' => match next {
				Some(b'=') => (SlashEq, 2),
				_ => (Slash, 1),
			},
			b'%' => match next {
				Some(b'=') => (PercentEq, 2),
				_ => (Percent, 1),
			},
			b'&' => match (next, next2) {
				(Some(b'&'), Some(b'=')) => (AmpAmpEq, 3),
				(Some(b'&'), _) => (AmpAmp, 2),
				(Some(b'='), _) => (AmpEq, 2),
				_ => (Amp, 1),
			},
			b'|' => match (next, next2) {
				(Some(b'|'), Some(b'=')) => (PipePipeEq, 3),
				(Some(b'|'), _) => (PipePipe, 2),
				(Some(b'='), _) => (PipeEq, 2),
				_ => (Pipe, 1),
			},
			b'^' => match next {
				Some(b'=') => (CaretEq, 2),
				_ => (Caret, 1),
			},
			_ => {
				return self.error_with(
					self.pos,
					Code::UnexpectedCharacter,
					format!("Unexpected character '{}'", b as char),
				);
			}
		};
		self.pos += len;
		Ok(kind)
	}

	fn read_word(&mut self) -> Result<TokenKind> {
		let start = self.pos;
		let src = self.src;
		let bytes = src.as_bytes();
		let mut pos = start;
		if bytes.get(pos).is_some_and(|&b| scan::class(b) & scan::ID_START != 0) {
			pos = scan::run_of(bytes, pos + 1, scan::ID_CONTINUE);
		}
		self.pos = pos;
		if pos > start && bytes.get(pos).is_none_or(|&b| b < 0x80 && b != b'\\') {
			return Ok(self.word(&src[start..pos]));
		}
		let mut first = pos == start;
		self.buf.clear();
		while let Some(c) = self.char() {
			if c == '\\' {
				if !self.escaped {
					self.escaped = true;
					self.buf.push_str(&self.src[start..self.pos]);
				}
				let c = self.read_word_escape(first)?;
				self.buf.push(c);
			} else if is_word_char(c, first) {
				self.pos += c.len_utf8();
				if self.escaped {
					self.buf.push(c);
				}
			} else {
				break;
			}
			first = false;
		}
		if self.escaped {
			let buf = std::mem::take(&mut self.buf);
			let kind = self.word(&buf);
			self.buf = buf;
			Ok(kind)
		} else {
			Ok(self.word(&src[start..self.pos]))
		}
	}

	fn word(&mut self, word: &str) -> TokenKind {
		match Keyword::from_word(word) {
			Some(keyword) => TokenKind::Keyword(keyword),
			None => TokenKind::Ident(self.strings.intern(word)),
		}
	}

	fn read_word_escape(&mut self, first: bool) -> Result<char> {
		let esc_start = self.pos;
		self.pos += 1;
		if self.byte() != Some(b'u') {
			return self.error(self.pos, Code::ExpectedUnicodeEscape);
		}
		self.pos += 1;
		let code = self.read_code_point()?;
		match char::from_u32(code) {
			Some(c) if is_word_char(c, first) => Ok(c),
			_ => self.error(esc_start, Code::InvalidUnicodeEscape),
		}
	}

	fn read_private_name(&mut self) -> Result<TokenKind> {
		self.pos += 1;
		match self.char() {
			Some(c) if c == '\\' || is_word_char(c, true) => {}
			next => {
				let c = next.unwrap_or('\u{10000}');
				return self.error_with(
					self.pos,
					Code::UnexpectedCharacter,
					format!("Unexpected character '{c}'"),
				);
			}
		}
		let name = match self.read_word()? {
			TokenKind::Ident(name) => name,
			TokenKind::Keyword(keyword) => self.strings.intern(keyword.as_str()),
			_ => unreachable!(),
		};
		Ok(TokenKind::PrivateName(name))
	}

	fn read_number(&mut self, starts_with_dot: bool) -> Result<TokenKind> {
		let start = self.pos;
		if !starts_with_dot && self.byte() == Some(b'0') {
			match self.byte_at(1) {
				Some(b'x' | b'X') => return self.read_radix_number(16),
				Some(b'o' | b'O') => return self.read_radix_number(8),
				Some(b'b' | b'B') => return self.read_radix_number(2),
				_ => {}
			}
		}
		if !starts_with_dot && self.read_int(10, true)?.is_none() {
			return self.error(start, Code::InvalidNumber);
		}
		let legacy = self.pos - start >= 2 && self.src.as_bytes()[start] == b'0';
		if legacy && self.strict {
			return self.error(start, Code::InvalidNumber);
		}
		if !legacy && !starts_with_dot && self.byte() == Some(b'n') {
			self.pos += 1;
			self.check_after_number()?;
			return Ok(TokenKind::BigInt);
		}
		let octal = legacy && self.src[start..self.pos].bytes().all(|b| (b'0'..=b'7').contains(&b));
		if !octal && self.byte() == Some(b'.') {
			self.pos += 1;
			self.read_int(10, false)?;
		}
		if !octal && matches!(self.byte(), Some(b'e' | b'E')) {
			self.pos += 1;
			if matches!(self.byte(), Some(b'+' | b'-')) {
				self.pos += 1;
			}
			if self.read_int(10, false)?.is_none() {
				return self.error(start, Code::InvalidNumber);
			}
		}
		self.check_after_number()?;
		let text = &self.src[start..self.pos];
		let value = if octal {
			match u128::from_str_radix(text, 8) {
				Ok(v) => v as f64,
				Err(_) => text.bytes().fold(0.0, |acc, b| acc * 8.0 + (b - b'0') as f64),
			}
		} else if text.contains('_') {
			text.replace('_', "").parse().unwrap()
		} else {
			text.parse().unwrap()
		};
		Ok(TokenKind::Number(value))
	}

	fn read_radix_number(&mut self, radix: u32) -> Result<TokenKind> {
		self.pos += 2;
		let Some(value) = self.read_int(radix, false)? else {
			return self.error_with(
				self.pos,
				Code::ExpectedNumberInRadix,
				format!("Expected number in radix {radix}"),
			);
		};
		if self.byte() == Some(b'n') {
			self.pos += 1;
			return Ok(TokenKind::BigInt);
		}
		self.check_after_number()?;
		Ok(TokenKind::Number(value))
	}

	fn read_int(&mut self, radix: u32, maybe_legacy_octal: bool) -> Result<Option<f64>> {
		let start = self.pos;
		let legacy_octal = maybe_legacy_octal && self.byte() == Some(b'0');
		let mut total = 0.0;
		let mut last_was_separator = false;
		while let Some(b) = self.byte() {
			if b == b'_' {
				if legacy_octal {
					return self.error(self.pos, Code::NumericSeparatorLegacyOctal);
				}
				if last_was_separator {
					return self.error(self.pos, Code::NumericSeparatorDouble);
				}
				if self.pos == start {
					return self.error(self.pos, Code::NumericSeparatorFirst);
				}
				last_was_separator = true;
				self.pos += 1;
				continue;
			}
			let Some(digit) = (b as char).to_digit(radix) else {
				break;
			};
			last_was_separator = false;
			total = total * radix as f64 + digit as f64;
			self.pos += 1;
		}
		if last_was_separator {
			return self.error(self.pos - 1, Code::NumericSeparatorLast);
		}
		Ok((self.pos > start).then_some(total))
	}

	fn check_after_number(&mut self) -> Result<()> {
		if let Some(c) = self.char()
			&& is_word_char(c, true)
		{
			return self.error(self.pos, Code::IdentifierAfterNumber);
		}
		Ok(())
	}

	pub(crate) fn read_regex(&mut self, token: Token) -> Result<Token> {
		let start = token.start as usize;
		self.pos = start + 1;
		let mut escaped = false;
		let mut in_class = false;
		loop {
			let Some(c) = self.char() else {
				return self.error(start + 1, Code::UnterminatedRegexp);
			};
			if is_new_line(c) {
				return self.error(start + 1, Code::UnterminatedRegexp);
			}
			if escaped {
				escaped = false;
			} else {
				match c {
					'[' => in_class = true,
					']' if in_class => in_class = false,
					'/' if !in_class => break,
					'\\' => escaped = true,
					_ => {}
				}
			}
			self.pos += c.len_utf8();
		}
		let pattern = self.strings.intern(&self.src[start + 1..self.pos]);
		self.pos += 1;
		let flags_start = self.pos;
		let mut escaped = false;
		while let Some(c) = self.char() {
			if c == '\\' {
				self.read_word_escape(self.pos == flags_start)?;
				escaped = true;
			} else if is_word_char(c, false) {
				self.pos += c.len_utf8();
			} else {
				break;
			}
		}
		if escaped {
			return self.error(flags_start, Code::UnexpectedToken);
		}
		let flags_text = &self.src[flags_start..self.pos];
		regexp::validate(start as u32 + 1, &self.src[start + 1..flags_start - 1], flags_text)?;
		let flags = self.strings.intern(flags_text);
		let kind = TokenKind::RegExp { pattern, flags };
		Ok(Token {
			kind,
			start: start as u32,
			end: self.pos as u32,
			newline_before: token.newline_before,
			escaped: false,
		})
	}

	fn read_string(&mut self, quote: u8) -> Result<TokenKind> {
		let start = self.pos;
		self.pos += 1;
		self.buf.clear();
		let mut pending = None;
		let mut chunk_start = self.pos;
		loop {
			self.pos = scan::find(self.src.as_bytes(), self.pos, [quote, b'\\', b'\n', b'\r'], false);
			let Some(c) = self.char() else {
				return self.error(start, Code::UnterminatedString);
			};
			match c {
				_ if c as u32 == quote as u32 => {
					self.push_chunk(chunk_start, &mut pending);
					self.flush(&mut pending);
					self.pos += 1;
					let value = self.strings.intern(&self.buf);
					return Ok(TokenKind::String(value));
				}
				'\\' => {
					self.push_chunk(chunk_start, &mut pending);
					self.pos += 1;
					match self.read_escape()? {
						Escape::Char(c) => {
							self.flush(&mut pending);
							self.buf.push(c);
						}
						Escape::Code(code) => self.push_code(code, &mut pending),
						Escape::Octal(c, pos, is_89) => {
							if self.strict {
								let code = if is_89 { Code::StrictEscape } else { Code::StrictOctal };
								return self.error(pos, code);
							}
							self.flush(&mut pending);
							self.buf.push(c);
						}
						Escape::Nothing => {}
						Escape::Invalid(e) => return Err(e),
					}
					chunk_start = self.pos;
				}
				'\n' | '\r' => return self.error(start, Code::UnterminatedString),
				_ => self.pos += c.len_utf8(),
			}
		}
	}

	pub(crate) fn read_template(&mut self) -> Result<Token> {
		let start = self.pos;
		if start == self.src.len() {
			return self.error_with(start, Code::UnterminatedTemplate, "Unterminated template literal");
		}
		self.buf.clear();
		let mut valid = true;
		let mut plain = true;
		let mut pending = None;
		let mut chunk_start = self.pos;
		loop {
			self.pos = scan::find(self.src.as_bytes(), self.pos, *b"`$\\\r", false);
			let Some(c) = self.char() else {
				return self.error(start, Code::UnterminatedTemplate);
			};
			match c {
				'`' | '$' if c == '`' || self.byte_at(1) == Some(b'{') => {
					self.push_chunk(chunk_start, &mut pending);
					self.flush(&mut pending);
					let end = self.pos;
					let tail = c == '`';
					self.pos += if tail { 1 } else { 2 };
					let raw_text = &self.src[start..end];
					let raw = if raw_text.contains('\r') {
						let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
						self.strings.intern(&normalized)
					} else {
						self.strings.intern(raw_text)
					};
					let cooked = if !valid {
						None
					} else if plain {
						Some(raw)
					} else {
						Some(self.strings.intern(&self.buf))
					};
					let kind = TokenKind::Template { cooked, raw, tail };
					return Ok(Token {
						kind,
						start: start as u32,
						end: end as u32,
						newline_before: false,
						escaped: false,
					});
				}
				'\\' => {
					plain = false;
					self.push_chunk(chunk_start, &mut pending);
					self.pos += 1;
					match self.read_escape()? {
						Escape::Char(c) => {
							self.flush(&mut pending);
							self.buf.push(c);
						}
						Escape::Code(code) => self.push_code(code, &mut pending),
						Escape::Nothing => {}
						Escape::Octal(..) | Escape::Invalid(..) => valid = false,
					}
					chunk_start = self.pos;
				}
				'\r' => {
					plain = false;
					self.push_chunk(chunk_start, &mut pending);
					self.flush(&mut pending);
					self.buf.push('\n');
					self.pos += 1;
					if self.byte() == Some(b'\n') {
						self.pos += 1;
					}
					chunk_start = self.pos;
				}
				_ => self.pos += c.len_utf8(),
			}
		}
	}

	fn push_chunk(&mut self, chunk_start: usize, pending: &mut Option<u32>) {
		if chunk_start < self.pos {
			self.flush(pending);
			self.buf.push_str(&self.src[chunk_start..self.pos]);
		}
	}

	// surrogates pair across escapes, as JavaScript strings do
	fn push_code(&mut self, code: u32, pending: &mut Option<u32>) {
		if let Some(high) = pending.take() {
			if (0xdc00..0xe000).contains(&code) {
				self.buf
					.push(char::from_u32(0x10000 + ((high - 0xd800) << 10) + (code - 0xdc00)).unwrap());
				return;
			}
			self.buf.push('\u{fffd}');
		}
		if (0xd800..0xdc00).contains(&code) {
			*pending = Some(code);
		} else {
			self.buf.push(char::from_u32(code).unwrap_or('\u{fffd}'));
		}
	}

	fn flush(&mut self, pending: &mut Option<u32>) {
		if pending.take().is_some() {
			self.buf.push('\u{fffd}');
		}
	}

	fn read_escape(&mut self) -> Result<Escape> {
		let backslash = self.pos - 1;
		let Some(c) = self.char() else {
			return Ok(Escape::Char('\0'));
		};
		self.pos += c.len_utf8();
		Ok(Escape::Char(match c {
			'n' => '\n',
			'r' => '\r',
			't' => '\t',
			'b' => '\u{8}',
			'v' => '\u{b}',
			'f' => '\u{c}',
			'x' => {
				let digits = self.pos;
				return Ok(match self.read_hex(2) {
					Some(v) => Escape::Code(v),
					None => Escape::Invalid(Box::new(SyntaxError::new(digits as u32, Code::BadCharacterEscape))),
				});
			}
			'u' => {
				return Ok(match self.read_code_point() {
					Ok(code) => Escape::Code(code),
					Err(e) => Escape::Invalid(e),
				});
			}
			'\r' => {
				if self.byte() == Some(b'\n') {
					self.pos += 1;
				}
				return Ok(Escape::Nothing);
			}
			'\n' | '\u{2028}' | '\u{2029}' => return Ok(Escape::Nothing),
			'8' | '9' => return Ok(Escape::Octal(c, self.pos - 1, true)),
			'0'..='7' => {
				let mut value = c.to_digit(8).unwrap();
				let mut digits = 1;
				while digits < 3 {
					match self.byte() {
						Some(b @ b'0'..=b'7') if value * 8 + (b - b'0') as u32 <= 255 => {
							value = value * 8 + (b - b'0') as u32;
							self.pos += 1;
							digits += 1;
						}
						_ => break,
					}
				}
				let next_is_89 = matches!(self.byte(), Some(b'8' | b'9'));
				let c = char::from_u32(value).unwrap();
				if c != '\0' || digits > 1 || next_is_89 {
					return Ok(Escape::Octal(c, backslash, false));
				}
				c
			}
			_ => c,
		}))
	}

	fn read_hex(&mut self, len: usize) -> Option<u32> {
		let mut value = 0;
		for _ in 0..len {
			let digit = (self.byte()? as char).to_digit(16)?;
			value = value * 16 + digit;
			self.pos += 1;
		}
		Some(value)
	}

	fn read_code_point(&mut self) -> Result<u32> {
		if self.byte() == Some(b'{') {
			self.pos += 1;
			let digits = self.pos;
			let mut value: u32 = 0;
			while let Some(d) = self.byte().and_then(|b| (b as char).to_digit(16)) {
				value = value.saturating_mul(16).saturating_add(d);
				self.pos += 1;
			}
			if self.pos == digits || self.byte() != Some(b'}') {
				return self.error(digits, Code::BadCharacterEscape);
			}
			self.pos += 1;
			if value > 0x10ffff {
				return self.error(digits, Code::CodePointOutOfBounds);
			}
			return Ok(value);
		}
		let digits = self.pos;
		match self.read_hex(4) {
			Some(v) => Ok(v),
			None => self.error(digits, Code::BadCharacterEscape),
		}
	}
}

/// Whether `bytes` starts with a line separator or paragraph separator (U+2028, U+2029).
pub(crate) fn is_separator(bytes: &[u8]) -> bool {
	matches!(bytes, [0xe2, 0x80, 0xa8 | 0xa9, ..])
}

pub(crate) fn line_end(bytes: &[u8]) -> usize {
	let mut i = 0;
	loop {
		i = scan::find(bytes, i, *b"\n\r\xe2", false);
		if i == bytes.len() || bytes[i] != 0xe2 || is_separator(&bytes[i..]) {
			return i;
		}
		i += 1;
	}
}

/// Where `*/` starts in `text`, and whether a line terminator precedes it.
pub(crate) fn comment_end(text: &str) -> Option<(usize, bool)> {
	let mut from = 0;
	loop {
		let star = from + text[from..].find('*')?;
		if text.as_bytes().get(star + 1) == Some(&b'/') {
			let body = &text.as_bytes()[..star];
			return Some((star, line_end(body) < body.len()));
		}
		from = star + 1;
	}
}

pub(crate) fn is_new_line(c: char) -> bool {
	matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

pub(crate) fn is_whitespace(c: char) -> bool {
	matches!(
		c,
		'\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
	)
}

fn is_word_char(c: char, first: bool) -> bool {
	match c {
		'$' | '_' => true,
		'\u{200c}' | '\u{200d}' => !first,
		_ if first => is_id_start(c),
		_ => is_id_continue(c),
	}
}

enum Escape {
	Char(char),
	Code(u32),
	Octal(char, usize, bool),
	Nothing,
	Invalid(Box<SyntaxError>),
}
