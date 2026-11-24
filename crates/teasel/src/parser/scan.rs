pub(crate) const SPACE: u8 = 1;
pub(crate) const NEWLINE: u8 = 2;
pub(crate) const ID_START: u8 = 4;
pub(crate) const ID_CONTINUE: u8 = 8;
pub(crate) const DIGIT: u8 = 16;
/// Means something to the search for a regular expression literal's end, with `NEWLINE`: a
/// slash, a backslash, a bracket, a `(` or the first byte of a line or paragraph separator.
pub(crate) const REGEXP: u8 = 64;

pub(crate) static CLASS: [u8; 256] = classes();

const fn classes() -> [u8; 256] {
	let mut table = [0u8; 256];
	let mut b = 0;
	while b < 128 {
		let c = b as u8;
		let mut class = 0;
		if matches!(c, b' ' | b'\t' | 0x0b | 0x0c) {
			class |= SPACE;
		}
		if matches!(c, b'\n' | b'\r') {
			class |= NEWLINE;
		}
		if c.is_ascii_alphabetic() || c == b'$' || c == b'_' {
			class |= ID_START | ID_CONTINUE;
		}
		if c.is_ascii_digit() {
			class |= DIGIT | ID_CONTINUE;
		}
		if matches!(c, b'/' | b'\\' | b'[' | b']' | b'(') {
			class |= REGEXP;
		}
		table[b] = class;
		b += 1;
	}
	table[0xe2] = REGEXP;
	table
}

const ONES: u64 = 0x0101_0101_0101_0101;
const HIGHS: u64 = 0x8080_8080_8080_8080;

// has-zero-byte trick: bits above the first hit may be wrong, the first never is
fn bytes_equal(word: u64, byte: u8) -> u64 {
	let x = word ^ (ONES * byte as u64);
	x.wrapping_sub(ONES) & !x & HIGHS
}

pub(crate) fn find<const N: usize>(bytes: &[u8], mut from: usize, needles: [u8; N], high: bool) -> usize {
	let (chunks, _) = bytes.get(from..).unwrap_or_default().as_chunks::<8>();
	for chunk in chunks {
		let word = u64::from_le_bytes(*chunk);
		let mut hits = if high { word & HIGHS } else { 0 };
		for needle in needles {
			hits |= bytes_equal(word, needle);
		}
		if hits != 0 {
			return from + (hits.trailing_zeros() / 8) as usize;
		}
		from += 8;
	}
	while from < bytes.len() {
		let b = bytes[from];
		if needles.contains(&b) || (high && b >= 0x80) {
			break;
		}
		from += 1;
	}
	from
}

// every hit exact, not only the first
fn bytes_exactly(word: u64, byte: u8) -> u64 {
	let x = word ^ (ONES * byte as u64);
	!((x & !HIGHS).wrapping_add(!HIGHS) | x) & HIGHS
}

/// Where `first` stands with one of `seconds` after it, from `from` on, or a `first` among the last
/// eight bytes, whatever follows it; the length when there is neither.
pub(crate) fn find_pair<const N: usize>(bytes: &[u8], mut from: usize, first: u8, seconds: [u8; N]) -> usize {
	while from + 9 <= bytes.len() {
		let word = u64::from_le_bytes(bytes[from..from + 8].try_into().unwrap());
		let next = word >> 8 | (bytes[from + 8] as u64) << 56;
		let mut hits = 0;
		for second in seconds {
			hits |= bytes_exactly(next, second);
		}
		hits &= bytes_exactly(word, first);
		if hits != 0 {
			return from + (hits.trailing_zeros() / 8) as usize;
		}
		from += 8;
	}
	find(bytes, from, [first], false)
}

/// Whether `bytes` starts with a line separator or paragraph separator (U+2028, U+2029).
pub(crate) fn is_separator(bytes: &[u8]) -> bool {
	matches!(bytes, [0xe2, 0x80, 0xa8 | 0xa9, ..])
}

pub(crate) fn line_end(bytes: &[u8]) -> usize {
	let mut i = 0;
	loop {
		i = find(bytes, i, *b"\n\r\xe2", false);
		if i == bytes.len() || bytes[i] != 0xe2 || is_separator(&bytes[i..]) {
			return i;
		}
		i += 1;
	}
}

