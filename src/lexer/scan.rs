pub(crate) const SPACE: u8 = 1;
pub(crate) const NEWLINE: u8 = 2;
pub(crate) const ID_START: u8 = 4;
pub(crate) const ID_CONTINUE: u8 = 8;
pub(crate) const DIGIT: u8 = 16;

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
		table[b] = class;
		b += 1;
	}
	table
}

#[inline]
pub(crate) fn class(b: u8) -> u8 {
	CLASS[b as usize]
}

#[inline]
pub(crate) fn run_of(bytes: &[u8], mut from: usize, flag: u8) -> usize {
	while from < bytes.len() && CLASS[bytes[from] as usize] & flag != 0 {
		from += 1;
	}
	from
}

const ONES: u64 = 0x0101_0101_0101_0101;
const HIGHS: u64 = 0x8080_8080_8080_8080;

// has-zero-byte trick: bits above the first hit may be wrong, the first never is
#[inline]
fn bytes_equal(word: u64, byte: u8) -> u64 {
	let x = word ^ (ONES * byte as u64);
	x.wrapping_sub(ONES) & !x & HIGHS
}

#[inline]
pub(crate) fn find<const N: usize>(bytes: &[u8], mut from: usize, needles: [u8; N], high: bool) -> usize {
	while let Some(chunk) = bytes.get(from..from + 8) {
		let word = u64::from_le_bytes(chunk.try_into().unwrap());
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_the_first_needle_or_high_byte() {
		assert_eq!(find(b"abcdefghij\nk", 0, [b'\n'], false), 10);
		assert_eq!(find(b"abc", 0, [b'\n'], false), 3);
		assert_eq!(find(b"", 0, [b'\n'], false), 0);
		assert_eq!(find(b"\n", 0, [b'\n', b'\r'], false), 0);
		assert_eq!(find(b"1234567\r", 0, [b'\n', b'\r'], false), 7);
		assert_eq!(find(b"12345678\r", 0, [b'\n', b'\r'], false), 8);
		assert_eq!(find("abcdefgh\u{e9}x".as_bytes(), 0, [], true), 8);
		assert_eq!(find(b"\x80", 0, [], true), 0);
		assert_eq!(find(b"\x80", 0, [], false), 1);
		assert_eq!(find(b"0123456789", 3, [b'5'], false), 5);
		assert_eq!(find(b"aaaaaaaaaaaaaaaab", 0, [b'b'], false), 16);
		assert_eq!(find(b"\x81\x01\xff\x00\n", 0, [b'\n'], false), 4);
	}

	#[test]
	fn classes_match_the_predicates() {
		for b in 0..=255u8 {
			assert_eq!(class(b) & SPACE != 0, matches!(b, b' ' | b'\t' | 0x0b | 0x0c));
			assert_eq!(class(b) & NEWLINE != 0, matches!(b, b'\n' | b'\r'));
			assert_eq!(
				class(b) & ID_START != 0,
				b.is_ascii_alphabetic() || b == b'$' || b == b'_'
			);
			assert_eq!(
				class(b) & ID_CONTINUE != 0,
				b.is_ascii_alphanumeric() || b == b'$' || b == b'_'
			);
			assert_eq!(class(b) & DIGIT != 0, b.is_ascii_digit());
		}
		assert_eq!(run_of(b"abc1_$ x", 0, ID_CONTINUE), 6);
		assert_eq!(run_of(b"  \tx", 0, SPACE), 3);
	}
}
