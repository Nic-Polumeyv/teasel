//! Validates regular expression literals against the ES2025 grammar, including Annex B, the
//! way acorn does: no matcher is built, only early errors are reported.

use super::regexp_data::{BINARY_PROPERTIES, BINARY_PROPERTIES_OF_STRINGS, GENERAL_CATEGORY_VALUES, SCRIPT_VALUES};
use super::unicode::{is_id_continue, is_id_start};
use crate::error::Code;
use crate::error::SyntaxError;
use std::collections::HashMap;

type Result<T> = std::result::Result<T, Box<SyntaxError>>;

/// Validates `pattern` and `flags` for a literal whose pattern starts at byte `start`.
pub(super) fn validate(start: u32, pattern: &str, flags: &str) -> Result<()> {
	let mut state = State::new(start, pattern, flags);
	state.validate_flags()?;
	state.validate_pattern()
}

const EOF: i32 = -1;
const MAX_DEPTH: usize = 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharSet {
	None,
	Ok,
	String,
}

/// A branch of a disjunction, so duplicate group names in separate alternatives can be allowed.
#[derive(Clone, Copy)]
struct Branch {
	parent: Option<usize>,
	base: usize,
}

struct State<'a> {
	start: u32,
	pattern: &'a str,
	source: Vec<u16>,
	flags: &'a str,
	switch_u: bool,
	switch_v: bool,
	switch_n: bool,
	pos: usize,
	last_int_value: f64,
	last_string_value: String,
	last_assertion_is_quantifiable: bool,
	num_capturing_parens: f64,
	max_back_reference: f64,
	group_names: HashMap<String, Vec<usize>>,
	back_reference_names: Vec<String>,
	branches: Vec<Branch>,
	branch: Option<usize>,
	class_depth: usize,
}

impl<'a> State<'a> {
	fn new(start: u32, pattern: &'a str, flags: &'a str) -> Self {
		let unicode_sets = flags.contains('v');
		let unicode = flags.contains('u');
		Self {
			start,
			pattern,
			source: pattern.encode_utf16().collect(),
			flags,
			switch_u: unicode_sets || unicode,
			switch_v: unicode_sets,
			switch_n: unicode_sets || unicode,
			pos: 0,
			last_int_value: 0.0,
			last_string_value: String::new(),
			last_assertion_is_quantifiable: false,
			num_capturing_parens: 0.0,
			max_back_reference: 0.0,
			group_names: HashMap::new(),
			back_reference_names: Vec::new(),
			branches: Vec::new(),
			branch: None,
			class_depth: 0,
		}
	}

	fn raise<T>(&self, message: &str) -> Result<T> {
		Err(Box::new(SyntaxError::with(
			self.start,
			Code::InvalidRegexp,
			format!("Invalid regular expression: /{}/: {message}", self.pattern),
		)))
	}

	fn flag_error<T>(&self, code: Code) -> Result<T> {
		Err(Box::new(SyntaxError::new(self.start, code)))
	}

	// Cursor

	fn at(&self, i: usize, force_u: bool) -> i32 {
		let Some(&c) = self.source.get(i) else { return EOF };
		if !(force_u || self.switch_u) || !(0xd800..0xdc00).contains(&c) || i + 1 >= self.source.len() {
			return c as i32;
		}
		let next = self.source[i + 1];
		if (0xdc00..0xe000).contains(&next) {
			0x10000 + (((c as i32) - 0xd800) << 10) + (next as i32 - 0xdc00)
		} else {
			c as i32
		}
	}

	fn next_index(&self, i: usize, force_u: bool) -> usize {
		let Some(&c) = self.source.get(i) else {
			return self.source.len();
		};
		if !(force_u || self.switch_u)
			|| !(0xd800..0xdc00).contains(&c)
			|| i + 1 >= self.source.len()
			|| !(0xdc00..0xe000).contains(&self.source[i + 1])
		{
			return i + 1;
		}
		i + 2
	}

	fn current(&self) -> i32 {
		self.at(self.pos, false)
	}

	fn current_u(&self, force_u: bool) -> i32 {
		self.at(self.pos, force_u)
	}

	fn lookahead(&self) -> i32 {
		self.at(self.next_index(self.pos, false), false)
	}

	fn advance(&mut self) {
		self.pos = self.next_index(self.pos, false);
	}

	fn advance_u(&mut self, force_u: bool) {
		self.pos = self.next_index(self.pos, force_u);
	}

	fn eat(&mut self, ch: char) -> bool {
		if self.current() == ch as i32 {
			self.advance();
			true
		} else {
			false
		}
	}

