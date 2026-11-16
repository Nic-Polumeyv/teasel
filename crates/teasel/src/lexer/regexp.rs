//! Checks a regular expression literal's pattern and flags against ECMA-262's early errors
//! (22.2.1, with Annex B.1.2). One walk over the pattern's bytes; nothing is built.

use super::unicode::{is_id_continue, is_id_start};
use crate::error::Code;
use crate::interner::FastMap;

type Result<T> = std::result::Result<T, Rejected>;

/// What is wrong with a literal and where, as offsets from the start of its pattern: the flags
/// start one past its end.
pub(super) struct Rejected {
	pub code: Code,
	pub at: usize,
	pub end: usize,
	/// The code is about a name, a number or a modifier: `Scratch::name` holds it.
	pub named: bool,
}

/// `named`: the pattern holds a `(?<` that is not a lookbehind's, outside every class.
pub(super) fn validate(pattern: &str, flags: &str, named: bool, scratch: &mut Scratch) -> Result<()> {
	let mut seen = 0u8;
	for (at, flag) in flags.char_indices() {
		let bit = b"gimsyduv".iter().position(|&known| known as char == flag);
		let code = match bit {
			None => Code::InvalidRegexpFlag,
			Some(bit) if seen & 1 << bit != 0 => Code::DuplicateRegexpFlag,
			Some(6 | 7) if seen >> 6 != 0 => Code::InvalidRegexpFlag,
			Some(bit) => {
				seen |= 1 << bit;
				continue;
			}
		};
		let at = pattern.len() + 1 + at;
		return Err(Rejected {
			code,
			at,
			end: at + flag.len_utf8(),
			named: false,
		});
	}
	let unicode = seen >> 6 != 0;
	let mut walk = Walk {
		text: pattern,
		pos: 0,
		unicode,
		sets: seen >> 7 != 0,
		named: named || unicode,
		captures: 0,
		reference: (0, 0, 0),
		trail: 0,
		scratch,
	};
	let result = walk.pattern();
	scratch.groups.clear();
	scratch.classes.clear();
	if !scratch.names.is_empty() {
		scratch.names.clear();
	}
	result
}

/// What a walk fills and the next one reuses.
#[derive(Default)]
pub(crate) struct Scratch {
	groups: Vec<Group>,
	classes: Vec<Class>,
	names: FastMap<String, Name>,
	name: String,
}

impl Scratch {
	/// What the last error with `named` is about.
	pub(super) fn name(&self) -> &str {
		&self.name
	}
}

const NONE: u32 = u32::MAX;
/// A class atom that is a set of characters, `\d` or `\p{…}`: no end of a range.
const SET: u32 = u32::MAX;

/// An open group; the first is the pattern itself.
#[derive(Clone, Copy)]
struct Group {
	/// The offset after its `(`.
	open: u32,
	/// Where its current alternative starts.
	alternative: u32,
	kind: Kind,
}

#[derive(Clone, Copy)]
enum Kind {
	Group,
	Ahead,
	Behind,
}

/// A group name: the `(` of the last group of the name, and the span of the first `\k` that asks
/// for it.
struct Name {
	declared: u32,
	referenced: (u32, u32),
}

/// An open class of a pattern with the `v` flag.
#[derive(Clone, Copy)]
struct Class {
	at: u32,
	negated: bool,
	/// What it holds so far may match more than one character.
	strings: bool,
	state: Sets,
}

/// What a class with the `v` flag has read: nothing, one operand, a union, or an intersection or
/// a subtraction after an operand or after its operator.
#[derive(Clone, Copy)]
enum Sets {
	Empty,
	One,
	Union,
	And,
	AndNext,
	Minus,
	MinusNext,
}

/// The name's entry, made when it is new.
fn known<'n>(names: &'n mut FastMap<String, Name>, name: &str) -> &'n mut Name {
	if !names.contains_key(name) {
		let unknown = Name {
			declared: NONE,
			referenced: (NONE, NONE),
		};
		names.insert(name.to_owned(), unknown);
	}
	names.get_mut(name).unwrap()
}

