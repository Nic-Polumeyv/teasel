use std::collections::HashMap;
use std::rc::Rc;

/// Index of an interned string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(pub(crate) u32);

#[derive(Debug, Default)]
pub struct Interner {
	map: HashMap<Rc<str>, StrId>,
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
