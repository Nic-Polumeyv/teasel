use super::token::{Token, TokenKind};
use super::{Lexer, Result};

impl Lexer<'_> {
	pub(super) fn read_string(&mut self, quote: u8) -> Result<TokenKind> {
		let start = self.pos;
		self.pos += 1;
		self.buf.clear();
		let mut octal = false;
		let mut chunk_start = self.pos;
		loop {
			let Some(c) = self.char() else {
				return self.error(start, "Unterminated string constant");
			};
			match c {
				_ if c as u32 == quote as u32 => {
					self.buf.push_str(&self.src[chunk_start..self.pos]);
					self.pos += 1;
					let value = self.strings.intern(&self.buf);
					return Ok(TokenKind::String { value, octal });
				}
				'\\' => {
					self.buf.push_str(&self.src[chunk_start..self.pos]);
					self.pos += 1;
					match self.read_escape()? {
						Escape::Char(c) => self.buf.push(c),
						Escape::Octal(c) => {
							self.buf.push(c);
							octal = true;
						}
						Escape::Nothing => {}
						Escape::Invalid(pos, message) => return self.error(pos, message),
					}
					chunk_start = self.pos;
				}
				'\n' | '\r' => return self.error(start, "Unterminated string constant"),
				_ => self.pos += c.len_utf8(),
			}
		}
	}

	/// Reads a template chunk, leaving the position after the closing backquote or `${`.
	pub(crate) fn read_template(&mut self) -> Result<Token> {
		let start = self.pos;
		self.buf.clear();
		let mut valid = true;
		let mut chunk_start = self.pos;
		loop {
			let Some(c) = self.char() else {
				return self.error(start, "Unterminated template");
			};
			match c {
				'`' | '$' if c == '`' || self.byte_at(1) == Some(b'{') => {
					self.buf.push_str(&self.src[chunk_start..self.pos]);
					let end = self.pos;
					let tail = c == '`';
					self.pos += if tail { 1 } else { 2 };
					let cooked = valid.then(|| self.strings.intern(&self.buf));
					let raw = self.src[start..end].replace("\r\n", "\n").replace('\r', "\n");
					let raw = self.strings.intern(&raw);
					let kind = TokenKind::Template { cooked, raw, tail };
					return Ok(Token {
						kind,
						start: start as u32,
						end: end as u32,
						newline_before: false,
					});
				}
				'\\' => {
					self.buf.push_str(&self.src[chunk_start..self.pos]);
					self.pos += 1;
					match self.read_escape()? {
						Escape::Char(c) => self.buf.push(c),
						Escape::Nothing => {}
						Escape::Octal(_) | Escape::Invalid(..) => valid = false,
					}
					chunk_start = self.pos;
				}
				'\r' => {
					self.buf.push_str(&self.src[chunk_start..self.pos]);
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

	fn read_escape(&mut self) -> Result<Escape> {
		let Some(c) = self.char() else {
			return self.error(self.pos, "Bad escape sequence");
		};
		let esc_pos = self.pos - 1;
		self.pos += c.len_utf8();
		Ok(Escape::Char(match c {
			'n' => '\n',
			'r' => '\r',
			't' => '\t',
			'b' => '\u{8}',
			'v' => '\u{b}',
			'f' => '\u{c}',
			'x' => match self.read_hex(2) {
				Some(v) => char::from_u32(v).unwrap(),
				None => return Ok(Escape::Invalid(esc_pos, "Bad character escape sequence")),
			},
			'u' => match self.read_code_point() {
				Ok(c) => c,
				Err(e) => return Ok(Escape::Invalid(e.pos as usize, "Bad character escape sequence")),
			},
			'\r' => {
				if self.byte() == Some(b'\n') {
					self.pos += 1;
				}
				return Ok(Escape::Nothing);
			}
			'\n' | '\u{2028}' | '\u{2029}' => return Ok(Escape::Nothing),
			'8' | '9' => return Ok(Escape::Octal(c)),
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
					return Ok(Escape::Octal(c));
				}
				c
			}
			_ => c,
		}))
	}

	pub(super) fn read_hex(&mut self, len: usize) -> Option<u32> {
		let start = self.pos;
		let mut value = 0;
		for _ in 0..len {
			let digit = (self.byte()? as char).to_digit(16)?;
			value = value * 16 + digit;
			self.pos += 1;
		}
		(self.pos - start == len).then_some(value)
	}

	pub(super) fn read_code_point(&mut self) -> Result<char> {
		let start = self.pos;
		let value = if self.byte() == Some(b'{') {
			self.pos += 1;
			let mut value: u32 = 0;
			let digits_start = self.pos;
			while let Some(d) = self.byte().and_then(|b| (b as char).to_digit(16)) {
				value = value.saturating_mul(16).saturating_add(d);
				self.pos += 1;
			}
			if self.pos == digits_start || self.byte() != Some(b'}') {
				return self.error(start, "Bad character escape sequence");
			}
			self.pos += 1;
			if value > 0x10ffff {
				return self.error(start, "Code point out of bounds");
			}
			value
		} else {
			let Some(v) = self.read_hex(4) else {
				return self.error(start, "Bad character escape sequence");
			};
			v
		};
		Ok(self
			.surrogate_pair(value)
			.unwrap_or_else(|| char::from_u32(value).unwrap_or('\u{fffd}')))
	}

	fn surrogate_pair(&mut self, high: u32) -> Option<char> {
		if !(0xd800..0xdc00).contains(&high) || !self.src[self.pos..].starts_with("\\u") {
			return None;
		}
		let save = self.pos;
		self.pos += 2;
		if let Some(low) = self.read_hex(4)
			&& (0xdc00..0xe000).contains(&low)
		{
			return char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00));
		}
		self.pos = save;
		None
	}
}

enum Escape {
	Char(char),
	Octal(char),
	Nothing,
	Invalid(usize, &'static str),
}