struct Walk<'a> {
	text: &'a str,
	pos: usize,
	unicode: bool,
	sets: bool,
	/// `\k` is a reference to a name, not an escaped `k`.
	named: bool,
	captures: u32,
	/// The largest `\1` to `\9…` of a unicode pattern, and its span.
	reference: (u32, usize, usize),
	/// Without `u` an astral character is two class atoms: the trail surrogate of the one whose
	/// lead was just read, the next atom.
	trail: u32,
	scratch: &'a mut Scratch,
}

impl<'a> Walk<'a> {
	/// `code` for what stands from `at` to where the walk is: one character at least, and an
	/// escape with the character it escapes.
	fn fail<T>(&self, code: Code, at: usize) -> Result<T> {
		let escape = self.text[at..].starts_with('\\') as usize;
		let mut end = self.pos.max(at + 1 + escape).min(self.text.len());
		while !self.text.is_char_boundary(end) {
			end += 1;
		}
		self.fail_at(code, at, end, false)
	}

	fn fail_at<T>(&self, code: Code, at: usize, end: usize, named: bool) -> Result<T> {
		Err(Rejected { code, at, end, named })
	}

	fn peek(&self) -> Option<u8> {
		self.text.as_bytes().get(self.pos).copied()
	}

	fn eat(&mut self, byte: u8) -> bool {
		let eaten = self.peek() == Some(byte);
		self.pos += eaten as usize;
		eaten
	}

	fn char(&self) -> Option<char> {
		self.text[self.pos..].chars().next()
	}

	fn pattern(&mut self) -> Result<()> {
		let bytes = self.text.as_bytes();
		self.scratch.groups.push(Group {
			open: 0,
			alternative: 0,
			kind: Kind::Group,
		});
		// whether what was just read takes a quantifier
		let mut repeats = false;
		while let Some(&byte) = bytes.get(self.pos) {
			let at = self.pos;
			self.pos += 1;
			repeats = match byte {
				b'^' | b'$' => false,
				b'|' => {
					self.scratch.groups.last_mut().unwrap().alternative = self.pos as u32;
					false
				}
				b'(' => {
					self.open(at)?;
					false
				}
				b')' => {
					let group = self.scratch.groups.pop().unwrap();
					if self.scratch.groups.is_empty() {
						return self.fail(Code::UnopenedRegexpGroup, at);
					}
					match group.kind {
						Kind::Group => true,
						Kind::Ahead => !self.unicode,
						Kind::Behind => false,
					}
				}
				b'[' if self.sets => {
					self.class_sets(at)?;
					true
				}
				b'[' => {
					self.class(at)?;
					true
				}
				b'*' | b'+' | b'?' if repeats => {
					self.eat(b'?');
					false
				}
				b'*' | b'+' | b'?' => return self.fail(Code::LoneRegexpQuantifier, at),
				b'{' => match self.braces() {
					Some(_) if !repeats => return self.fail(Code::LoneRegexpQuantifier, at),
					Some(false) => return self.fail(Code::RegexpQuantifierOrder, at),
					Some(true) => {
						self.eat(b'?');
						false
					}
					None if self.unicode => return self.fail(Code::LoneRegexpBracket, at),
					None => true,
				},
				b'}' | b']' if self.unicode => return self.fail(Code::LoneRegexpBracket, at),
				b'\\' => self.escape(at)?,
				_ => true,
			};
		}
		if let [_, .., unclosed] = self.scratch.groups[..] {
			let open = unclosed.open as usize;
			return self.fail_at(Code::UnterminatedRegexpGroup, open - 1, open, false);
		}
		let Scratch { names, name, .. } = &mut *self.scratch;
		let (number, at, end) = self.reference;
		if number > self.captures {
			name.clear();
			name.push_str(&self.text[at + 1..end]);
			return self.fail_at(Code::UndefinedRegexpGroup, at, end, true);
		}
		let undeclared = names.iter().filter(|(_, known)| known.declared == NONE);
		match undeclared.min_by_key(|(_, known)| known.referenced) {
			Some((
				unknown,
				&Name {
					referenced: (at, end), ..
				},
			)) => {
				name.clear();
				name.push_str(unknown);
				self.fail_at(Code::UndefinedRegexpGroup, at as usize, end as usize, true)
			}
			None => Ok(()),
		}
	}