	fn eat_chars(&mut self, chars: &[char]) -> bool {
		let mut pos = self.pos;
		for &ch in chars {
			if self.at(pos, false) != ch as i32 {
				return false;
			}
			pos = self.next_index(pos, false);
		}
		self.pos = pos;
		true
	}

	// Flags

	fn validate_flags(&self) -> Result<()> {
		let mut u = false;
		let mut v = false;
		for (i, flag) in self.flags.char_indices() {
			if !"gimuysdv".contains(flag) {
				return self.flag_error(Code::InvalidRegexpFlag);
			}
			if self.flags[i + flag.len_utf8()..].contains(flag) {
				return self.flag_error(Code::DuplicateRegexpFlag);
			}
			u |= flag == 'u';
			v |= flag == 'v';
		}
		if u && v {
			return self.flag_error(Code::InvalidRegexpFlag);
		}
		Ok(())
	}

	// Pattern

	fn validate_pattern(&mut self) -> Result<()> {
		self.pattern()?;
		if !self.switch_n && !self.group_names.is_empty() {
			self.switch_n = true;
			self.pattern()?;
		}
		Ok(())
	}

	fn pattern(&mut self) -> Result<()> {
		self.pos = 0;
		self.last_int_value = 0.0;
		self.last_string_value.clear();
		self.last_assertion_is_quantifiable = false;
		self.num_capturing_parens = 0.0;
		self.max_back_reference = 0.0;
		self.group_names.clear();
		self.back_reference_names.clear();
		self.branches.clear();
		self.branch = None;

		self.disjunction()?;

		if self.pos != self.source.len() {
			if self.eat(')') {
				return self.raise("Unmatched ')'");
			}
			if self.eat(']') || self.eat('}') {
				return self.raise("Lone quantifier brackets");
			}
		}
		if self.max_back_reference > self.num_capturing_parens {
			return self.raise("Invalid escape");
		}
		for name in &self.back_reference_names {
			if !self.group_names.contains_key(name) {
				return self.raise("Invalid named capture referenced");
			}
		}
		Ok(())
	}

	fn push_branch(&mut self) {
		let id = self.branches.len();
		self.branches.push(Branch {
			parent: self.branch,
			base: id,
		});
		self.branch = Some(id);
	}

	fn sibling_branch(&mut self) {
		let current = self.branches[self.branch.unwrap()];
		let id = self.branches.len();
		self.branches.push(Branch {
			parent: current.parent,
			base: current.base,
		});
		self.branch = Some(id);
	}

	/// The base of every branch on the chain from `branch` to the root, keyed by base.
	fn ancestor_bases(&self, branch: usize) -> HashMap<usize, usize> {
		let mut bases = HashMap::new();
		let mut x = Some(branch);
		while let Some(i) = x {
			bases.insert(self.branches[i].base, i);
			x = self.branches[i].parent;
		}
		bases
	}

	/// Whether `other` sits in a different alternative from the branch whose bases are given.
	fn separated(&self, bases: &HashMap<usize, usize>, other: usize) -> bool {
		let mut y = Some(other);
		while let Some(j) = y {
			if let Some(&i) = bases.get(&self.branches[j].base)
				&& i != j
			{
				return true;
			}
			y = self.branches[j].parent;
		}
		false
	}

	fn disjunction(&mut self) -> Result<()> {
		if self.branches.len() >= MAX_DEPTH {
			return self.raise("Regular expression nested too deeply");
		}
		self.push_branch();
		self.alternative()?;
		while self.eat('|') {
			self.sibling_branch();
			self.alternative()?;
		}
		self.branch = self.branches[self.branch.unwrap()].parent;

		if self.eat_quantifier(true)? {
			return self.raise("Nothing to repeat");
		}
		if self.eat('{') {
			return self.raise("Lone quantifier brackets");
		}
		Ok(())
	}

	fn alternative(&mut self) -> Result<()> {
		while self.pos < self.source.len() && self.eat_term()? {}
		Ok(())
	}

	fn eat_term(&mut self) -> Result<bool> {
		if self.eat_assertion()? {
			if self.last_assertion_is_quantifiable && self.eat_quantifier(false)? && self.switch_u {
				return self.raise("Invalid quantifier");
			}
			return Ok(true);
		}
		let atom = if self.switch_u {
			self.eat_atom()?
		} else {
			self.eat_extended_atom()?
		};
		if atom {
			self.eat_quantifier(false)?;
			return Ok(true);
		}
		Ok(false)
	}

