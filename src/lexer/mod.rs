mod identifier;
mod number;
mod regex;
mod regexp;
mod regexp_data;
mod string;
pub(crate) mod token;
pub(crate) mod unicode;

#[cfg(test)]
mod tests;

use crate::ast::{Comment, CommentKind};
use crate::error::SyntaxError;
use crate::interner::Interner;
use token::{Token, TokenKind};
use unicode::is_id_start;

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

	fn error<T>(&self, pos: usize, message: impl Into<String>) -> Result<T> {
		Err(Box::new(SyntaxError::new(pos as u32, message)))
	}

	pub(crate) fn next_token(&mut self) -> Result<Token> {
		let newline_before = self.skip_space()?;
		let start = self.pos;
		self.escaped = false;
		let Some(b) = self.byte() else {
			return Ok(Token {
				kind: TokenKind::Eof,
				start: start as u32,
				end: start as u32,
				newline_before,
				escaped: false,
			});
		};

		let kind = match b {
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
					return self.error(start, format!("Unexpected character '{c}'"));
				}
			}
		};

		Ok(Token {
			kind,
			start: start as u32,
			end: self.pos as u32,
			newline_before,
			escaped: self.escaped,
		})
	}

	fn skip_space(&mut self) -> Result<bool> {
		let mut newline = false;
		let last_end = self.pos;
		if self.pos == 0 && self.src.starts_with("#!") {
			self.skip_line_comment(CommentKind::Hashbang);
		}
		while let Some(b) = self.byte() {
			match b {
				b'<' if !self.module && self.src[self.pos..].starts_with("<!--") => {
					self.skip_line_comment(CommentKind::HtmlOpen)
				}
				b'-' if !self.module && (last_end == 0 || newline) && self.src[self.pos..].starts_with("-->") => {
					self.skip_line_comment(CommentKind::HtmlClose)
				}
				b' ' | b'\t' | 0x0b | 0x0c => self.pos += 1,
				b'\n' | b'\r' => {
					self.pos += 1;
					newline = true;
				}
				b'/' => match self.byte_at(1) {
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
			return self.error(start, "Unterminated comment");
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
			_ => return self.error(self.pos, format!("Unexpected character '{}'", b as char)),
		};
		self.pos += len;
		Ok(kind)
	}
}

/// Whether `bytes` starts with a line separator or paragraph separator (U+2028, U+2029).
pub(crate) fn is_separator(bytes: &[u8]) -> bool {
	matches!(bytes, [0xe2, 0x80, 0xa8 | 0xa9, ..])
}

/// The length of `bytes` up to the first line terminator.
pub(crate) fn line_end(bytes: &[u8]) -> usize {
	let mut i = 0;
	while i < bytes.len() {
		match bytes[i] {
			b'\n' | b'\r' => break,
			0xe2 if is_separator(&bytes[i..]) => break,
			_ => i += 1,
		}
	}
	i
}

/// Where `*/` starts in `text`, and whether a line terminator precedes it.
pub(crate) fn comment_end(text: &str) -> Option<(usize, bool)> {
	let mut from = 0;
	loop {
		let star = from + text[from..].find('*')?;
		if text.as_bytes().get(star + 1) == Some(&b'/') {
			let body = &text[..star];
			let newline = ['\n', '\r', '\u{2028}', '\u{2029}'].iter().any(|&c| body.contains(c));
			return Some((star, newline));
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