	/// Opens the group whose `(` at `at` was just passed.
	fn open(&mut self, at: usize) -> Result<()> {
		let mut kind = Kind::Group;
		if !self.eat(b'?') {
			self.captures += 1;
		} else if self.eat(b'=') || self.eat(b'!') {
			kind = Kind::Ahead;
		} else if !self.eat(b'<') {
			let add = self.modifiers(0)?;
			let remove = if self.eat(b'-') {
				Some(self.modifiers(add)?)
			} else {
				None
			};
			if !self.eat(b':') {
				return self.fail(Code::InvalidRegexpGroup, at);
			}
			if add == 0 && remove == Some(0) {
				return self.fail(Code::InvalidRegexpGroup, at);
			}
		} else if self.eat(b'=') || self.eat(b'!') {
			kind = Kind::Behind;
		} else {
			self.name(at)?;
			self.captures += 1;
			let Scratch {
				groups, names, name, ..
			} = &mut *self.scratch;
			let known = known(names, name);
			// two groups of one name are alternatives, never both matched, only when the earlier
			// one stands in an earlier alternative of the deepest open group that holds it; every
			// still earlier group of the name then does too
			let holder = groups[groups.partition_point(|group| group.open <= known.declared) - 1];
			if known.declared != NONE && known.declared >= holder.alternative {
				return self.fail_at(Code::DuplicateRegexpGroupName, at, self.pos, true);
			}
			known.declared = at as u32;
		}
		let open = at as u32 + 1;
		self.scratch.groups.push(Group {
			open,
			alternative: open,
			kind,
		});
		Ok(())
	}

	/// The `i`, `m` and `s` here as bits, none of them twice or among `taken`.
	fn modifiers(&mut self, taken: u8) -> Result<u8> {
		let mut seen = 0;
		while let Some(bit) = self.peek().and_then(|byte| b"ims".iter().position(|&m| m == byte)) {
			if (seen | taken) & 1 << bit != 0 {
				self.scratch.name.clear();
				self.scratch.name.push(b"ims"[bit] as char);
				return self.fail_at(Code::DuplicateRegexpModifier, self.pos, self.pos + 1, true);
			}
			seen |= 1 << bit;
			self.pos += 1;
		}
		Ok(seen)
	}

	/// Reads a group name through its `>` into the scratch.
	fn name(&mut self, at: usize) -> Result<()> {
		self.scratch.name.clear();
		loop {
			let Some(mut c) = self.char() else {
				return self.fail(Code::InvalidRegexpGroupName, at);
			};
			self.pos += c.len_utf8();
			if c == '>' && !self.scratch.name.is_empty() {
				return Ok(());
			}
			if c == '\\' && self.eat(b'u') {
				let code = self.unicode_escape(at, true).ok().flatten();
				c = code.and_then(char::from_u32).unwrap_or(c);
			}
			let fits = if self.scratch.name.is_empty() {
				is_id_start(c)
			} else {
				is_id_continue(c)
			};
			if !fits {
				return self.fail(Code::InvalidRegexpGroupName, at);
			}
			self.scratch.name.push(c);
		}
	}