	fn eat_assertion(&mut self) -> Result<bool> {
		let start = self.pos;
		self.last_assertion_is_quantifiable = false;
		if self.eat('^') || self.eat('$') {
			return Ok(true);
		}
		if self.eat('\\') {
			if self.eat('B') || self.eat('b') {
				return Ok(true);
			}
			self.pos = start;
		}
		if self.eat('(') && self.eat('?') {
			let lookbehind = self.eat('<');
			if self.eat('=') || self.eat('!') {
				self.disjunction()?;
				if !self.eat(')') {
					return self.raise("Unterminated group");
				}
				self.last_assertion_is_quantifiable = !lookbehind;
				return Ok(true);
			}
		}
		self.pos = start;
		Ok(false)
	}

	fn eat_quantifier(&mut self, no_error: bool) -> Result<bool> {
		if self.eat_quantifier_prefix(no_error)? {
			self.eat('?');
			return Ok(true);
		}
		Ok(false)
	}

	fn eat_quantifier_prefix(&mut self, no_error: bool) -> Result<bool> {
		Ok(self.eat('*') || self.eat('+') || self.eat('?') || self.eat_braced_quantifier(no_error)?)
	}

	fn eat_braced_quantifier(&mut self, no_error: bool) -> Result<bool> {
		let start = self.pos;
		if self.eat('{') {
			let mut max = -1.0;
			if self.eat_decimal_digits() {
				let min = self.last_int_value;
				if self.eat(',') && self.eat_decimal_digits() {
					max = self.last_int_value;
				}
				if self.eat('}') {
					if max != -1.0 && max < min && !no_error {
						return self.raise("numbers out of order in {} quantifier");
					}
					return Ok(true);
				}
			}
			if self.switch_u && !no_error {
				return self.raise("Incomplete quantifier");
			}
			self.pos = start;
		}
		Ok(false)
	}

	fn eat_atom(&mut self) -> Result<bool> {
		Ok(self.eat_pattern_characters()
			|| self.eat('.')
			|| self.eat_reverse_solidus_atom_escape()?
			|| self.eat_character_class()?
			|| self.eat_uncapturing_group()?
			|| self.eat_capturing_group()?)
	}

	fn eat_reverse_solidus_atom_escape(&mut self) -> Result<bool> {
		let start = self.pos;
		if self.eat('\\') {
			if self.eat_atom_escape()? {
				return Ok(true);
			}
			self.pos = start;
		}
		Ok(false)
	}

	fn eat_uncapturing_group(&mut self) -> Result<bool> {
		let start = self.pos;
		if self.eat('(') {
			if self.eat('?') {
				let add = self.eat_modifiers();
				let has_hyphen = self.eat('-');
				if !add.is_empty() || has_hyphen {
					for (i, m) in add.char_indices() {
						if add[i + 1..].contains(m) {
							return self.raise("Duplicate regular expression modifiers");
						}
					}
					if has_hyphen {
						let remove = self.eat_modifiers();
						if add.is_empty() && remove.is_empty() && self.current() == ':' as i32 {
							return self.raise("Invalid regular expression modifiers");
						}
						for (i, m) in remove.char_indices() {
							if remove[i + 1..].contains(m) || add.contains(m) {
								return self.raise("Duplicate regular expression modifiers");
							}
						}
					}
				}
				if self.eat(':') {
					self.disjunction()?;
					if self.eat(')') {
						return Ok(true);
					}
					return self.raise("Unterminated group");
				}
			}
			self.pos = start;
		}
		Ok(false)
	}

	fn eat_capturing_group(&mut self) -> Result<bool> {
		if self.eat('(') {
			self.group_specifier()?;
			self.disjunction()?;
			if self.eat(')') {
				self.num_capturing_parens += 1.0;
				return Ok(true);
			}
			return self.raise("Unterminated group");
		}
		Ok(false)
	}

	fn eat_modifiers(&mut self) -> String {
		let mut modifiers = String::new();
		loop {
			let ch = self.current();
			if ch == 'i' as i32 || ch == 'm' as i32 || ch == 's' as i32 {
				modifiers.push(ch as u8 as char);
				self.advance();
			} else {
				return modifiers;
			}
		}
	}

	fn eat_extended_atom(&mut self) -> Result<bool> {
		Ok(self.eat('.')
			|| self.eat_reverse_solidus_atom_escape()?
			|| self.eat_character_class()?
			|| self.eat_uncapturing_group()?
			|| self.eat_capturing_group()?
			|| self.eat_invalid_braced_quantifier()?
			|| self.eat_extended_pattern_character())
	}

