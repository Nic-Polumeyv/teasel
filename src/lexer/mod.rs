mod identifier;
mod number;
mod regex;
mod string;
pub(crate) mod token;
pub(crate) mod unicode;

#[cfg(test)]
mod tests;

use crate::ast::Comment;
use crate::error::SyntaxError;
use crate::interner::Interner;
use token::{Token, TokenKind};
use unicode::is_id_start;

type Result<T> = std::result::Result<T, SyntaxError>;

#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
	pos: usize,
	comments: usize,
}

/// Positions are byte offsets into the source.
pub(crate) struct Lexer<'a> {
	src: &'a str,
	pos: usize,
	buf: String,
	pub(crate) comments: Vec<Comment>,
	pub(crate) strings: Interner,
}

impl<'a> Lexer<'a> {
	pub(crate) fn new(src: &'a str) -> Self {
		Self {
			src,
			pos: 0,
			buf: String::new(),
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

	pub(crate) fn snapshot(&self) -> Snapshot {
		Snapshot {
			pos: self.pos,
			comments: self.comments.len(),
		}
	}

	pub(crate) fn restore(&mut self, snapshot: Snapshot) {
		self.pos = snapshot.pos;
		self.comments.truncate(snapshot.comments);
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
		Err(SyntaxError::new(pos as u32, message))
	}

	pub(crate) fn next_token(&mut self) -> Result<Token> {
		let newline_before = self.skip_space()?;
		let start = self.pos;
		let Some(b) = self.byte() else {
			return Ok(Token {
				kind: TokenKind::Eof,
				start: start as u32,
				end: start as u32,
				newline_before,
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
		})
	}

	fn skip_space(&mut self) -> Result<bool> {
		let mut newline = false;
		if self.pos == 0 && self.src.starts_with("#!") {
			self.skip_line_comment(2, false);
		}
		while let Some(b) = self.byte() {
			match b {
				b' ' | b'\t' | 0x0b | 0x0c => self.pos += 1,
				b'\n' | b'\r' => {
					self.pos += 1;
					newline = true;
				}
				b'/' => match self.byte_at(1) {
					Some(b'/') => self.skip_line_comment(2, true),
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

	fn skip_line_comment(&mut self, skip: usize, record: bool) {
		let start = self.pos;
		self.pos += skip;
		while let Some(c) = self.char() {
			if is_new_line(c) {
				break;
			}
			self.pos += c.len_utf8();
		}
		if record {
			self.comments.push(Comment {
				block: false,
				start: start as u32,
				end: self.pos as u32,
			});
		}
	}

	fn skip_block_comment(&mut self) -> Result<bool> {
		let start = self.pos;
		let Some(len) = self.src[start + 2..].find("*/") else {
			return self.error(start, "Unterminated comment");
		};
		let end = start + 2 + len + 2;
		let newline = self.src[start + 2..end - 2].chars().any(is_new_line);
		self.pos = end;
		self.comments.push(Comment {
			block: true,
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
			b'@' => (At, 1),
			b'.' => match (next, next2) {
				(Some(b'.'), Some(b'.')) => (Ellipsis, 3),
				_ => (Dot, 1),
			},
			b'?' => match (next, next2) {
				(Some(b'?'), Some(b'=')) => (QuestionQuestionEq, 3),
				(Some(b'?'), _) => (QuestionQuestion, 2),
				(Some(b'.'), Some(d)) if d.is_ascii_digit() => (Question, 1),
				(Some(b'.'), _) => (QuestionDot, 2),
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

pub(crate) fn is_new_line(c: char) -> bool {
	matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn is_whitespace(c: char) -> bool {
	matches!(
		c,
		'\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
	)
}
