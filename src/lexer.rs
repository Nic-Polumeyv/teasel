use crate::error::SyntaxError;
use crate::token::{Comment, Keyword, Token, TokenKind};
use crate::unicode::{is_id_continue, is_id_start};

type Result<T> = std::result::Result<T, SyntaxError>;

/// Positions are byte offsets into the source.
pub struct Lexer<'a> {
	src: &'a str,
	pos: usize,
	pub comments: Vec<Comment>,

	/// Cooked value of the last identifier, private name, string or template token.
	pub value: String,
	/// Whether the last string or template token cooked without invalid escapes.
	pub cooked_valid: bool,
	/// Whether the last identifier contained a unicode escape.
	pub contains_escape: bool,
	/// Value of the last number token.
	pub number: f64,
	/// Position of a legacy octal number or escape in the last token, for strict mode checks.
	pub legacy_octal: Option<u32>,
	/// Whether the last template token ended the template.
	pub template_tail: bool,
	/// Start of the flags in the last regex token.
	pub regex_flags_start: u32,
}

impl<'a> Lexer<'a> {
	pub fn new(src: &'a str) -> Self {
		Self {
			src,
			pos: 0,
			comments: Vec::new(),
			value: String::new(),
			cooked_valid: true,
			contains_escape: false,
			number: 0.0,
			legacy_octal: None,
			template_tail: false,
			regex_flags_start: 0,
		}
	}

