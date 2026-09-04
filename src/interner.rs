use std::collections::HashMap;

/// Index of an interned string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(pub u32);

#[derive(Debug, Default)]
pub struct Interner {
	map: HashMap<String, StrId>,
	strings: Vec<String>,
}

impl Interner {
	pub fn intern(&mut self, s: &str) -> StrId {
		if let Some(&id) = self.map.get(s) {
			return id;
		}
		let id = StrId(self.strings.len() as u32);
		self.strings.push(s.to_owned());
		self.map.insert(s.to_owned(), id);
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