	fn eat_invalid_braced_quantifier(&mut self) -> Result<bool> {
		if self.eat_braced_quantifier(true)? {
			return self.raise("Nothing to repeat");
		}
		Ok(false)
	}

	fn eat_syntax_character(&mut self) -> bool {
		let ch = self.current();
		if is_syntax_character(ch) {
			self.last_int_value = ch as f64;
			self.advance();
			return true;
		}
		false
	}

	fn eat_pattern_characters(&mut self) -> bool {
		let start = self.pos;
		loop {
			let ch = self.current();
			if ch == EOF || is_syntax_character(ch) {
				break;
			}
			self.advance();
		}
		self.pos != start
	}

	fn eat_extended_pattern_character(&mut self) -> bool {
		let ch = self.current();
		if ch != EOF
			&& ch != '$' as i32
			&& !(ch >= '(' as i32 && ch <= '+' as i32)
			&& ch != '.' as i32
			&& ch != '?' as i32
			&& ch != '[' as i32
			&& ch != '^' as i32
			&& ch != '|' as i32
		{
			self.advance();
			return true;
		}
		false
	}

	fn group_specifier(&mut self) -> Result<()> {
		if self.eat('?') {
			if !self.eat_group_name()? {
				return self.raise("Invalid group");
			}
			let name = self.last_string_value.clone();
			let branch = self.branch.unwrap();
			if let Some(known) = self.group_names.get(&name) {
				let bases = self.ancestor_bases(branch);
				for &other in known {
					if !self.separated(&bases, other) {
						return self.raise("Duplicate capture group name");
					}
				}
			}
			self.group_names.entry(name).or_default().push(branch);
		}
		Ok(())
	}

	fn eat_group_name(&mut self) -> Result<bool> {
		self.last_string_value.clear();
		if self.eat('<') {
			if self.eat_regexp_identifier_name()? && self.eat('>') {
				return Ok(true);
			}
			return self.raise("Invalid capture group name");
		}
		Ok(false)
	}

	fn eat_regexp_identifier_name(&mut self) -> Result<bool> {
		self.last_string_value.clear();
		if self.eat_regexp_identifier_part(true)? {
			let mut name = String::new();
			name.push(char::from_u32(self.last_int_value as u32).unwrap_or('\u{fffd}'));
			while self.eat_regexp_identifier_part(false)? {
				name.push(char::from_u32(self.last_int_value as u32).unwrap_or('\u{fffd}'));
			}
			self.last_string_value = name;
			return Ok(true);
		}
		Ok(false)
	}

	fn eat_regexp_identifier_part(&mut self, first: bool) -> Result<bool> {
		let start = self.pos;
		let mut ch = self.current_u(true);
		self.advance_u(true);
		if ch == '\\' as i32 && self.eat_regexp_unicode_escape_sequence(true)? {
			ch = self.last_int_value as i32;
		}
		let ok = match char::from_u32(ch as u32) {
			Some(c) if ch >= 0 => {
				c == '$'
					|| c == '_' || if first {
					is_id_start(c)
				} else {
					is_id_continue(c) || c == '\u{200c}' || c == '\u{200d}'
				}
			}
			_ => false,
		};
		if ok {
			self.last_int_value = ch as f64;
			return Ok(true);
		}
		self.pos = start;
		Ok(false)
	}

	fn eat_atom_escape(&mut self) -> Result<bool> {
		if self.eat_back_reference()
			|| self.eat_character_class_escape()? != CharSet::None
			|| self.eat_character_escape()?
			|| (self.switch_n && self.eat_k_group_name()?)
		{
			return Ok(true);
		}
		if self.switch_u {
			if self.current() == 'c' as i32 {
				return self.raise("Invalid unicode escape");
			}
			return self.raise("Invalid escape");
		}
		Ok(false)
	}

	fn eat_back_reference(&mut self) -> bool {
		let start = self.pos;
		if self.eat_decimal_escape() {
			let n = self.last_int_value;
			if self.switch_u {
				if n > self.max_back_reference {
					self.max_back_reference = n;
				}
				return true;
			}
			if n <= self.num_capturing_parens {
				return true;
			}
			self.pos = start;
		}
		false
	}

	fn eat_k_group_name(&mut self) -> Result<bool> {
		if self.eat('k') {
			if self.eat_group_name()? {
				let name = self.last_string_value.clone();
				self.back_reference_names.push(name);
				return Ok(true);
			}
			return self.raise("Invalid named reference");
		}
		Ok(false)
	}