	/// The escape whose `\` at `at` was just passed, outside a class; answers whether it takes a
	/// quantifier.
	fn escape(&mut self, at: usize) -> Result<bool> {
		let Some(byte) = self.peek() else {
			return if self.unicode {
				self.fail(Code::InvalidRegexpEscape, at)
			} else {
				Ok(true)
			};
		};
		self.pos += 1;
		match byte {
			b'b' | b'B' => return Ok(false),
			b'd' | b'D' | b's' | b'S' | b'w' | b'W' | b't' | b'n' | b'v' | b'f' | b'r' => {}
			b'c' if self.peek().is_some_and(|letter| letter.is_ascii_alphabetic()) => self.pos += 1,
			b'c' if self.unicode => return self.fail(Code::InvalidRegexpEscape, at),
			b'x' => {
				if self.hex(2).is_none() && self.unicode {
					return self.fail(Code::InvalidRegexpEscape, at);
				}
			}
			b'u' => {
				self.unicode_escape(at, self.unicode)?;
			}
			b'k' if self.named => {
				if !self.eat(b'<') {
					return self.fail(Code::InvalidRegexpGroupName, at);
				}
				self.name(at)?;
				let Scratch { names, name, .. } = &mut *self.scratch;
				let known = known(names, name);
				if known.referenced.0 == NONE {
					known.referenced = (at as u32, self.pos as u32);
				}
			}
			// a back reference or an octal escape, and a `k`, a `p` or any other character escaping
			// itself: nothing of them is an error, whatever follows is read as itself
			_ if !self.unicode => {}
			b'1'..=b'9' => {
				let mut number = (byte - b'0') as u32;
				while let Some(digit @ b'0'..=b'9') = self.peek() {
					number = number.saturating_mul(10).saturating_add((digit - b'0') as u32);
					self.pos += 1;
				}
				if number > self.reference.0 {
					self.reference = (number, at, self.pos);
				}
			}
			b'0' if !self.peek().is_some_and(|digit| digit.is_ascii_digit()) => {}
			b'p' | b'P' => {
				self.property(at, byte == b'P')?;
			}
			b'^' | b'$' | b'\\' | b'.' | b'*' | b'+' | b'?' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' | b'/' => {
			}
			_ => return self.fail(Code::InvalidRegexpEscape, at),
		}
		Ok(true)
	}

	/// The `{n}`, `{n,}` or `{n,m}` whose `{` was just passed, as whether its numbers are in
	/// order; None, the cursor unmoved, when it is none of them.
	fn braces(&mut self) -> Option<bool> {
		let start = self.pos;
		let low = self.digits();
		let high = if self.eat(b',') { self.digits() } else { "" };
		if low.is_empty() || !self.eat(b'}') {
			self.pos = start;
			return None;
		}
		// compared as written: the numbers have any length
		let number = |digits: &'a str| {
			let digits = digits.trim_start_matches('0');
			(digits.len(), digits)
		};
		Some(high.is_empty() || number(low) <= number(high))
	}

	fn digits(&mut self) -> &'a str {
		let start = self.pos;
		while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
			self.pos += 1;
		}
		&self.text[start..self.pos]
	}

	fn hex(&mut self, length: usize) -> Option<u32> {
		let digits = self.text.as_bytes().get(self.pos..self.pos + length)?;
		let mut value = 0;
		for &digit in digits {
			value = value * 16 + (digit as char).to_digit(16)?;
		}
		self.pos += length;
		Some(value)
	}

	/// The code of the escape whose `\u` was just passed. With `unicode`, two escapes that are a
	/// surrogate pair are one code point, `\u{…}` is read and anything else is an error; without,
	/// None is a `u` escaping itself.
	fn unicode_escape(&mut self, at: usize, unicode: bool) -> Result<Option<u32>> {
		if let Some(lead) = self.hex(4) {
			if unicode && (0xd800..0xdc00).contains(&lead) && self.text[self.pos..].starts_with("\\u") {
				let after = self.pos;
				self.pos += 2;
				match self.hex(4) {
					Some(trail @ 0xdc00..0xe000) => {
						return Ok(Some(0x10000 + ((lead - 0xd800) << 10) + (trail - 0xdc00)));
					}
					_ => self.pos = after,
				}
			}
			return Ok(Some(lead));
		}
		if !unicode {
			return Ok(None);
		}
		if self.eat(b'{') {
			let digits = self.pos;
			let mut value = 0u32;
			while let Some(digit) = self.peek().and_then(|byte| (byte as char).to_digit(16)) {
				value = value.saturating_mul(16).saturating_add(digit);
				self.pos += 1;
			}
			if self.pos > digits && value <= 0x10ffff && self.eat(b'}') {
				return Ok(Some(value));
			}
		}
		self.fail(Code::InvalidRegexpEscape, at)
	}

	/// A `\p` or `\P` after its letter; answers whether it may match more than one character.
	fn property(&mut self, at: usize, negated: bool) -> Result<bool> {
		let listed = |list: &str, name: &str| list.split(' ').any(|known| known == name);
		if !self.eat(b'{') {
			return self.fail(Code::InvalidRegexpProperty, at);
		}
		let name = self.word();
		let strings = if self.eat(b'=') {
			let values = match name {
				"General_Category" | "gc" => GENERAL_CATEGORIES,
				"Script" | "sc" | "Script_Extensions" | "scx" => SCRIPTS,
				_ => return self.fail(Code::InvalidRegexpProperty, at),
			};
			if !listed(values, self.word()) {
				return self.fail(Code::InvalidRegexpProperty, at);
			}
			false
		} else if listed(BINARY, name) || listed(GENERAL_CATEGORIES, name) {
			false
		} else if self.sets && listed(STRINGS, name) {
			true
		} else {
			return self.fail(Code::InvalidRegexpProperty, at);
		};
		if !self.eat(b'}') || negated && strings {
			return self.fail(Code::InvalidRegexpProperty, at);
		}
		Ok(strings)
	}

	fn word(&mut self) -> &'a str {
		let start = self.pos;
		while self
			.peek()
			.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
		{
			self.pos += 1;
		}
		&self.text[start..self.pos]
	}

	/// The class whose `[` at `at` was just passed, of a pattern without the `v` flag.
	fn class(&mut self, at: usize) -> Result<()> {
		self.eat(b'^');
		let mut from = self.pos;
		loop {
			// a trail surrogate starts where its character does
			if self.trail == 0 {
				from = self.pos;
			}
			let Some(left) = self.class_atom(at)? else {
				return Ok(());
			};
			if self.trail != 0 || !self.eat(b'-') {
				continue;
			}
			let Some(right) = self.class_atom(at)? else {
				return Ok(());
			};
			if self.unicode && (left == SET || right == SET) {
				return self.fail(Code::InvalidRegexpClass, from);
			}
			if left != SET && right != SET && left > right {
				return self.fail(Code::RegexpRangeOrder, from);
			}
		}
	}

	/// The next atom of a class as its code, or `SET`; None past the `]` that closes the class.
	fn class_atom(&mut self, at: usize) -> Result<Option<u32>> {
		if self.trail != 0 {
			return Ok(Some(std::mem::take(&mut self.trail)));
		}
		let Some(c) = self.char() else {
			return self.fail_at(Code::InvalidRegexpClass, at, at + 1, false);
		};
		let from = self.pos;
		self.pos += c.len_utf8();
		if c != '\\' {
			return Ok((c != ']').then(|| self.unit(c)));
		}
		let value = match self.peek() {
			Some(b'b') => 8,
			Some(b'-') if self.unicode => b'-' as u32,
			Some(b'd' | b'D' | b's' | b'S' | b'w' | b'W') => SET,
			Some(letter @ (b'p' | b'P')) if self.unicode => {
				self.pos += 1;
				self.property(from, letter == b'P')?;
				return Ok(Some(SET));
			}
			Some(b'c')
				if !self.unicode && matches!(self.text.as_bytes().get(self.pos + 1), Some(b'0'..=b'9' | b'_')) =>
			{
				self.pos += 1;
				(self.text.as_bytes()[self.pos] % 32) as u32
			}
			letter => {
				return match self.character(from)? {
					Some(value) => Ok(Some(value)),
					None if self.unicode || letter == Some(b'k') => self.fail(Code::InvalidRegexpEscape, from),
					// a `\c` no control letter follows: the `\` is the atom
					None => Ok(Some('\\' as u32)),
				};
			}
		};
		self.pos += 1;
		Ok(Some(value))
	}

	/// A character as a class atom: without `u` an astral one is its lead surrogate.
	fn unit(&mut self, c: char) -> u32 {
		let code = c as u32;
		if code < 0x10000 || self.unicode {
			return code;
		}
		self.trail = 0xdc00 + (code & 0x3ff);
		0xd800 + ((code - 0x10000) >> 10)
	}

	/// The character escape after the `\` at `at`, as its code; None, the cursor unmoved, when
	/// what follows is none.
	fn character(&mut self, at: usize) -> Result<Option<u32>> {
		let Some(c) = self.char() else {
			return Ok(None);
		};
		let start = self.pos;
		self.pos += c.len_utf8();
		Ok(Some(match c {
			't' => 9,
			'n' => 10,
			'v' => 11,
			'f' => 12,
			'r' => 13,
			'c' if self.peek().is_some_and(|letter| letter.is_ascii_alphabetic()) => {
				self.pos += 1;
				(self.text.as_bytes()[self.pos - 1] % 32) as u32
			}
			'0'..='7' if !self.unicode => {
				let mut value = c as u32 - '0' as u32;
				for _ in 0..if value <= 3 { 2 } else { 1 } {
					let Some(digit @ b'0'..=b'7') = self.peek() else { break };
					value = value * 8 + (digit - b'0') as u32;
					self.pos += 1;
				}
				value
			}
			'0' if !self.peek().is_some_and(|digit| digit.is_ascii_digit()) => 0,
			'x' => match self.hex(2) {
				Some(value) => value,
				None if self.unicode => return self.fail(Code::InvalidRegexpEscape, at),
				None => 'x' as u32,
			},
			'u' => self.unicode_escape(at, self.unicode)?.unwrap_or('u' as u32),
			_ if !self.unicode && c != 'c' && !(c == 'k' && self.named) => self.unit(c),
			'^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/'
				if self.unicode =>
			{
				c as u32
			}
			_ => {
				self.pos = start;
				return Ok(None);
			}
		}))
	}

	/// The class whose `[` at `at` was just passed, of a pattern with the `v` flag, and the
	/// classes nested in it.
	fn class_sets(&mut self, at: usize) -> Result<()> {
		use Sets::*;
		let mut class = Class {
			at: at as u32,
			negated: self.eat(b'^'),
			strings: false,
			state: Empty,
		};
		loop {
			let from = self.pos;
			let strings = match self.text.as_bytes()[from..] {
				[] => {
					return self.fail_at(
						Code::InvalidRegexpClass,
						class.at as usize,
						class.at as usize + 1,
						false,
					);
				}
				[b']', ..] => {
					self.pos += 1;
					if matches!(class.state, AndNext | MinusNext) {
						return self.fail(Code::InvalidRegexpClass, from);
					}
					if class.negated && class.strings {
						return self.fail(Code::InvalidRegexpClass, class.at as usize);
					}
					let Some(outer) = self.scratch.classes.pop() else {
						return Ok(());
					};
					let strings = class.strings;
					class = outer;
					strings
				}
				[b'&', b'&', ..] | [b'-', b'-', ..] => {
					self.pos += 2;
					class.state = match (class.state, self.text.as_bytes()[from]) {
						(One | And, b'&') if self.peek() != Some(b'&') => AndNext,
						(One | Minus, b'-') => MinusNext,
						_ => return self.fail(Code::InvalidRegexpClass, from),
					};
					continue;
				}
				_ if matches!(class.state, And | Minus) => {
					return self.fail(Code::InvalidRegexpClass, from);
				}
				[b'[', ..] => {
					self.pos += 1;
					self.scratch.classes.push(class);
					class = Class {
						at: from as u32,
						negated: self.eat(b'^'),
						strings: false,
						state: Empty,
					};
					continue;
				}
				_ => match self.set_character()? {
					Some(left) => {
						if matches!(class.state, Empty | One | Union) && self.eat(b'-') {
							match self.set_character()? {
								Some(right) if left > right => {
									return self.fail(Code::RegexpRangeOrder, from);
								}
								Some(_) => {
									class.state = Union;
									continue;
								}
								None => self.pos -= 1,
							}
						}
						false
					}
					None => self.set_operand(from)?,
				},
			};
			(class.strings, class.state) = match class.state {
				Empty => (strings, One),
				One | Union => (class.strings | strings, Union),
				AndNext => (class.strings & strings, And),
				_ => (class.strings, Minus),
			};
		}
	}

	/// A character of a class with the `v` flag as its code; None, the cursor unmoved, at
	/// anything else.
	fn set_character(&mut self) -> Result<Option<u32>> {
		let start = self.pos;
		let Some(c) = self.char() else {
			return Ok(None);
		};
		if c == '\\' {
			self.pos += 1;
			if let Some(value) = self.character(start)? {
				return Ok(Some(value));
			}
			return Ok(match self.peek() {
				Some(b'b') => {
					self.pos += 1;
					Some(8)
				}
				Some(
					byte @ (b'&' | b'-' | b'!' | b'#' | b'%' | b',' | b':' | b';' | b'<' | b'=' | b'>' | b'@' | b'`'
					| b'~'),
				) => {
					self.pos += 1;
					Some(byte as u32)
				}
				_ => {
					self.pos = start;
					None
				}
			});
		}
		let doubled = "!#$%&*+,.:;<=>?@^`~".contains(c) && self.text[start + 1..].starts_with(c);
		if doubled || "()-/[]{}|".contains(c) {
			return Ok(None);
		}
		self.pos += c.len_utf8();
		Ok(Some(c as u32))
	}

	/// An operand of a class with the `v` flag that is no character and no class: `\q{…}` or a
	/// class escape; answers whether it may match more than one character.
	fn set_operand(&mut self, at: usize) -> Result<bool> {
		if !self.eat(b'\\') {
			return self.fail(Code::InvalidRegexpClass, at);
		}
		let letter = self.peek();
		self.pos += 1;
		match letter {
			Some(b'd' | b'D' | b's' | b'S' | b'w' | b'W') => Ok(false),
			Some(b'p' | b'P') => self.property(at, letter == Some(b'P')),
			Some(b'q') if self.eat(b'{') => {
				let mut strings = false;
				loop {
					let mut length = 0;
					while self.set_character()?.is_some() {
						length += 1;
					}
					strings |= length != 1;
					if !self.eat(b'|') {
						break;
					}
				}
				if !self.eat(b'}') {
					return self.fail(Code::InvalidRegexpClass, at);
				}
				Ok(strings)
			}
			_ => self.fail(Code::InvalidRegexpEscape, at),
		}
	}
}

