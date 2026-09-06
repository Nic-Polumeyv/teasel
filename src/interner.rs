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
	spans: Vec<(u32, u32)>,
	hashes: Vec<u32>,
	/// Slots hold an id plus one; zero is empty. Always a power of two, at most half full.
	table: Vec<u32>,
}

fn hash(s: &str) -> u32 {
	let mut hasher = FastHasher::default();
	hasher.write(s.as_bytes());
	hasher.finish() as u32
}

impl Interner {
	pub fn intern(&mut self, s: &str) -> StrId {
		let hash = hash(s);
		if let Some(id) = self.slot(s, hash) {
			return id;
		}
		if self.table.len() < 2 * (self.spans.len() + 1) {
			self.grow();
		}
		let id = self.spans.len() as u32;
		let start = self.text.len() as u32;
		self.text.push_str(s);
		self.spans.push((start, start + s.len() as u32));
		self.hashes.push(hash);
		let mask = self.table.len() - 1;
		let mut i = hash as usize & mask;
		while self.table[i] != 0 {
			i = (i + 1) & mask;
		}
		self.table[i] = id + 1;
		StrId(id)
	}

	fn slot(&self, s: &str, hash: u32) -> Option<StrId> {
		if self.table.is_empty() {
			return None;
		}
		let mask = self.table.len() - 1;
		let mut i = hash as usize & mask;
		loop {
			let entry = self.table[i];
			if entry == 0 {
				return None;
			}
			let id = entry - 1;
			if self.hashes[id as usize] == hash && self.get(StrId(id)) == s {
				return Some(StrId(id));
			}
			i = (i + 1) & mask;
		}
	}

	fn grow(&mut self) {
		let size = (self.table.len() * 2).max(64);
		self.table = vec![0; size];
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
		self.slot(s, hash(s))
	}

	pub fn get(&self, id: StrId) -> &str {
		let (start, end) = self.spans[id.0 as usize];
		&self.text[start as usize..end as usize]
	}

	pub fn len(&self) -> usize {
		self.spans.len()
	}

	pub fn is_empty(&self) -> bool {
		self.spans.is_empty()
	}
}
