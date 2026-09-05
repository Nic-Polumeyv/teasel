use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

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

/// Index of an interned string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(pub(crate) u32);

#[derive(Debug, Default)]
pub struct Interner {
	map: FastMap<Rc<str>, StrId>,
	strings: Vec<Rc<str>>,
}

impl Interner {
	pub fn intern(&mut self, s: &str) -> StrId {
		if let Some(&id) = self.map.get(s) {
			return id;
		}
		let id = StrId(self.strings.len() as u32);
		let s: Rc<str> = Rc::from(s);
		self.strings.push(Rc::clone(&s));
		self.map.insert(s, id);
		id
	}

	pub fn get(&self, id: StrId) -> &str {
		&self.strings[id.0 as usize]
	}

	pub fn len(&self) -> usize {
		self.strings.len()
	}

	pub fn is_empty(&self) -> bool {
		self.strings.is_empty()
	}
}