	fn eat_character_escape(&mut self) -> Result<bool> {
		Ok(self.eat_control_escape()
			|| self.eat_c_control_letter()
			|| self.eat_zero()
			|| self.eat_hex_escape_sequence()?
			|| self.eat_regexp_unicode_escape_sequence(false)?
			|| (!self.switch_u && self.eat_legacy_octal_escape_sequence())
			|| self.eat_identity_escape())
	}

	fn eat_c_control_letter(&mut self) -> bool {
		let start = self.pos;
		if self.eat('c') {
			if self.eat_control_letter() {
				return true;
			}
			self.pos = start;
		}
		false
	}

	fn eat_zero(&mut self) -> bool {
		if self.current() == '0' as i32 && !is_decimal_digit(self.lookahead()) {
			self.last_int_value = 0.0;
			self.advance();
			return true;
		}
		false
	}

	fn eat_control_escape(&mut self) -> bool {
		let value = match self.current() {
			0x74 => 0x09,
			0x6e => 0x0a,
			0x76 => 0x0b,
			0x66 => 0x0c,
			0x72 => 0x0d,
			_ => return false,
		};
		self.last_int_value = value as f64;
		self.advance();
		true
	}

	fn eat_control_letter(&mut self) -> bool {
		let ch = self.current();
		if is_control_letter(ch) {
			self.last_int_value = (ch % 0x20) as f64;
			self.advance();
			return true;
		}
		false
	}

	fn eat_regexp_unicode_escape_sequence(&mut self, force_u: bool) -> Result<bool> {
		let start = self.pos;
		let switch_u = force_u || self.switch_u;
		if self.eat('u') {
			if self.eat_fixed_hex_digits(4) {
				let lead = self.last_int_value;
				if switch_u && (55296.0..=56319.0).contains(&lead) {
					let lead_end = self.pos;
					if self.eat('\\') && self.eat('u') && self.eat_fixed_hex_digits(4) {
						let trail = self.last_int_value;
						if (56320.0..=57343.0).contains(&trail) {
							self.last_int_value = (lead - 55296.0) * 1024.0 + (trail - 56320.0) + 65536.0;
							return Ok(true);
						}
					}
					self.pos = lead_end;
					self.last_int_value = lead;
				}
				return Ok(true);
			}
			if switch_u && self.eat('{') && self.eat_hex_digits() && self.eat('}') && self.last_int_value <= 1114111.0 {
				return Ok(true);
			}
			if switch_u {
				return self.raise("Invalid unicode escape");
			}
			self.pos = start;
		}
		Ok(false)
	}

	fn eat_identity_escape(&mut self) -> bool {
		if self.switch_u {
			if self.eat_syntax_character() {
				return true;
			}
			if self.eat('/') {
				self.last_int_value = '/' as u32 as f64;
				return true;
			}
			return false;
		}
		let ch = self.current();
		if ch != 'c' as i32 && (!self.switch_n || ch != 'k' as i32) {
			self.last_int_value = ch as f64;
			self.advance();
			return true;
		}
		false
	}

	fn eat_decimal_escape(&mut self) -> bool {
		self.last_int_value = 0.0;
		let mut ch = self.current();
		if ch >= '1' as i32 && ch <= '9' as i32 {
			loop {
				self.last_int_value = 10.0 * self.last_int_value + (ch - '0' as i32) as f64;
				self.advance();
				ch = self.current();
				if !(ch >= '0' as i32 && ch <= '9' as i32) {
					break;
				}
			}
			return true;
		}
		false
	}

	fn eat_character_class_escape(&mut self) -> Result<CharSet> {
		let ch = self.current();
		if is_character_class_escape(ch) {
			self.last_int_value = -1.0;
			self.advance();
			return Ok(CharSet::Ok);
		}
		let negate = ch == 'P' as i32;
		if self.switch_u && (negate || ch == 'p' as i32) {
			self.last_int_value = -1.0;
			self.advance();
			if self.eat('{') {
				let result = self.eat_unicode_property_value_expression()?;
				if result != CharSet::None && self.eat('}') {
					if negate && result == CharSet::String {
						return self.raise("Invalid property name");
					}
					return Ok(result);
				}
			}
			return self.raise("Invalid property name");
		}
		Ok(CharSet::None)
	}

