use super::token::{Keyword, TokenKind};
use super::unicode::{is_id_continue, is_id_start};
use super::{Lexer, Result, scan};
use crate::error::Code;

impl Lexer<'_> {
	pub(super) fn read_word(&mut self) -> Result<TokenKind> {
		let start = self.pos;
		let src = self.src;
		let bytes = src.as_bytes();
		let mut pos = start;
		if bytes.get(pos).is_some_and(|&b| scan::class(b) & scan::ID_START != 0) {
			pos = scan::run_of(bytes, pos + 1, scan::ID_CONTINUE);
		}
		self.pos = pos;
		// nearly every word is ASCII and ends at an ASCII byte: nothing more to read
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

	/// A keyword, or the name interned.
	fn word(&mut self, word: &str) -> TokenKind {
		match Keyword::from_word(word) {
			Some(keyword) => TokenKind::Keyword(keyword),
			None => TokenKind::Ident(self.strings.intern(word)),
		}
	}

	pub(super) fn read_word_escape(&mut self, first: bool) -> Result<char> {
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

	pub(super) fn read_private_name(&mut self) -> Result<TokenKind> {
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
}

pub(super) fn is_word_char(c: char, first: bool) -> bool {
	match c {
		'$' | '_' => true,
		'\u{200c}' | '\u{200d}' => !first,
		_ if first => is_id_start(c),
		_ => is_id_continue(c),
	}
}
