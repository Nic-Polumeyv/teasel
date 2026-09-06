use super::token::{Token, TokenKind};
use super::{Lexer, Result};
use crate::error::{Code, SyntaxError};

impl Lexer<'_> {
	pub(super) fn read_string(&mut self, quote: u8) -> Result<TokenKind> {
		let start = self.pos;
		self.pos += 1;
		self.buf.clear();
		let mut pending = None;
		let mut chunk_start = self.pos;
		loop {
			let bytes = self.src.as_bytes();
			while bytes
				.get(self.pos)
				.is_some_and(|&b| b < 0x80 && b != quote && !matches!(b, b'\\' | b'\n' | b'\r'))
			{
				self.pos += 1;
			}
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
						Escape::Invalid(e) => return Err(Box::new(e)),
					}
					chunk_start = self.pos;
				}
				'\n' | '\r' => return self.error(start, Code::UnterminatedString),
				_ => self.pos += c.len_utf8(),
			}
		}
	}

	/// Reads a template chunk, leaving the position after the closing backquote or `${`.
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

	/// Appends a code unit from an escape, pairing surrogates across escapes as JavaScript strings do.
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
					None => Escape::Invalid(SyntaxError::new(digits as u32, Code::BadCharacterEscape)),
				});
			}
			'u' => {
				return Ok(match self.read_code_point() {
					Ok(code) => Escape::Code(code),
					Err(e) => Escape::Invalid(*e),
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

	pub(super) fn read_hex(&mut self, len: usize) -> Option<u32> {
		let mut value = 0;
		for _ in 0..len {
			let digit = (self.byte()? as char).to_digit(16)?;
			value = value * 16 + digit;
			self.pos += 1;
		}
		Some(value)
	}

	/// Reads the hex digits of a `\u` escape, returning a code point that may be a lone surrogate.
	pub(super) fn read_code_point(&mut self) -> Result<u32> {
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

enum Escape {
	Char(char),
	Code(u32),
	Octal(char, usize, bool),
	Nothing,
	Invalid(SyntaxError),
}