	pub fn source(&self) -> &'a str {
		self.src
	}

	pub fn pos(&self) -> u32 {
		self.pos as u32
	}

	pub fn set_pos(&mut self, pos: u32) {
		self.pos = pos as usize;
	}

	pub fn slice(&self, start: u32, end: u32) -> &'a str {
		&self.src[start as usize..end as usize]
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

	pub fn next_token(&mut self) -> Result<Token> {
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

	fn read_word(&mut self) -> Result<TokenKind> {
		let start = self.pos;
		self.value.clear();
		self.contains_escape = false;
		let mut first = true;
		while let Some(c) = self.char() {
			if c == '\\' {
				if !self.contains_escape {
					self.contains_escape = true;
					self.value.push_str(&self.src[start..self.pos]);
				}
				let c = self.read_word_escape(first)?;
				self.value.push(c);
			} else if is_word_char(c, first) {
				self.pos += c.len_utf8();
				if self.contains_escape {
					self.value.push(c);
				}
			} else {
				break;
			}
			first = false;
		}
		if !self.contains_escape {
			self.value.push_str(&self.src[start..self.pos]);
		}
		if let Some(keyword) = Keyword::from_word(&self.value) {
			if self.contains_escape {
				return self.error(start, format!("Escape sequence in keyword {}", keyword.as_str()));
			}
			return Ok(TokenKind::Keyword(keyword));
		}
		Ok(TokenKind::Ident)
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

	fn read_private_name(&mut self) -> Result<TokenKind> {
		let start = self.pos;
		self.pos += 1;
		match self.char() {
			Some(c) if c == '\\' || is_word_char(c, true) => {}
			_ => return self.error(start, "Unexpected character '#'"),
		}
		self.read_word()?;
		self.value.insert(0, '#');
		Ok(TokenKind::PrivateName)
	}

	fn read_number(&mut self, starts_with_dot: bool) -> Result<TokenKind> {
		let start = self.pos;
		self.legacy_octal = None;
		if !starts_with_dot && self.byte() == Some(b'0') {
			match self.byte_at(1) {
				Some(b'x' | b'X') => return self.read_radix_number(16),
				Some(b'o' | b'O') => return self.read_radix_number(8),
				Some(b'b' | b'B') => return self.read_radix_number(2),
				_ => {}
			}
		}
		let mut legacy = false;
		if !starts_with_dot {
			legacy = self.byte() == Some(b'0') && self.byte_at(1).is_some_and(|b| b.is_ascii_digit());
			if self.read_int(10, legacy)?.is_none() {
				return self.error(start, "Invalid number");
			}
		}
		let mut octal = false;
		if legacy {
			self.legacy_octal = Some(start as u32);
			octal = self.src[start..self.pos].bytes().all(|b| (b'0'..=b'7').contains(&b));
		}
		if !legacy && !starts_with_dot && self.byte() == Some(b'n') {
			self.pos += 1;
			self.check_after_number()?;
			return Ok(TokenKind::BigInt);
		}
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
		self.number = if octal {
			text.bytes().fold(0.0, |acc, b| acc * 8.0 + (b - b'0') as f64)
		} else if text.contains('_') {
			text.replace('_', "").parse().unwrap()
		} else {
			text.parse().unwrap()
		};
		Ok(TokenKind::Number)
	}

	fn read_radix_number(&mut self, radix: u32) -> Result<TokenKind> {
		let start = self.pos;
		self.pos += 2;
		let Some(value) = self.read_int(radix, false)? else {
			return self.error(start, format!("Expected number in radix {radix}"));
		};
		if self.byte() == Some(b'n') {
			self.pos += 1;
			self.check_after_number()?;
			return Ok(TokenKind::BigInt);
		}
		self.check_after_number()?;
		self.number = value;
		Ok(TokenKind::Number)
	}

	fn read_int(&mut self, radix: u32, legacy_octal: bool) -> Result<Option<f64>> {
		let start = self.pos;
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
			&& (is_word_char(c, true) || c == '\\')
		{
			return self.error(self.pos, "Identifier directly after number");
		}
		Ok(())
	}

	fn read_string(&mut self, quote: u8) -> Result<TokenKind> {
		let start = self.pos;
		self.pos += 1;
		self.value.clear();
		self.cooked_valid = true;
		self.legacy_octal = None;
		let mut chunk_start = self.pos;
		loop {
			let Some(c) = self.char() else {
				return self.error(start, "Unterminated string constant");
			};
			match c {
				_ if c as u32 == quote as u32 => {
					self.value.push_str(&self.src[chunk_start..self.pos]);
					self.pos += 1;
					return Ok(TokenKind::String);
				}
				'\\' => {
					self.value.push_str(&self.src[chunk_start..self.pos]);
					self.pos += 1;
					if let Some(c) = self.read_escape(false)? {
						self.value.push(c);
					}
					chunk_start = self.pos;
				}
				'\n' | '\r' => return self.error(start, "Unterminated string constant"),
				_ => self.pos += c.len_utf8(),
			}
		}
	}

	fn read_escape(&mut self, in_template: bool) -> Result<Option<char>> {
		let Some(c) = self.char() else {
			return self.error(self.pos, "Bad escape sequence");
		};
		let esc_pos = self.pos;
		self.pos += c.len_utf8();
		Ok(Some(match c {
			'n' => '\n',
			'r' => '\r',
			't' => '\t',
			'b' => '\u{8}',
			'v' => '\u{b}',
			'f' => '\u{c}',
			'x' => {
				let Some(v) = self.read_hex(2) else {
					return self.invalid_escape(in_template, esc_pos, "Bad character escape sequence");
				};
				char::from_u32(v).unwrap()
			}
			'u' => match self.read_code_point() {
				Ok(c) => c,
				Err(e) => return self.invalid_escape(in_template, e.pos as usize, e.message),
			},
			'\r' => {
				if self.byte() == Some(b'\n') {
					self.pos += 1;
				}
				return Ok(None);
			}
			'\n' | '\u{2028}' | '\u{2029}' => return Ok(None),
			'8' | '9' => {
				self.legacy_octal.get_or_insert(esc_pos as u32 - 1);
				if in_template {
					return self.invalid_escape(true, esc_pos, "Invalid escape sequence in template string");
				}
				c
			}
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
				if c != '0' || digits > 1 || next_is_89 {
					self.legacy_octal.get_or_insert(esc_pos as u32 - 1);
					if in_template {
						return self.invalid_escape(true, esc_pos, "Octal literal in template string");
					}
				}
				char::from_u32(value).unwrap()
			}
			_ => c,
		}))
	}

	fn invalid_escape(&mut self, in_template: bool, pos: usize, message: impl Into<String>) -> Result<Option<char>> {
		if in_template {
			self.cooked_valid = false;
			Ok(None)
		} else {
			self.error(pos, message)
		}
	}

	fn read_hex(&mut self, len: usize) -> Option<u32> {
		let start = self.pos;
		let mut value = 0;
		for _ in 0..len {
			let digit = (self.byte()? as char).to_digit(16)?;
			value = value * 16 + digit;
			self.pos += 1;
		}
		(self.pos - start == len).then_some(value)
	}

	fn read_code_point(&mut self) -> Result<char> {
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
		Ok(surrogate_pair(self, value).unwrap_or_else(|| char::from_u32(value).unwrap_or('\u{fffd}')))
	}

	/// Reads a template chunk, leaving the position after the closing backquote or `${`.
	pub fn read_template(&mut self) -> Result<Token> {
		let start = self.pos;
		self.value.clear();
		self.cooked_valid = true;
		let mut chunk_start = self.pos;
		loop {
			let Some(c) = self.char() else {
				return self.error(start, "Unterminated template");
			};
			match c {
				'`' | '$' if c == '`' || self.byte_at(1) == Some(b'{') => {
					self.value.push_str(&self.src[chunk_start..self.pos]);
					let end = self.pos;
					self.template_tail = c == '`';
					self.pos += if self.template_tail { 1 } else { 2 };
					return Ok(Token {
						kind: TokenKind::Template,
						start: start as u32,
						end: end as u32,
						newline_before: false,
					});
				}
				'\\' => {
					self.value.push_str(&self.src[chunk_start..self.pos]);
					self.pos += 1;
					if let Some(c) = self.read_escape(true)? {
						self.value.push(c);
					}
					chunk_start = self.pos;
				}
				'\r' => {
					self.value.push_str(&self.src[chunk_start..self.pos]);
					self.value.push('\n');
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

	/// Re-reads a `/` or `/=` token as a regular expression literal.
	pub fn read_regex(&mut self, token: Token) -> Result<Token> {
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
		self.pos += 1;
		self.regex_flags_start = self.pos as u32;
		while let Some(c) = self.char() {
			if c == '\\' {
				return self.error(self.pos, "Invalid regular expression flag");
			}
			if !is_word_char(c, false) {
				break;
			}
			self.pos += c.len_utf8();
		}
		Ok(Token {
			kind: TokenKind::RegExp,
			start: start as u32,
			end: self.pos as u32,
			newline_before: token.newline_before,
		})
	}
}

fn surrogate_pair(lexer: &mut Lexer, high: u32) -> Option<char> {
	if !(0xd800..0xdc00).contains(&high) || !lexer.src[lexer.pos..].starts_with("\\u") {
		return None;
	}
	let save = lexer.pos;
	lexer.pos += 2;
	if let Some(low) = lexer.read_hex(4)
		&& (0xdc00..0xe000).contains(&low)
	{
		return char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00));
	}
	lexer.pos = save;
	None
}

fn is_word_char(c: char, first: bool) -> bool {
	match c {
		'$' | '_' => true,
		'\u{200c}' | '\u{200d}' => !first,
		_ if first => is_id_start(c),
		_ => is_id_continue(c),
	}
}

pub fn is_new_line(c: char) -> bool {
	matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn is_whitespace(c: char) -> bool {
	matches!(
		c,
		'\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use TokenKind::*;

	fn lex(src: &str) -> Vec<(TokenKind, &str)> {
		let mut lexer = Lexer::new(src);
		let mut out = Vec::new();
		loop {
			let token = lexer.next_token().unwrap();
			if token.kind == Eof {
				return out;
			}
			out.push((token.kind, &src[token.start as usize..token.end as usize]));
		}
	}

	fn kinds(src: &str) -> Vec<TokenKind> {
		lex(src).into_iter().map(|(k, _)| k).collect()
	}

	fn error(src: &str) -> SyntaxError {
		let mut lexer = Lexer::new(src);
		loop {
			match lexer.next_token() {
				Ok(t) if t.kind == Eof => panic!("no error for {src:?}"),
				Ok(_) => {}
				Err(e) => return e,
			}
		}
	}

	fn number(src: &str) -> f64 {
		let mut lexer = Lexer::new(src);
		assert_eq!(lexer.next_token().unwrap().kind, Number);
		lexer.number
	}

	fn string(src: &str) -> std::string::String {
		let mut lexer = Lexer::new(src);
		assert_eq!(lexer.next_token().unwrap().kind, String);
		lexer.value.clone()
	}

	#[test]
	fn punctuators() {
		assert_eq!(
			kinds("{ } ( ) [ ] ; , : ~ @"),
			[
				BraceL, BraceR, ParenL, ParenR, BracketL, BracketR, Semi, Comma, Colon, Tilde, At
			]
		);
		assert_eq!(
			kinds(". ... ?. ?? ??= ? :"),
			[
				Dot,
				Ellipsis,
				QuestionDot,
				QuestionQuestion,
				QuestionQuestionEq,
				Question,
				Colon
			]
		);
		assert_eq!(
			kinds("= == === => != !== !"),
			[Eq, EqEq, EqEqEq, Arrow, BangEq, BangEqEq, Bang]
		);
		assert_eq!(
			kinds("< << <<= <= > >> >>> >>= >>>= >="),
			[Lt, LtLt, LtLtEq, LtEq, Gt, GtGt, GtGtGt, GtGtEq, GtGtGtEq, GtEq]
		);
		assert_eq!(
			kinds("+ ++ += - -- -= * ** *= **= / /= % %="),
			[
				Plus, PlusPlus, PlusEq, Minus, MinusMinus, MinusEq, Star, StarStar, StarEq, StarStarEq, Slash, SlashEq,
				Percent, PercentEq
			]
		);
		assert_eq!(
			kinds("& && &= &&= | || |= ||= ^ ^="),
			[
				Amp, AmpAmp, AmpEq, AmpAmpEq, Pipe, PipePipe, PipeEq, PipePipeEq, Caret, CaretEq
			]
		);
	}

	#[test]
	fn longest_match_without_spaces() {
		assert_eq!(kinds("a>>>=b"), [Ident, GtGtGtEq, Ident]);
		assert_eq!(kinds("a**=b"), [Ident, StarStarEq, Ident]);
		assert_eq!(kinds("a?.b"), [Ident, QuestionDot, Ident]);
		assert_eq!(kinds("a?.5:b"), [Ident, Question, Number, Colon, Ident]);
		assert_eq!(kinds("...a"), [Ellipsis, Ident]);
		assert_eq!(kinds("..a"), [Dot, Dot, Ident]);
	}

	#[test]
	fn words_and_keywords() {
		assert_eq!(
			lex("let x = await y"),
			[(Ident, "let"), (Ident, "x"), (Eq, "="), (Ident, "await"), (Ident, "y")]
		);
		assert_eq!(
			kinds("if else while function class"),
			[
				Keyword(crate::Keyword::If),
				Keyword(crate::Keyword::Else),
				Keyword(crate::Keyword::While),
				Keyword(crate::Keyword::Function),
				Keyword(crate::Keyword::Class)
			]
		);
		assert_eq!(kinds("$foo _bar a1 ünïcödé 変数"), [Ident, Ident, Ident, Ident, Ident]);
		assert_eq!(lex("a\u{200d}b"), [(Ident, "a\u{200d}b")]);
	}

	#[test]
	fn escaped_identifiers() {
		let mut lexer = Lexer::new("\\u0061bc \\u{62}c");
		assert_eq!(lexer.next_token().unwrap().kind, Ident);
		assert_eq!(lexer.value, "abc");
		assert!(lexer.contains_escape);
		assert_eq!(lexer.next_token().unwrap().kind, Ident);
		assert_eq!(lexer.value, "bc");
		assert_eq!(error("\\u0069f").message, "Escape sequence in keyword if");
		assert_eq!(error("\\u0031a").message, "Invalid Unicode escape");
		assert_eq!(error("a\\x").message, "Expecting Unicode escape sequence \\uXXXX");
	}

	#[test]
	fn private_names() {
		let mut lexer = Lexer::new("#foo");
		assert_eq!(lexer.next_token().unwrap().kind, PrivateName);
		assert_eq!(lexer.value, "#foo");
		assert_eq!(error("# a").message, "Unexpected character '#'");
	}

	#[test]
	fn hashbang() {
		assert_eq!(lex("#!/usr/bin/env node\nfoo"), [(Ident, "foo")]);
		let mut lexer = Lexer::new("#!/x");
		lexer.next_token().unwrap();
		assert!(lexer.comments.is_empty());
	}

	#[test]
	fn numbers() {
		assert_eq!(number("42"), 42.0);
		assert_eq!(number("2.75"), 2.75);
		assert_eq!(number(".5"), 0.5);
		assert_eq!(number("5."), 5.0);
		assert_eq!(number("1e3"), 1000.0);
		assert_eq!(number("1.5E-2"), 0.015);
		assert_eq!(number("0xff"), 255.0);
		assert_eq!(number("0o17"), 15.0);
		assert_eq!(number("0B101"), 5.0);
		assert_eq!(number("1_000_000"), 1_000_000.0);
		assert_eq!(number("0x1_f"), 31.0);
		assert_eq!(kinds("10n 0xFFn 0n"), [BigInt, BigInt, BigInt]);
	}

	#[test]
	fn legacy_octal_numbers() {
		let mut lexer = Lexer::new("017");
		assert_eq!(lexer.next_token().unwrap().kind, Number);
		assert_eq!(lexer.number, 15.0);
		assert_eq!(lexer.legacy_octal, Some(0));
		let mut lexer = Lexer::new("08.5");
		assert_eq!(lexer.next_token().unwrap().kind, Number);
		assert_eq!(lexer.number, 8.5);
		assert_eq!(lexer.legacy_octal, Some(0));
		assert_eq!(error("017n").message, "Identifier directly after number");
		assert_eq!(
			error("01_7").message,
			"Numeric separator is not allowed in legacy octal numeric literals"
		);
	}

	#[test]
	fn number_errors() {
		assert_eq!(error("1a").message, "Identifier directly after number");
		assert_eq!(
			error("1__0").message,
			"Numeric separator must be exactly one underscore"
		);
		assert_eq!(
			error("1_").message,
			"Numeric separator is not allowed at the last of digits"
		);
		assert_eq!(
			error("0x_1").message,
			"Numeric separator is not allowed at the first of digits"
		);
		assert_eq!(error("0x").message, "Expected number in radix 16");
		assert_eq!(error("1e").message, "Invalid number");
		assert_eq!(error("1.5n").message, "Identifier directly after number");
	}

	#[test]
	fn strings() {
		assert_eq!(string("'hello'"), "hello");
		assert_eq!(string("\"it's\""), "it's");
		assert_eq!(string("'a\\nb\\tc'"), "a\nb\tc");
		assert_eq!(string("'\\x41\\u0042\\u{43}'"), "ABC");
		assert_eq!(string("'\\ud83d\\ude00'"), "😀");
		assert_eq!(string("'a\\\nb'"), "ab");
		assert_eq!(string("'a\\\r\nb'"), "ab");
		assert_eq!(string("'\\0'"), "\0");
		assert_eq!(string("'\\q'"), "q");
		assert_eq!(string("'\u{2028}'"), "\u{2028}");
		assert_eq!(error("'abc").message, "Unterminated string constant");
		assert_eq!(error("'a\nb'").message, "Unterminated string constant");
		assert_eq!(error("'\\x4'").message, "Bad character escape sequence");
		assert_eq!(error("'\\u{110000}'").message, "Code point out of bounds");
	}

	#[test]
	fn legacy_octal_escapes() {
		let mut lexer = Lexer::new("'\\101'");
		lexer.next_token().unwrap();
		assert_eq!(lexer.value, "A");
		assert_eq!(lexer.legacy_octal, Some(1));
		let mut lexer = Lexer::new("'\\0'");
		lexer.next_token().unwrap();
		assert_eq!(lexer.legacy_octal, None);
		let mut lexer = Lexer::new("'\\08'");
		lexer.next_token().unwrap();
		assert_eq!(lexer.value, "\08");
		assert_eq!(lexer.legacy_octal, Some(1));
		let mut lexer = Lexer::new("'\\8'");
		lexer.next_token().unwrap();
		assert_eq!(lexer.value, "8");
		assert_eq!(lexer.legacy_octal, Some(1));
	}

	#[test]
	fn templates() {
		let mut lexer = Lexer::new("`a${b}c\\n`");
		assert_eq!(lexer.next_token().unwrap().kind, Backquote);
		let chunk = lexer.read_template().unwrap();
		assert_eq!((chunk.start, chunk.end), (1, 2));
		assert_eq!(lexer.value, "a");
		assert!(!lexer.template_tail);
		assert_eq!(lexer.next_token().unwrap().kind, Ident);
		assert_eq!(lexer.next_token().unwrap().kind, BraceR);
		let chunk = lexer.read_template().unwrap();
		assert_eq!((chunk.start, chunk.end), (6, 9));
		assert_eq!(lexer.value, "c\n");
		assert!(lexer.template_tail);
		assert_eq!(lexer.next_token().unwrap().kind, Eof);
	}

	#[test]
	fn template_invalid_escapes_cook_to_nothing() {
		let mut lexer = Lexer::new("`\\unicode`");
		lexer.next_token().unwrap();
		lexer.read_template().unwrap();
		assert!(!lexer.cooked_valid);
		assert!(lexer.template_tail);
		let mut lexer = Lexer::new("`\\1`");
		lexer.next_token().unwrap();
		lexer.read_template().unwrap();
		assert!(!lexer.cooked_valid);
	}

	#[test]
	fn template_newlines_normalise() {
		let mut lexer = Lexer::new("`a\r\nb`");
		lexer.next_token().unwrap();
		lexer.read_template().unwrap();
		assert_eq!(lexer.value, "a\nb");
	}

	#[test]
	fn regex() {
		let mut lexer = Lexer::new("/a[/]b\\/c/gi;");
		let slash = lexer.next_token().unwrap();
		assert_eq!(slash.kind, Slash);
		let re = lexer.read_regex(slash).unwrap();
		assert_eq!(re.kind, RegExp);
		assert_eq!(lexer.slice(re.start + 1, lexer.regex_flags_start - 1), "a[/]b\\/c");
		assert_eq!(lexer.slice(lexer.regex_flags_start, re.end), "gi");
		assert_eq!(lexer.next_token().unwrap().kind, Semi);
		let mut lexer = Lexer::new("/=x/");
		let t = lexer.next_token().unwrap();
		assert_eq!(t.kind, SlashEq);
		assert_eq!(lexer.read_regex(t).unwrap().end, 4);
		let mut lexer = Lexer::new("/abc\n/");
		let t = lexer.next_token().unwrap();
		assert_eq!(
			lexer.read_regex(t).unwrap_err().message,
			"Unterminated regular expression"
		);
	}

	#[test]
	fn comments_are_collected_and_skipped() {
		let mut lexer = Lexer::new("a // one\n/* two\n */ b");
		assert_eq!(lexer.next_token().unwrap().kind, Ident);
		let b = lexer.next_token().unwrap();
		assert_eq!(b.kind, Ident);
		assert!(b.newline_before);
		assert_eq!(
			lexer.comments,
			[
				Comment {
					block: false,
					start: 2,
					end: 8
				},
				Comment {
					block: true,
					start: 9,
					end: 19
				}
			]
		);
		assert_eq!(error("/* x").message, "Unterminated comment");
	}

	#[test]
	fn newline_before() {
		let mut lexer = Lexer::new("a\nb c\u{2028}d");
		assert!(!lexer.next_token().unwrap().newline_before);
		assert!(lexer.next_token().unwrap().newline_before);
		assert!(!lexer.next_token().unwrap().newline_before);
		assert!(lexer.next_token().unwrap().newline_before);
	}

	#[test]
	fn unicode_whitespace() {
		assert_eq!(kinds("a\u{a0}b\u{feff}c\u{3000}d"), [Ident, Ident, Ident, Ident]);
	}

	#[test]
	fn unexpected_characters() {
		assert_eq!(error("a ¬ b").message, "Unexpected character '¬'");
		assert_eq!(error("\\").message, "Expecting Unicode escape sequence \\uXXXX");
	}
}