// ECMA-262's tables of the names `\p{…}` takes: #table-binary-unicode-properties,
// #table-binary-unicode-properties-of-strings, #table-unicode-general-category-values and
// #table-unicode-script-values
const BINARY: &str = "ASCII ASCII_Hex_Digit AHex Alphabetic Alpha Any Assigned Bidi_Control Bidi_C Bidi_Mirrored Bidi_M Case_Ignorable \
	CI Cased Changes_When_Casefolded CWCF Changes_When_Casemapped CWCM Changes_When_Lowercased CWL \
	Changes_When_NFKC_Casefolded CWKCF Changes_When_Titlecased CWT Changes_When_Uppercased CWU Dash \
	Default_Ignorable_Code_Point DI Deprecated Dep Diacritic Dia Emoji Emoji_Component Emoji_Modifier \
	Emoji_Modifier_Base Emoji_Presentation Extender Ext Grapheme_Base Gr_Base Grapheme_Extend Gr_Ext Hex_Digit Hex \
	IDS_Binary_Operator IDSB IDS_Trinary_Operator IDST ID_Continue IDC ID_Start IDS Ideographic Ideo Join_Control \
	Join_C Logical_Order_Exception LOE Lowercase Lower Math Noncharacter_Code_Point NChar Pattern_Syntax Pat_Syn \
	Pattern_White_Space Pat_WS Quotation_Mark QMark Radical Regional_Indicator RI Sentence_Terminal STerm \
	Soft_Dotted SD Terminal_Punctuation Term Unified_Ideograph UIdeo Uppercase Upper Variation_Selector VS \
	White_Space space XID_Continue XIDC XID_Start XIDS Extended_Pictographic EBase EComp EMod EPres ExtPict";
