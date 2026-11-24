//! What a token's text says, read where a node needs it: a string's value with its escapes, a
//! template chunk cooked and raw, a number, a word written with escapes. The lexer reads the
//! same escapes to reject a bad one where it stands.

use super::unicode::{is_id_continue, is_id_start};
use crate::error::Code;
use crate::interner::{Interner, StrId};

pub(crate) enum Escape {
	Char(char),
	Code(u32),
	/// A legacy octal escape or `\8`, `\9`: the character, where the error would stand, which.
	Octal(char, usize, bool),
	Nothing,
	Invalid(Code, usize),
}

fn hex(bytes: &[u8], pos: usize, len: usize) -> Option<u32> {
	let mut value = 0;
	for i in 0..len {
		let digit = (*bytes.get(pos + i)? as char).to_digit(16)?;
		value = value * 16 + digit;
	}
	Some(value)
}

/// The code point written after `\u`, from `pos`: four hex digits or `{…}`; and where it ends.
pub(crate) fn code_point(bytes: &[u8], pos: usize) -> Result<(u32, usize), (Code, usize)> {
	if bytes.get(pos) == Some(&b'{') {
		let digits = pos + 1;
		let mut at = digits;
		let mut value: u32 = 0;
		while let Some(d) = bytes.get(at).and_then(|&b| (b as char).to_digit(16)) {
			value = value.saturating_mul(16).saturating_add(d);
			at += 1;
		}
		if at == digits || bytes.get(at) != Some(&b'}') {
			return Err((Code::BadCharacterEscape, digits));
		}
		if value > 0x10ffff {
			return Err((Code::CodePointOutOfBounds, digits));
		}
		return Ok((value, at + 1));
	}
	match hex(bytes, pos, 4) {
		Some(v) => Ok((v, pos + 4)),
		None => Err((Code::BadCharacterEscape, pos)),
	}
}

/// The character a word's escape at `pos`, a backslash, spells, and where the escape ends.
pub(crate) fn word_escape(src: &str, pos: usize, first: bool) -> Result<(char, usize), (Code, usize)> {
	let bytes = src.as_bytes();
	if bytes.get(pos + 1) != Some(&b'u') {
		return Err((Code::ExpectedUnicodeEscape, pos + 1));
	}
	let (code, end) = code_point(bytes, pos + 2)?;
	match char::from_u32(code) {
		Some(c) if if first { is_id_start(c) } else { is_id_continue(c) } => Ok((c, end)),
		_ => Err((Code::InvalidUnicodeEscape, pos)),
	}
}

/// The escape after the backslash before `pos`, and where it ends.
pub(crate) fn escape(src: &str, pos: usize) -> (Escape, usize) {
	let bytes = src.as_bytes();
	let backslash = pos - 1;
	let Some(c) = src[pos..].chars().next() else {
		return (Escape::Char('\0'), pos);
	};
	let mut at = pos + c.len_utf8();
	let c = match c {
		'n' => '\n',
		'r' => '\r',
		't' => '\t',
		'b' => '\u{8}',
		'v' => '\u{b}',
		'f' => '\u{c}',
		'x' => {
			return match hex(bytes, at, 2) {
				Some(v) => (Escape::Code(v), at + 2),
				None => (Escape::Invalid(Code::BadCharacterEscape, at), at),
			};
		}
		'u' => {
			return match code_point(bytes, at) {
				Ok((code, end)) => (Escape::Code(code), end),
				Err((code, pos)) => (Escape::Invalid(code, pos), at),
			};
		}
		'\r' => {
			if bytes.get(at) == Some(&b'\n') {
				at += 1;
			}
			return (Escape::Nothing, at);
		}
		'\n' | '\u{2028}' | '\u{2029}' => return (Escape::Nothing, at),
		'8' | '9' => return (Escape::Octal(c, at - 1, true), at),
		'0'..='7' => {
			let mut value = c.to_digit(8).unwrap();
			let mut digits = 1;
			while digits < 3 {
				match bytes.get(at) {
					Some(&b @ b'0'..=b'7') if value * 8 + (b - b'0') as u32 <= 255 => {
						value = value * 8 + (b - b'0') as u32;
						at += 1;
						digits += 1;
					}
					_ => break,
				}
			}
			let next_is_89 = matches!(bytes.get(at), Some(b'8' | b'9'));
			let c = char::from_u32(value).unwrap();
			if c != '\0' || digits > 1 || next_is_89 {
				return (Escape::Octal(c, backslash, false), at);
			}
			c
		}
		_ => c,
	};
	(Escape::Char(c), at)
}

