use super::token::{Keyword, TokenKind};
use super::unicode::{is_id_continue, is_id_start};
use super::{Lexer, Result};

impl Lexer<'_> {
	pub(super) fn read_word(&mut self) -> Result<TokenKind> {
		let start = self.pos;
		let mut first = true;
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
		let word = if self.escaped {
			self.buf.as_str()
		} else {
			&self.src[start..self.pos]
		};
		if let Some(keyword) = Keyword::from_word(word) {
			return Ok(TokenKind::Keyword(keyword));
		}
		let name = self.strings.intern(word);
		Ok(TokenKind::Ident(name))
	}

	fn read_word_escape(&mut self, first: bool) -> Result<char> {
		let esc_start = self.pos;
		self.pos += 1;
		if self.byte() != Some(b'u') {
			return self.error(self.pos, "Expecting Unicode escape sequence \\uXXXX");
		}
		self.pos += 1;
		let code = self.read_code_point()?;
		match char::from_u32(code) {
			Some(c) if is_word_char(c, first) => Ok(c),
			_ => self.error(esc_start, "Invalid Unicode escape"),
		}
	}

	pub(super) fn read_private_name(&mut self) -> Result<TokenKind> {
		self.pos += 1;
		match self.char() {
			Some(c) if c == '\\' || is_word_char(c, true) => {}
			next => {
				let c = next.unwrap_or('\u{10000}');
				return self.error(self.pos, format!("Unexpected character '{c}'"));
			}
		}
		let name = match self.read_word()? {
			TokenKind::Ident(name) => name,
			TokenKind::Keyword(keyword) => self.strings.intern(keyword.as_str()),
			_ => unreachable!(),
		};
		Ok(TokenKind::PrivateName(name))
	}
}

pub(super) fn is_word_char(c: char, first: bool) -> bool {
	match c {
		'$' | '_' => true,
		'\u{200c}' | '\u{200d}' => !first,
		_ if first => is_id_start(c),
		_ => is_id_continue(c),
	}
}
