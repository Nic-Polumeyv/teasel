use super::identifier::is_word_char;
use super::token::{Token, TokenKind};
use super::{Lexer, Result, is_new_line};

impl Lexer<'_> {
	/// Re-reads a `/` or `/=` token as a regular expression literal.
	pub(crate) fn read_regex(&mut self, token: Token) -> Result<Token> {
		let start = token.start as usize;
		self.pos = start + 1;
		let mut escaped = false;
		let mut in_class = false;
		loop {
			let Some(c) = self.char() else {
				return self.error(start, "Unterminated regular expression");
			};
			if is_new_line(c) {
				return self.error(start, "Unterminated regular expression");
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
		while let Some(c) = self.char() {
			if c == '\\' {
				return self.error(self.pos, "Invalid regular expression flag");
			}
			if !is_word_char(c, false) {
				break;
			}
			self.pos += c.len_utf8();
		}
		let flags = self.strings.intern(&self.src[flags_start..self.pos]);
		let kind = TokenKind::RegExp { pattern, flags };
		Ok(Token {
			kind,
			start: start as u32,
			end: self.pos as u32,
			newline_before: token.newline_before,
		})
	}
}