/// A template chunk's values; a chunk with a bad escape has no cooked one.
pub(crate) struct Chunk {
	pub cooked: Option<StrId>,
	pub raw: StrId,
}

/// The text being built, with the lone surrogates it holds.
#[derive(Default)]
pub(crate) struct Values {
	buf: String,
	marks: Vec<(u32, u16)>,
	marked: usize,
}

impl Values {
	fn start(&mut self) {
		self.buf.clear();
		self.marks.clear();
		self.marked = 0;
	}

	fn push_chunk(&mut self, src: &str, from: usize, to: usize, pending: &mut Option<u32>) {
		if from < to {
			self.flush(pending);
			self.buf.push_str(&src[from..to]);
		}
	}

	// surrogates pair across escapes, as JavaScript strings do
	fn push_code(&mut self, code: u32, pending: &mut Option<u32>) {
		if let Some(high) = pending.take() {
			if (0xdc00..0xe000).contains(&code) {
				self.buf
					.push(char::from_u32(0x10000 + ((high - 0xd800) << 10) + (code - 0xdc00)).unwrap());
				return;
			}
			self.push_lone(high);
		}
		if (0xd800..0xdc00).contains(&code) {
			*pending = Some(code);
		} else {
			match char::from_u32(code) {
				Some(c) => self.buf.push(c),
				None => self.push_lone(code),
			}
		}
	}

	fn flush(&mut self, pending: &mut Option<u32>) {
		if let Some(high) = pending.take() {
			self.push_lone(high);
		}
	}

	fn push_lone(&mut self, code: u32) {
		let (bytes, units) = self.marks.last().map_or((0, 0), |&(at, _)| (self.marked, at + 1));
		self.marks
			.push((units + self.buf[bytes..].encode_utf16().count() as u32, code as u16));
		self.buf.push('\u{fffd}');
		self.marked = self.buf.len();
	}

	fn intern_buf(&mut self, strings: &mut Interner) -> StrId {
		if self.marks.is_empty() {
			strings.intern(&self.buf)
		} else {
			strings.intern_marked(&self.buf, &self.marks)
		}
	}

	/// Whether the value read last holds a lone surrogate.
	pub(crate) fn marked(&self) -> bool {
		!self.marks.is_empty()
	}

	/// The value of the string token from `start` to `end`, quotes included, its escapes valid.
	pub(crate) fn string(&mut self, src: &str, start: usize, end: usize, strings: &mut Interner) -> StrId {
		self.start();
		let bytes = src.as_bytes();
		let close = end - 1;
		let mut pending = None;
		let mut pos = start + 1;
		let mut chunk_start = pos;
		while pos < close {
			if bytes[pos] != b'\\' {
				pos += 1;
				continue;
			}
			self.push_chunk(src, chunk_start, pos, &mut pending);
			let (escape, next) = escape(src, pos + 1);
			match escape {
				Escape::Char(c) | Escape::Octal(c, ..) => {
					self.flush(&mut pending);
					self.buf.push(c);
				}
				Escape::Code(code) => self.push_code(code, &mut pending),
				Escape::Nothing | Escape::Invalid(..) => {}
			}
			pos = next;
			chunk_start = pos;
		}
		self.push_chunk(src, chunk_start, close, &mut pending);
		self.flush(&mut pending);
		self.intern_buf(strings)
	}