	fn eat_unicode_property_value_expression(&mut self) -> Result<CharSet> {
		let start = self.pos;
		if self.eat_unicode_property_name() && self.eat('=') {
			let name = self.last_string_value.clone();
			if self.eat_unicode_property_value() {
				let value = self.last_string_value.clone();
				let values = match name.as_str() {
					"General_Category" | "gc" => GENERAL_CATEGORY_VALUES,
					"Script" | "sc" | "Script_Extensions" | "scx" => SCRIPT_VALUES,
					_ => return self.raise("Invalid property name"),
				};
				if !values.contains(&value.as_str()) {
					return self.raise("Invalid property value");
				}
				return Ok(CharSet::Ok);
			}
		}
		self.pos = start;
		if self.eat_unicode_property_value() {
			let name = self.last_string_value.as_str();
			if BINARY_PROPERTIES.contains(&name) || GENERAL_CATEGORY_VALUES.contains(&name) {
				return Ok(CharSet::Ok);
			}
			if self.switch_v && BINARY_PROPERTIES_OF_STRINGS.contains(&name) {
				return Ok(CharSet::String);
			}
			return self.raise("Invalid property name");
		}
		Ok(CharSet::None)
	}

	fn eat_unicode_property_name(&mut self) -> bool {
		self.last_string_value.clear();
		loop {
			let ch = self.current();
			if is_control_letter(ch) || ch == '_' as i32 {
				self.last_string_value.push(ch as u8 as char);
				self.advance();
			} else {
				break;
			}
		}
		!self.last_string_value.is_empty()
	}

	fn eat_unicode_property_value(&mut self) -> bool {
		self.last_string_value.clear();
		loop {
			let ch = self.current();
			if is_control_letter(ch) || ch == '_' as i32 || is_decimal_digit(ch) {
				self.last_string_value.push(ch as u8 as char);
				self.advance();
			} else {
				break;
			}
		}
		!self.last_string_value.is_empty()
	}

	fn eat_character_class(&mut self) -> Result<bool> {
		if self.eat('[') {
			let negate = self.eat('^');
			let result = self.class_contents()?;
			if !self.eat(']') {
				return self.raise("Unterminated character class");
			}
			if negate && result == CharSet::String {
				return self.raise("Negated character class may contain strings");
			}
			return Ok(true);
		}
		Ok(false)
	}

	fn class_contents(&mut self) -> Result<CharSet> {
		self.class_depth += 1;
		if self.class_depth > MAX_DEPTH {
			return self.raise("Regular expression nested too deeply");
		}
		let result = self.class_contents_inner();
		self.class_depth -= 1;
		result
	}

	fn class_contents_inner(&mut self) -> Result<CharSet> {
		if self.current() == ']' as i32 {
			return Ok(CharSet::Ok);
		}
		if self.switch_v {
			return self.class_set_expression();
		}
		self.non_empty_class_ranges()?;
		Ok(CharSet::Ok)
	}

	fn non_empty_class_ranges(&mut self) -> Result<()> {
		while self.eat_class_atom()? {
			let left = self.last_int_value;
			if self.eat('-') && self.eat_class_atom()? {
				let right = self.last_int_value;
				if self.switch_u && (left == -1.0 || right == -1.0) {
					return self.raise("Invalid character class");
				}
				if left != -1.0 && right != -1.0 && left > right {
					return self.raise("Range out of order in character class");
				}
			}
		}
		Ok(())
	}

	fn eat_class_atom(&mut self) -> Result<bool> {
		let start = self.pos;
		if self.eat('\\') {
			if self.eat_class_escape()? {
				return Ok(true);
			}
			if self.switch_u {
				let ch = self.current();
				if ch == 'c' as i32 || is_octal_digit(ch) {
					return self.raise("Invalid class escape");
				}
				return self.raise("Invalid escape");
			}
			self.pos = start;
		}
		let ch = self.current();
		if ch != ']' as i32 && ch != EOF {
			self.last_int_value = ch as f64;
			self.advance();
			return Ok(true);
		}
		Ok(false)
	}

	fn eat_class_escape(&mut self) -> Result<bool> {
		let start = self.pos;
		if self.eat('b') {
			self.last_int_value = 8.0;
			return Ok(true);
		}
		if self.switch_u && self.eat('-') {
			self.last_int_value = '-' as u32 as f64;
			return Ok(true);
		}
		if !self.switch_u && self.eat('c') {
			if self.eat_class_control_letter() {
				return Ok(true);
			}
			self.pos = start;
		}
		Ok(self.eat_character_class_escape()? != CharSet::None || self.eat_character_escape()?)
	}