const STRINGS: &str = "Basic_Emoji Emoji_Keycap_Sequence RGI_Emoji_Modifier_Sequence RGI_Emoji_Flag_Sequence RGI_Emoji_Tag_Sequence \
	RGI_Emoji_ZWJ_Sequence RGI_Emoji";
const GENERAL_CATEGORIES: &str = "Cased_Letter LC Close_Punctuation Pe Connector_Punctuation Pc Control Cc cntrl Currency_Symbol Sc \
	Dash_Punctuation Pd Decimal_Number Nd digit Enclosing_Mark Me Final_Punctuation Pf Format Cf Initial_Punctuation \
	Pi Letter L Letter_Number Nl Line_Separator Zl Lowercase_Letter Ll Mark M Combining_Mark Math_Symbol Sm \
	Modifier_Letter Lm Modifier_Symbol Sk Nonspacing_Mark Mn Number N Open_Punctuation Ps Other C Other_Letter Lo \
	Other_Number No Other_Punctuation Po Other_Symbol So Paragraph_Separator Zp Private_Use Co Punctuation P punct \
	Separator Z Space_Separator Zs Spacing_Mark Mc Surrogate Cs Symbol S Titlecase_Letter Lt Unassigned Cn \
	Uppercase_Letter Lu";
const SCRIPTS: &str = "Adlam Adlm Ahom Anatolian_Hieroglyphs Hluw Arabic Arab Armenian Armn Avestan Avst Balinese Bali Bamum Bamu \
	Bassa_Vah Bass Batak Batk Bengali Beng Bhaiksuki Bhks Bopomofo Bopo Brahmi Brah Braille Brai Buginese Bugi Buhid \
	Buhd Canadian_Aboriginal Cans Carian Cari Caucasian_Albanian Aghb Chakma Cakm Cham Cherokee Cher Common Zyyy \
	Coptic Copt Qaac Cuneiform Xsux Cypriot Cprt Cyrillic Cyrl Deseret Dsrt Devanagari Deva Duployan Dupl \
	Egyptian_Hieroglyphs Egyp Elbasan Elba Ethiopic Ethi Georgian Geor Glagolitic Glag Gothic Goth Grantha Gran \
	Greek Grek Gujarati Gujr Gurmukhi Guru Han Hani Hangul Hang Hanunoo Hano Hatran Hatr Hebrew Hebr Hiragana Hira \
	Imperial_Aramaic Armi Inherited Zinh Qaai Inscriptional_Pahlavi Phli Inscriptional_Parthian Prti Javanese Java \
	Kaithi Kthi Kannada Knda Katakana Kana Kayah_Li Kali Kharoshthi Khar Khmer Khmr Khojki Khoj Khudawadi Sind Lao \
	Laoo Latin Latn Lepcha Lepc Limbu Limb Linear_A Lina Linear_B Linb Lisu Lycian Lyci Lydian Lydi Mahajani Mahj \
	Malayalam Mlym Mandaic Mand Manichaean Mani Marchen Marc Masaram_Gondi Gonm Meetei_Mayek Mtei Mende_Kikakui Mend \
	Meroitic_Cursive Merc Meroitic_Hieroglyphs Mero Miao Plrd Modi Mongolian Mong Mro Mroo Multani Mult Myanmar Mymr \
	Nabataean Nbat New_Tai_Lue Talu Newa Nko Nkoo Nushu Nshu Ogham Ogam Ol_Chiki Olck Old_Hungarian Hung Old_Italic \
	Ital Old_North_Arabian Narb Old_Permic Perm Old_Persian Xpeo Old_South_Arabian Sarb Old_Turkic Orkh Oriya Orya \
	Osage Osge Osmanya Osma Pahawh_Hmong Hmng Palmyrene Palm Pau_Cin_Hau Pauc Phags_Pa Phag Phoenician Phnx \
	Psalter_Pahlavi Phlp Rejang Rjng Runic Runr Samaritan Samr Saurashtra Saur Sharada Shrd Shavian Shaw Siddham \
	Sidd SignWriting Sgnw Sinhala Sinh Sora_Sompeng Sora Soyombo Soyo Sundanese Sund Syloti_Nagri Sylo Syriac Syrc \
	Tagalog Tglg Tagbanwa Tagb Tai_Le Tale Tai_Tham Lana Tai_Viet Tavt Takri Takr Tamil Taml Tangut Tang Telugu Telu \
	Thaana Thaa Thai Tibetan Tibt Tifinagh Tfng Tirhuta Tirh Ugaritic Ugar Vai Vaii Warang_Citi Wara Yi Yiii \
	Zanabazar_Square Zanb Dogra Dogr Gunjala_Gondi Gong Hanifi_Rohingya Rohg Makasar Maka Medefaidrin Medf \
	Old_Sogdian Sogo Sogdian Sogd Elymaic Elym Nandinagari Nand Nyiakeng_Puachue_Hmong Hmnp Wancho Wcho Chorasmian \
	Chrs Diak Dives_Akuru Khitan_Small_Script Kits Yezi Yezidi Cypro_Minoan Cpmn Old_Uyghur Ougr Tangsa Tnsa Toto \
	Vithkuqi Vith Berf Beria_Erfe Gara Garay Gukh Gurung_Khema Hrkt Katakana_Or_Hiragana Kawi Kirat_Rai Krai \
	Nag_Mundari Nagm Ol_Onal Onao Sidetic Sidt Sunu Sunuwar Tai_Yo Tayo Todhri Todr Tolong_Siki Tols Tulu_Tigalari \
	Tutg Unknown Zzzz";