	/// The chunk of a template from `start` to `end`, neither a delimiter's.
	pub(crate) fn template(&mut self, src: &str, start: usize, end: usize, strings: &mut Interner) -> Chunk {
		self.start();
		let bytes = src.as_bytes();
		let mut bad = false;
		let mut plain = true;
		let mut returns = false;
		let mut pending = None;
		let mut pos = start;
		let mut chunk_start = pos;
		while pos < end {
			match bytes[pos] {
				b'\\' => {
					plain = false;
					self.push_chunk(src, chunk_start, pos, &mut pending);
					pos += 1;
					returns |= bytes.get(pos) == Some(&b'\r');
					let (escape, next) = escape(src, pos);
					match escape {
						Escape::Char(c) => {
							self.flush(&mut pending);
							self.buf.push(c);
						}
						Escape::Code(code) => self.push_code(code, &mut pending),
						Escape::Nothing => {}
						Escape::Octal(..) | Escape::Invalid(..) => bad = true,
					}
					pos = next;
					chunk_start = pos;
				}
				b'\r' => {
					plain = false;
					returns = true;
					self.push_chunk(src, chunk_start, pos, &mut pending);
					self.flush(&mut pending);
					self.buf.push('\n');
					pos += 1;
					if bytes.get(pos) == Some(&b'\n') && pos < end {
						pos += 1;
					}
					chunk_start = pos;
				}
				_ => pos += 1,
			}
		}
		self.push_chunk(src, chunk_start, end.max(chunk_start), &mut pending);
		self.flush(&mut pending);
		let raw_text = &src[start..end];
		let raw = if returns {
			let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
			strings.intern(&normalized)
		} else {
			strings.intern(raw_text)
		};
		let cooked = if bad {
			None
		} else if plain {
			Some(raw)
		} else {
			Some(self.intern_buf(strings))
		};
		Chunk { cooked, raw }
	}

	/// The word from `start` to `end`, its escapes read: they were checked when it was a token.
	pub(crate) fn word<'b>(&'b mut self, src: &str, start: usize, end: usize) -> &'b str {
		self.start();
		let bytes = src.as_bytes();
		let mut pos = start;
		let mut chunk_start = pos;
		while pos < end {
			if bytes[pos] != b'\\' {
				pos += 1;
				continue;
			}
			self.buf.push_str(&src[chunk_start..pos]);
			match code_point(bytes, pos + 2) {
				Ok((code, next)) => {
					self.buf.push(char::from_u32(code).unwrap_or('\u{fffd}'));
					pos = next;
				}
				Err(_) => pos += 1,
			}
			chunk_start = pos;
		}
		self.buf.push_str(&src[chunk_start..end]);
		&self.buf
	}
}

/// What the number token `text` is worth.
pub(crate) fn number(text: &str) -> f64 {
	let bytes = text.as_bytes();
	if bytes.len() > 1 && bytes[0] == b'0' {
		let radix = match bytes[1] {
			b'x' | b'X' => 16,
			b'o' | b'O' => 8,
			b'b' | b'B' => 2,
			_ => 0,
		};
		if radix != 0 {
			let digit = |b: u8| (b as char).to_digit(radix).unwrap() as f64;
			return bytes[2..]
				.iter()
				.filter(|&&b| b != b'_')
				.fold(0.0, |total, &b| total * radix as f64 + digit(b));
		}
		if bytes.iter().all(|b| (b'0'..=b'7').contains(b)) {
			return match u128::from_str_radix(text, 8) {
				Ok(v) => v as f64,
				Err(_) => bytes.iter().fold(0.0, |acc, b| acc * 8.0 + (b - b'0') as f64),
			};
		}
	}
	if text.contains('_') {
		let plain: String = text.chars().filter(|c| *c != '_').collect();
		plain.parse().unwrap()
	} else {
		text.parse().unwrap()
	}
}