	fn class_set_expression(&mut self) -> Result<CharSet> {
		let mut result = CharSet::Ok;
		if self.eat_class_set_range()? {
		} else if let Some(sub) = self.eat_class_set_operand()? {
			if sub == CharSet::String {
				result = CharSet::String;
			}
			let start = self.pos;
			while self.eat_chars(&['&', '&']) {
				if self.current() != '&' as i32
					&& let Some(sub) = self.eat_class_set_operand()?
				{
					if sub != CharSet::String {
						result = CharSet::Ok;
					}
					continue;
				}
				return self.raise("Invalid character in character class");
			}
			if start != self.pos {
				return Ok(result);
			}
			while self.eat_chars(&['-', '-']) {
				if self.eat_class_set_operand()?.is_some() {
					continue;
				}
				return self.raise("Invalid character in character class");
			}
			if start != self.pos {
				return Ok(result);
			}
		} else {
			return self.raise("Invalid character in character class");
		}
		loop {
			if self.eat_class_set_range()? {
				continue;
			}
			let Some(sub) = self.eat_class_set_operand()? else {
				return Ok(result);
			};
			if sub == CharSet::String {
				result = CharSet::String;
			}
		}
	}

	fn eat_class_set_range(&mut self) -> Result<bool> {
		let start = self.pos;
		if self.eat_class_set_character()? {
			let left = self.last_int_value;
			if self.eat('-') && self.eat_class_set_character()? {
				let right = self.last_int_value;
				if left != -1.0 && right != -1.0 && left > right {
					return self.raise("Range out of order in character class");
				}
				return Ok(true);
			}
			self.pos = start;
		}
		Ok(false)
	}

	fn eat_class_set_operand(&mut self) -> Result<Option<CharSet>> {
		if self.eat_class_set_character()? {
			return Ok(Some(CharSet::Ok));
		}
		if let Some(result) = self.eat_class_string_disjunction()? {
			return Ok(Some(result));
		}
		self.eat_nested_class()
	}

	fn eat_nested_class(&mut self) -> Result<Option<CharSet>> {
		let start = self.pos;
		if self.eat('[') {
			let negate = self.eat('^');
			let result = self.class_contents()?;
			if self.eat(']') {
				if negate && result == CharSet::String {
					return self.raise("Negated character class may contain strings");
				}
				return Ok(Some(result));
			}
			self.pos = start;
		}
		if self.eat('\\') {
			let result = self.eat_character_class_escape()?;
			if result != CharSet::None {
				return Ok(Some(result));
			}
			self.pos = start;
		}
		Ok(None)
	}

	fn eat_class_string_disjunction(&mut self) -> Result<Option<CharSet>> {
		let start = self.pos;
		if self.eat_chars(&['\\', 'q']) {
			if self.eat('{') {
				let result = self.class_string_disjunction_contents()?;
				if self.eat('}') {
					return Ok(Some(result));
				}
			} else {
				return self.raise("Invalid escape");
			}
			self.pos = start;
		}
		Ok(None)
	}

	fn class_string_disjunction_contents(&mut self) -> Result<CharSet> {
		let mut result = self.class_string()?;
		while self.eat('|') {
			if self.class_string()? == CharSet::String {
				result = CharSet::String;
			}
		}
		Ok(result)
	}

	fn class_string(&mut self) -> Result<CharSet> {
		let mut count = 0;
		while self.eat_class_set_character()? {
			count += 1;
		}
		Ok(if count == 1 { CharSet::Ok } else { CharSet::String })
	}

	fn eat_class_set_character(&mut self) -> Result<bool> {
		let start = self.pos;
		if self.eat('\\') {
			if self.eat_character_escape()? || self.eat_class_set_reserved_punctuator() {
				return Ok(true);
			}
			if self.eat('b') {
				self.last_int_value = 8.0;
				return Ok(true);
			}
			self.pos = start;
			return Ok(false);
		}
		let ch = self.current();
		if ch < 0 || (ch == self.lookahead() && is_class_set_reserved_double_punctuator(ch)) {
			return Ok(false);
		}
		if is_class_set_syntax_character(ch) {
			return Ok(false);
		}
		self.advance();
		self.last_int_value = ch as f64;
		Ok(true)
	}

	fn eat_class_set_reserved_punctuator(&mut self) -> bool {
		let ch = self.current();
		if is_class_set_reserved_punctuator(ch) {
			self.last_int_value = ch as f64;
			self.advance();
			return true;
		}
		false
	}

	fn eat_class_control_letter(&mut self) -> bool {
		let ch = self.current();
		if is_decimal_digit(ch) || ch == '_' as i32 {
			self.last_int_value = (ch % 0x20) as f64;
			self.advance();
			return true;
		}
		false
	}

	fn eat_hex_escape_sequence(&mut self) -> Result<bool> {
		let start = self.pos;
		if self.eat('x') {
			if self.eat_fixed_hex_digits(2) {
				return Ok(true);
			}
			if self.switch_u {
				return self.raise("Invalid escape");
			}
			self.pos = start;
		}
		Ok(false)
	}

