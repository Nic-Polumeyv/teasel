use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A multiply-rotate hasher for short keys, the shape rustc uses; SipHash was a measurable
/// share of parse time.
#[derive(Default)]
pub struct FastHasher(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FastHasher {
	fn add(&mut self, word: u64) {
		self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
	}
}

impl Hasher for FastHasher {
	fn write(&mut self, bytes: &[u8]) {
		let mut chunks = bytes.chunks_exact(8);
		for chunk in &mut chunks {
			self.add(u64::from_le_bytes(chunk.try_into().unwrap()));
		}
		let rest = chunks.remainder();
		if !rest.is_empty() {
			let mut buf = [0u8; 8];
			buf[..rest.len()].copy_from_slice(rest);
			self.add(u64::from_le_bytes(buf));
		}
	}

	fn write_u8(&mut self, i: u8) {
		self.add(i as u64);
	}

	fn write_u32(&mut self, i: u32) {
		self.add(i as u64);
	}

	fn write_u64(&mut self, i: u64) {
		self.add(i);
	}

	fn write_usize(&mut self, i: usize) {
		self.add(i as u64);
	}

	fn finish(&self) -> u64 {
		self.0
	}
}

pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;
pub type FastSet<K> = std::collections::HashSet<K, BuildHasherDefault<FastHasher>>;

/// Index of an interned string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(pub(crate) u32);

/// Strings back to back in one text, found through an open-addressing table by hash: no
/// allocation per string and one hash per lookup, which `HashMap<Rc<str>>` paid twice on a miss.
#[derive(Debug, Default)]
pub struct Interner {
	text: String,
	/// Where each string starts, and where the next would.
	starts: Vec<u32>,
	hashes: Vec<u32>,
	/// Slots hold an id plus one; zero is empty. Always a power of two, at most half full.
	table: Vec<u32>,
}

/// The same mix as `FastHasher`, eight bytes at a time, the tail read as two overlapping words
/// rather than copied into one; the low half of the product depends on the first bytes only,
/// so the high half is folded in.
fn hash(s: &str) -> u32 {
	let bytes = s.as_bytes();
	let n = bytes.len();
	let mix = |h: u64, word: u64| (h.rotate_left(5) ^ word).wrapping_mul(SEED);
	let mut h = mix(0, n as u64);
	let mut i = 0;
	while i + 8 <= n {
		h = mix(h, u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap()));
		i += 8;
	}
	if i < n {
		let tail = if n - i >= 4 {
			u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as u64
				| (u32::from_le_bytes(bytes[n - 4..].try_into().unwrap()) as u64) << 32
		} else if n - i >= 2 {
			u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap()) as u64
				| (u16::from_le_bytes(bytes[n - 2..].try_into().unwrap()) as u64) << 16
		} else {
			bytes[i] as u64
		};
		h = mix(h, tail);
	}
	(h ^ (h >> 32)) as u32
}

impl Interner {
	/// Room for the strings of `bytes` of source, so the table grows rarely.
	pub fn sized(bytes: usize) -> Self {
		let slots = (bytes / 16).next_power_of_two().clamp(64, 4096);
		let mut starts = Vec::with_capacity(slots / 2 + 1);
		starts.push(0);
		Interner {
			text: String::with_capacity(bytes / 4),
			starts,
			hashes: Vec::with_capacity(slots / 2),
			table: vec![0; slots],
		}
	}

	pub fn intern(&mut self, s: &str) -> StrId {
		let hash = hash(s);
		let mut slot = match self.probe(s, hash) {
			Ok(id) => return id,
			Err(slot) => slot,
		};
		if self.table.len() < 2 * self.len() + 2 {
			self.grow();
			slot = self.probe(s, hash).unwrap_err();
		}
		let id = self.len() as u32;
		self.text.push_str(s);
		self.starts.push(self.text.len() as u32);
		self.hashes.push(hash);
		self.table[slot] = id + 1;
		StrId(id)
	}

	/// The id of `s`, or the empty slot it would take.
	fn probe(&self, s: &str, hash: u32) -> Result<StrId, usize> {
		if self.table.is_empty() {
			return Err(0);
		}
		let mask = self.table.len() - 1;
		let mut i = hash as usize & mask;
		loop {
			let entry = self.table[i];
			if entry == 0 {
				return Err(i);
			}
			let id = entry - 1;
			if self.hashes[id as usize] == hash && self.get(StrId(id)) == s {
				return Ok(StrId(id));
			}
			i = (i + 1) & mask;
		}
	}

	fn grow(&mut self) {
		let size = (self.table.len() * 2).max(64);
		self.table = vec![0; size];
		if self.starts.is_empty() {
			self.starts.push(0);
		}
		let mask = size - 1;
		for (id, &hash) in self.hashes.iter().enumerate() {
			let mut i = hash as usize & mask;
			while self.table[i] != 0 {
				i = (i + 1) & mask;
			}
			self.table[i] = id as u32 + 1;
		}
	}

	pub fn find(&self, s: &str) -> Option<StrId> {
		self.probe(s, hash(s)).ok()
	}

	pub fn get(&self, id: StrId) -> &str {
		let i = id.0 as usize;
		// every start is where a whole string was appended, so a character boundary
		unsafe {
			self.text
				.get_unchecked(self.starts[i] as usize..self.starts[i + 1] as usize)
		}
	}

	pub fn len(&self) -> usize {
		self.starts.len().saturating_sub(1)
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn interns_once_and_finds() {
		let mut interner = Interner::default();
		assert_eq!(interner.find(""), None);
		assert_eq!(interner.find("x"), None);
		let empty = interner.intern("");
		assert_eq!(interner.get(empty), "");
		assert_eq!(interner.intern(""), empty);
		assert_eq!(interner.find(""), Some(empty));
		let ids: Vec<_> = (0..500).map(|i| interner.intern(&format!("name{i}"))).collect();
		for (i, &id) in ids.iter().enumerate() {
			let name = format!("name{i}");
			assert_eq!(interner.get(id), name);
			assert_eq!(interner.find(&name), Some(id));
			assert_eq!(interner.intern(&name), id);
		}
		assert_eq!(interner.find("nope"), None);
		assert_eq!(interner.len(), 501);
	}

	#[test]
	fn prefixes_and_collisions_stay_apart() {
		let mut interner = Interner::default();
		let names = [
			"value", "value123", "length", "lengthy", "abcd", "abcdefg", "k100", "k1000",
		];
		let ids: Vec<_> = names.iter().map(|n| interner.intern(n)).collect();
		for (name, id) in names.iter().zip(&ids) {
			assert_eq!(interner.get(*id), *name);
		}
		assert_ne!(hash("value"), hash("value123"));
		let mut sorted = ids.clone();
		sorted.dedup();
		assert_eq!(sorted.len(), names.len());
	}
}
