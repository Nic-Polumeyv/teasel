use super::token::{Keyword, TokenKind};
use super::unicode::{is_id_continue, is_id_start};
use super::{Lexer, Result};

impl Lexer<'_> {
	pub(super) fn read_word(&mut self) -> Result<TokenKind> {
		let start = self.pos;
		let mut escaped = false;
		let mut first = true;
		self.buf.clear();
		while let Some(c) = self.char() {
			if c == '\\' {
				if !escaped {
					escaped = true;
					self.buf.push_str(&self.src[start..self.pos]);
				}
				let c = self.read_word_escape(first)?;
				self.buf.push(c);
			} else if is_word_char(c, first) {
				self.pos += c.len_utf8();
				if escaped {
					self.buf.push(c);
				}
			} else {
				break;
			}
			first = false;
		}
		let word = if escaped {
			self.buf.as_str()
		} else {
			&self.src[start..self.pos]
		};
		if let Some(keyword) = Keyword::from_word(word) {
			if escaped {
				return self.error(start, format!("Escape sequence in keyword {}", keyword.as_str()));
			}
			return Ok(TokenKind::Keyword(keyword));
		}
		let name = self.strings.intern(word);
		Ok(TokenKind::Ident { name, escaped })
	}

	fn read_word_escape(&mut self, first: bool) -> Result<char> {
		let esc_start = self.pos;
		self.pos += 1;
		if self.byte() != Some(b'u') {
			return self.error(self.pos, "Expecting Unicode escape sequence \\uXXXX");
		}
		self.pos += 1;
		let c = self.read_code_point()?;
		if !is_word_char(c, first) {
			return self.error(esc_start, "Invalid Unicode escape");
		}
		Ok(c)
	}

	pub(super) fn read_private_name(&mut self) -> Result<TokenKind> {
		let start = self.pos;
		self.pos += 1;
		match self.char() {
			Some(c) if c == '\\' || is_word_char(c, true) => {}
			_ => return self.error(start, "Unexpected character '#'"),
		}
		let name = match self.read_word()? {
			TokenKind::Ident { name, .. } => self.strings.get(name).to_owned(),
			TokenKind::Keyword(keyword) => keyword.as_str().to_owned(),
			_ => unreachable!(),
		};
		let name = self.strings.intern(&format!("#{name}"));
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