	fn eat_decimal_digits(&mut self) -> bool {
		let start = self.pos;
		self.last_int_value = 0.0;
		loop {
			let ch = self.current();
			if !is_decimal_digit(ch) {
				break;
			}
			self.last_int_value = 10.0 * self.last_int_value + (ch - '0' as i32) as f64;
			self.advance();
		}
		self.pos != start
	}

	fn eat_hex_digits(&mut self) -> bool {
		let start = self.pos;
		self.last_int_value = 0.0;
		loop {
			let ch = self.current();
			let Some(digit) = hex_value(ch) else { break };
			self.last_int_value = 16.0 * self.last_int_value + digit as f64;
			self.advance();
		}
		self.pos != start
	}

	fn eat_legacy_octal_escape_sequence(&mut self) -> bool {
		if self.eat_octal_digit() {
			let n1 = self.last_int_value;
			if self.eat_octal_digit() {
				let n2 = self.last_int_value;
				if n1 <= 3.0 && self.eat_octal_digit() {
					self.last_int_value += n1 * 64.0 + n2 * 8.0;
				} else {
					self.last_int_value = n1 * 8.0 + n2;
				}
			} else {
				self.last_int_value = n1;
			}
			return true;
		}
		false
	}

	fn eat_octal_digit(&mut self) -> bool {
		let ch = self.current();
		if is_octal_digit(ch) {
			self.last_int_value = (ch - '0' as i32) as f64;
			self.advance();
			return true;
		}
		self.last_int_value = 0.0;
		false
	}

	fn eat_fixed_hex_digits(&mut self, length: usize) -> bool {
		let start = self.pos;
		self.last_int_value = 0.0;
		for _ in 0..length {
			let Some(digit) = hex_value(self.current()) else {
				self.pos = start;
				return false;
			};
			self.last_int_value = 16.0 * self.last_int_value + digit as f64;
			self.advance();
		}
		true
	}
}

fn is_syntax_character(ch: i32) -> bool {
	ch == '$' as i32
		|| (ch >= '(' as i32 && ch <= '+' as i32)
		|| ch == '.' as i32
		|| ch == '?' as i32
		|| (ch >= '[' as i32 && ch <= '^' as i32)
		|| (ch >= '{' as i32 && ch <= '}' as i32)
}

fn is_control_letter(ch: i32) -> bool {
	(ch >= 'A' as i32 && ch <= 'Z' as i32) || (ch >= 'a' as i32 && ch <= 'z' as i32)
}

fn is_character_class_escape(ch: i32) -> bool {
	matches!(ch, 0x64 | 0x44 | 0x73 | 0x53 | 0x77 | 0x57)
}

fn is_decimal_digit(ch: i32) -> bool {
	ch >= '0' as i32 && ch <= '9' as i32
}

fn is_octal_digit(ch: i32) -> bool {
	ch >= '0' as i32 && ch <= '7' as i32
}

fn hex_value(ch: i32) -> Option<i64> {
	match ch {
		0x30..=0x39 => Some((ch - 0x30) as i64),
		0x41..=0x46 => Some((ch - 0x41 + 10) as i64),
		0x61..=0x66 => Some((ch - 0x61 + 10) as i64),
		_ => None,
	}
}

fn is_class_set_reserved_double_punctuator(ch: i32) -> bool {
	ch == '!' as i32
		|| (ch >= '#' as i32 && ch <= '&' as i32)
		|| (ch >= '*' as i32 && ch <= ',' as i32)
		|| ch == '.' as i32
		|| (ch >= ':' as i32 && ch <= '@' as i32)
		|| ch == '^' as i32
		|| ch == '`' as i32
		|| ch == '~' as i32
}

fn is_class_set_syntax_character(ch: i32) -> bool {
	ch == '(' as i32
		|| ch == ')' as i32
		|| ch == '-' as i32
		|| ch == '/' as i32
		|| (ch >= '[' as i32 && ch <= ']' as i32)
		|| (ch >= '{' as i32 && ch <= '}' as i32)
}

fn is_class_set_reserved_punctuator(ch: i32) -> bool {
	ch == '!' as i32
		|| ch == '#' as i32
		|| ch == '%' as i32
		|| ch == '&' as i32
		|| ch == ',' as i32
		|| ch == '-' as i32
		|| (ch >= ':' as i32 && ch <= '>' as i32)
		|| ch == '@' as i32
		|| ch == '`' as i32
		|| ch == '~' as i32
}
