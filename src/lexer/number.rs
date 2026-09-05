use super::identifier::is_word_char;
use super::token::TokenKind;
use super::{Lexer, Result};

impl Lexer<'_> {
	pub(super) fn read_number(&mut self, starts_with_dot: bool) -> Result<TokenKind> {
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
			return self.error(start, "Invalid number");
		}
		let legacy = self.pos - start >= 2 && self.src.as_bytes()[start] == b'0';
		if legacy && self.strict {
			return self.error(start, "Invalid number");
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
				return self.error(start, "Invalid number");
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
			return self.error(self.pos, format!("Expected number in radix {radix}"));
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
					return self.error(
						self.pos,
						"Numeric separator is not allowed in legacy octal numeric literals",
					);
				}
				if last_was_separator {
					return self.error(self.pos, "Numeric separator must be exactly one underscore");
				}
				if self.pos == start {
					return self.error(self.pos, "Numeric separator is not allowed at the first of digits");
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
			return self.error(self.pos - 1, "Numeric separator is not allowed at the last of digits");
		}
		Ok((self.pos > start).then_some(total))
	}

	fn check_after_number(&mut self) -> Result<()> {
		if let Some(c) = self.char()
			&& is_word_char(c, true)
		{
			return self.error(self.pos, "Identifier directly after number");
		}
		Ok(())
	}
}