pub(crate) fn is_new_line(c: char) -> bool {
	matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

pub(crate) fn is_whitespace(c: char) -> bool {
	matches!(
		c,
		'\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
	)
}

/// ECMAScript white space or a line terminator.
pub(crate) fn is_space(c: char) -> bool {
	matches!(c, ' ' | '\t'..='\r') || is_new_line(c) || is_whitespace(c)
}


#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn line_end_finds_every_terminator() {
		let cases: &[(&[u8], usize)] = &[
			(b"abc\ndef", 3),
			(b"abc\rdef", 3),
			("ab\u{2028}cd".as_bytes(), 2),
			("ab\u{2029}cd".as_bytes(), 2),
			(b"no terminator here at all", 25),
			(b"", 0),
			(b"\n", 0),
			("\u{e2}\u{80}x\n".as_bytes(), 5),
			(b"\xe2\x80\xa7\n", 3),
			("1234567\u{2028}".as_bytes(), 7),
			("12345678\u{2028}".as_bytes(), 8),
			(b"1234567\n", 7),
			(b"12345678\n", 8),
			(b"123456789012345\n", 15),
			(b"\xe2\xe2\xe2\xe2\xe2\xe2\xe2\xe2\xe2\n", 9),
		];
		for &(bytes, end) in cases {
			assert_eq!(
				line_end(bytes),
				end,
				"{:?}",
				std::string::String::from_utf8_lossy(bytes)
			);
		}
	}

	#[test]
	fn finds_the_first_needle_or_high_byte() {
		assert_eq!(find(b"abcdefghij\nk", 0, *b"\n", false), 10);
		assert_eq!(find(b"abc", 0, *b"\n", false), 3);
		assert_eq!(find(b"", 0, *b"\n", false), 0);
		assert_eq!(find(b"\n", 0, *b"\n\r", false), 0);
		assert_eq!(find(b"1234567\r", 0, *b"\n\r", false), 7);
		assert_eq!(find(b"12345678\r", 0, *b"\n\r", false), 8);
		assert_eq!(find("abcdefgh\u{e9}x".as_bytes(), 0, [], true), 8);
		assert_eq!(find(b"\x80", 0, [], true), 0);
		assert_eq!(find(b"\x80", 0, [], false), 1);
		assert_eq!(find(b"0123456789", 3, *b"5", false), 5);
		assert_eq!(find(b"aaaaaaaaaaaaaaaab", 0, *b"b", false), 16);
		assert_eq!(find(b"\x81\x01\xff\x00\n", 0, *b"\n", false), 4);
	}

	#[test]
	fn finds_the_first_pair_or_a_first_near_the_end() {
		for len in 0..26 {
			for pair in 0..=len {
				for lone in 0..=len {
					let mut bytes = vec![b'a'; len];
					if lone < len {
						bytes[lone] = b'<';
					}
					if pair + 1 < len {
						bytes[pair] = b'<';
						bytes[pair + 1] = b'/';
					}
					for from in (0..=len).step_by(3) {
						let exact = (from..len).find(|&i| bytes[i..].starts_with(b"</")).unwrap_or(len);
						let found = find_pair(&bytes, from, b'<', *b"/");
						assert!(
							found == exact || (found < exact && bytes[found] == b'<' && found + 8 >= len),
							"{:?} from {from}: {found}",
							std::string::String::from_utf8_lossy(&bytes)
						);
					}
				}
			}
		}
		assert_eq!(find_pair(b"<a <! <script", 0, b'<', *b"!s"), 3);
		assert_eq!(find_pair(b"<a <b <script>....", 0, b'<', *b"!s"), 6);
	}

	#[test]
	fn classes_match_the_predicates() {
		for b in 0..=255u8 {
			assert_eq!(CLASS[b as usize] & SPACE != 0, matches!(b, b' ' | b'\t' | 0x0b | 0x0c));
			assert_eq!(CLASS[b as usize] & NEWLINE != 0, matches!(b, b'\n' | b'\r'));
			assert_eq!(
				CLASS[b as usize] & ID_START != 0,
				b.is_ascii_alphabetic() || b == b'$' || b == b'_'
			);
			assert_eq!(
				CLASS[b as usize] & ID_CONTINUE != 0,
				b.is_ascii_alphanumeric() || b == b'$' || b == b'_'
			);
			assert_eq!(CLASS[b as usize] & DIGIT != 0, b.is_ascii_digit());
			assert_eq!(
				CLASS[b as usize] & REGEXP != 0,
				matches!(b, b'/' | b'\\' | b'[' | b']' | b'(' | 0xe2)
			);
		}
	}
}
