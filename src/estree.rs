//! Serializes an `Ast` to ESTree: as JSON text, or as a token stream a binding hands to
//! JavaScript without a text round trip.

use crate::ast::{Ast, Class, Function, List, MethodKind, NodeId, NodeKind, PropertyKind, Value};
use crate::interner::{FastMap, Interner, StrId};
use crate::parser::Entry;
use crate::scopes::Role;
use std::fmt::Write;

/// How an extension's data serializes: its own nodes, and the keys it adds to JavaScript nodes.
pub trait Emit: crate::ast::Walk {
	/// Emits one of the extension's own nodes and closes it, or what erasing puts in its place.
	fn node<S: Sink>(&self, w: &mut Writer<Self, S>, id: NodeId, index: u32);
	fn extras<S: Sink>(&self, _w: &mut Writer<Self, S>, _id: NodeId) {}
	/// Whether erasing leaves nothing of a node in a list: type-only declarations and imports.
	fn erased<S: Sink>(&self, _w: &Writer<Self, S>, _id: NodeId) -> bool {
		false
	}
}

/// What the answer carries beyond the tree.
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
	/// The answer lists every comment read.
	pub comments: bool,
	/// Nodes carry their scope and identifiers their binding, and the answer lists both tables.
	pub scopes: bool,
	/// TypeScript is erased: annotations, type-only declarations and imports go, assertions give
	/// way to their expression, and what erasure cannot express is listed as `typescript`.
	pub erase: bool,
	/// The answer lists the errors recovered from, as `errors`.
	pub errors: bool,
}

impl Emit for () {
	fn node<S: Sink>(&self, _w: &mut Writer<Self, S>, _id: NodeId, _index: u32) {
		unreachable!("the JavaScript parser adds no extension nodes")
	}
}

/// Where the writer puts what it emits. Containers nest: a node begins with its type and ends
/// like a plain object or a list; offsets in `slice`, `span` and `loc` are UTF-16. Strings the
/// writer names itself are constants; a string computed for one tree is text.
pub trait Sink {
	/// The tree's interned strings, before anything refers to them.
	fn strings(&mut self, _interner: &Interner) {}
	fn begin(&mut self, ty: &'static str);
	fn object(&mut self);
	fn list(&mut self);
	fn end(&mut self);
	fn key(&mut self, key: &'static str);
	fn int(&mut self, value: u32);
	fn float(&mut self, value: f64);
	fn bool(&mut self, value: bool);
	fn null(&mut self);
	fn str(&mut self, value: &'static str);
	fn text(&mut self, value: &str);
	/// A string of the tree's interner.
	fn interned(&mut self, id: StrId, value: &str);
	/// A string equal to the source between two offsets.
	fn slice(&mut self, value: &str, start: u32, end: u32);
	fn span(&mut self, start: u32, end: u32);
	fn loc(&mut self, start_line: u32, start_column: u32, end_line: u32, end_column: u32);
	/// The key of a scope table: the tables are a root's last entries, and a sink may place them
	/// where a decoder finds them before the nodes that refer to them.
	fn table(&mut self, key: &'static str) {
		self.key(key);
	}
	fn ints(&mut self, values: &[u32]) {
		self.list();
		for &value in values {
			self.int(value);
		}
		self.end();
	}
	/// A list of the tree's interned strings.
	fn strs(&mut self, strings: &[(StrId, &str)]) {
		self.list();
		for &(id, value) in strings {
			self.interned(id, value);
		}
		self.end();
	}
}

impl<S: Sink> Sink for &mut S {
	fn strings(&mut self, interner: &Interner) {
		(**self).strings(interner)
	}
	fn begin(&mut self, ty: &'static str) {
		(**self).begin(ty)
	}
	fn object(&mut self) {
		(**self).object()
	}
	fn list(&mut self) {
		(**self).list()
	}
	fn end(&mut self) {
		(**self).end()
	}
	fn key(&mut self, key: &'static str) {
		(**self).key(key)
	}
	fn int(&mut self, value: u32) {
		(**self).int(value)
	}
	fn float(&mut self, value: f64) {
		(**self).float(value)
	}
	fn bool(&mut self, value: bool) {
		(**self).bool(value)
	}
	fn null(&mut self) {
		(**self).null()
	}
	fn str(&mut self, value: &'static str) {
		(**self).str(value)
	}
	fn text(&mut self, value: &str) {
		(**self).text(value)
	}
	fn interned(&mut self, id: StrId, value: &str) {
		(**self).interned(id, value)
	}
	fn slice(&mut self, value: &str, start: u32, end: u32) {
		(**self).slice(value, start, end)
	}
	fn span(&mut self, start: u32, end: u32) {
		(**self).span(start, end)
	}
	fn loc(&mut self, start_line: u32, start_column: u32, end_line: u32, end_column: u32) {
		(**self).loc(start_line, start_column, end_line, end_column)
	}
	fn table(&mut self, key: &'static str) {
		(**self).table(key)
	}
	fn ints(&mut self, values: &[u32]) {
		(**self).ints(values)
	}
	fn strs(&mut self, strings: &[(StrId, &str)]) {
		(**self).strs(strings)
	}
}

/// JSON text.
pub struct Json {
	out: String,
	/// Whether the next entry of the open container is its first.
	first: bool,
	/// The open containers, `true` for a list.
	stack: Vec<bool>,
}

impl Default for Json {
	fn default() -> Self {
		Json {
			out: String::new(),
			first: true,
			stack: Vec::new(),
		}
	}
}

impl Json {
	pub fn finish(self) -> String {
		self.out
	}

	fn sep(&mut self) {
		if !self.first {
			self.out.push(',');
		}
		self.first = false;
	}

	fn open(&mut self, list: bool) {
		self.sep();
		self.out.push(if list { '[' } else { '{' });
		self.stack.push(list);
		self.first = true;
	}
}

impl Sink for Json {
	fn begin(&mut self, ty: &'static str) {
		self.open(false);
		self.out.push_str("\"type\":\"");
		self.out.push_str(ty);
		self.out.push('"');
		self.first = false;
	}

	fn object(&mut self) {
		self.open(false);
	}

	fn list(&mut self) {
		self.open(true);
	}

	fn end(&mut self) {
		self.out.push(if self.stack.pop() == Some(true) { ']' } else { '}' });
		self.first = false;
	}

	fn key(&mut self, key: &'static str) {
		self.sep();
		self.out.push('"');
		self.out.push_str(key);
		self.out.push_str("\":");
		self.first = true;
	}

	fn int(&mut self, value: u32) {
		self.sep();
		push_int(&mut self.out, value);
	}

	fn float(&mut self, value: f64) {
		self.sep();
		write_number(&mut self.out, value);
	}

	fn bool(&mut self, value: bool) {
		self.sep();
		self.out.push_str(if value { "true" } else { "false" });
	}

	fn null(&mut self) {
		self.sep();
		self.out.push_str("null");
	}

	fn str(&mut self, value: &'static str) {
		self.text(value);
	}

	fn text(&mut self, value: &str) {
		self.sep();
		write_json_string(&mut self.out, value);
	}

	fn interned(&mut self, _id: StrId, value: &str) {
		self.text(value);
	}

	fn slice(&mut self, value: &str, _start: u32, _end: u32) {
		self.text(value);
	}

	// the two entries every node has, written in one piece: a measurable share of the text
	fn span(&mut self, start: u32, end: u32) {
		self.sep();
		self.out.push_str("\"start\":");
		push_int(&mut self.out, start);
		self.out.push_str(",\"end\":");
		push_int(&mut self.out, end);
	}

	fn loc(&mut self, start_line: u32, start_column: u32, end_line: u32, end_column: u32) {
		self.sep();
		self.out.push_str("\"loc\":{\"start\":{\"line\":");
		push_int(&mut self.out, start_line);
		self.out.push_str(",\"column\":");
		push_int(&mut self.out, start_column);
		self.out.push_str("},\"end\":{\"line\":");
		push_int(&mut self.out, end_line);
		self.out.push_str(",\"column\":");
		push_int(&mut self.out, end_column);
		self.out.push_str("}}");
	}
}

/// What a shape's entry holds, read by a binding's decoder without a tag.
pub mod kind {
	/// A shape id then the shape's values; `NULL` alone for null.
	pub const NODE: u32 = 0;
	pub const INT: u32 = 1;
	/// An index into the floats.
	pub const FLOAT: u32 = 2;
	/// 0 or 1.
	pub const BOOL: u32 = 3;
	/// A constant id.
	pub const CONST: u32 = 4;
	/// An index into the answer's own strings.
	pub const STR: u32 = 5;
	/// Two UTF-16 offsets into the source.
	pub const SLICE: u32 = 6;
	/// `loc`: start line and column, end line and column.
	pub const LOC: u32 = 7;
	/// Nodes up to an `END`.
	pub const NODES: u32 = 8;
	/// A count, then that many ints.
	pub const INTS: u32 = 9;
	/// A count, then that many indexes into the answer's strings.
	pub const STRS: u32 = 10;

	/// In a node's place.
	pub const NULL: u32 = 0;
	/// In a node's place, closing a list.
	pub const END: u32 = 1;
	/// The first shape id.
	pub const FIRST: u32 = 2;
}

/// The strings the writer names itself, numbered once per writer for every answer: a binding
/// fetches the list when an answer refers past what it has.
struct Constants {
	names: Vec<&'static str>,
	ids: FastMap<&'static str, u32>,
	// literals only: their address is a cheaper key than their text
	recent: Box<[(usize, u32); 512]>,
}

impl Constants {
	fn new() -> Self {
		Constants {
			names: Vec::new(),
			ids: FastMap::default(),
			recent: Box::new([(0, 0); 512]),
		}
	}

	fn id(&mut self, value: &'static str) -> u32 {
		let address = value.as_ptr() as usize;
		let slot = (address.wrapping_mul(crate::interner::SEED as usize) >> 55) & 511;
		if self.recent[slot].0 == address {
			return self.recent[slot].1;
		}
		let id = match self.ids.get(value) {
			Some(&id) => id,
			None => {
				let id = self.names.len() as u32;
				self.names.push(value);
				self.ids.insert(value, id);
				id
			}
		};
		self.recent[slot] = (address, id);
		id
	}
}

/// The shapes numbered so far by a writer, from `kind::FIRST`. A shape is what a node's
/// entries hold: its record is its type's constant id plus one (0 for a plain object), then a
/// word per entry, the key's constant id shifted left four with the value's `kind` in the low
/// bits. The records lie back to back, each behind its length.
struct Shapes {
	words: Vec<u32>,
	starts: Vec<u32>,
	ids: FastMap<Box<[u32]>, u32>,
	// the map lookup was a fifth of the encode; a hit here is a probe and a compare
	recent: Box<[(u32, u32); 1024]>,
}

impl Shapes {
	fn new() -> Self {
		Shapes {
			words: Vec::new(),
			starts: vec![0; kind::FIRST as usize],
			ids: FastMap::default(),
			recent: Box::new([(0, 0); 1024]),
		}
	}

	fn id(&mut self, record: &[u32]) -> u32 {
		// a hit is checked against the record, so the hash only has to spread: a sum does
		let mut hash = (record.len() as u64) << 32;
		for &word in record {
			hash = hash.wrapping_add(word as u64);
		}
		let hash = (hash.wrapping_mul(crate::interner::SEED) >> 32) as u32;
		let slot = (hash >> 22) as usize;
		let (seen, id) = self.recent[slot];
		if seen == hash && id != 0 {
			let start = self.starts[id as usize] as usize;
			if self.words[start] as usize == record.len() && self.words[start + 1..start + 1 + record.len()] == *record
			{
				return id;
			}
		}
		let id = match self.ids.get(record) {
			Some(&id) => id,
			None => {
				let id = self.starts.len() as u32;
				let start = self.words.len() as u32;
				self.starts.push(start);
				self.words.push(record.len() as u32);
				self.words.extend_from_slice(record);
				self.ids.insert(record.into(), id);
				id
			}
		};
		self.recent[slot] = (hash, id);
		id
	}
}

/// A tree packed into one buffer of 32-bit words, what a binding's JavaScript turns into objects
/// directly. A node is its shape id then its values in the shape's order, each read as the
/// shape's `kind` says. The header is seven counts: the tree's words, the strings, the floats,
/// the bytes of text, the constants and the shapes numbered when it was written, and where the
/// scope tables start in the tree's words, 0 for none. Then the tree, the strings' UTF-16 ends,
/// the text as UTF-8 padded to a word, padding to an even word, then the floats two words each.
/// The strings are the tree's interned ones first, then any text written for this answer. Words
/// are the host's endianness, which every target the package builds for shares with the
/// decoder's check.
/// The answer's words. A front end may take the allocation over and read the answer where it was
/// written; a growth after that leaves the allocation to its holder instead of freeing it.
pub struct Words {
	vec: Vec<u32>,
	owned: bool,
}

impl Words {
	const LEAST: usize = 1 << 16;

	pub fn new() -> Self {
		Self::default()
	}

	fn fresh() -> Self {
		Words {
			vec: Vec::new(),
			owned: true,
		}
	}

	pub fn as_ptr(&self) -> *const u32 {
		self.vec.as_ptr()
	}

	pub fn capacity(&self) -> usize {
		self.vec.capacity()
	}

	fn clear(&mut self) {
		self.vec.clear();
	}

	#[inline(always)]
	fn room(&mut self, more: usize) {
		if self.vec.capacity() - self.vec.len() < more {
			self.grow(more);
		}
	}

	#[inline(always)]
	fn push(&mut self, word: u32) {
		self.room(1);
		self.vec.push(word);
	}

	#[inline(always)]
	fn extend_from_slice(&mut self, words: &[u32]) {
		self.room(words.len());
		self.vec.extend_from_slice(words);
	}

	fn grow(&mut self, more: usize) {
		let cap = (self.vec.capacity() * 2).max(self.vec.len() + more).max(Self::LEAST);
		let mut next = Vec::with_capacity(cap);
		next.extend_from_slice(&self.vec);
		let old = std::mem::replace(&mut self.vec, next);
		if !self.owned {
			std::mem::forget(old);
		}
		self.owned = true;
	}

	/// Hands the allocation to the caller, who frees it as a `Vec<u32>` of that capacity;
	/// None when a caller already holds it.
	pub fn release(&mut self) -> Option<(*mut u32, usize)> {
		if !self.owned {
			return None;
		}
		if self.vec.capacity() == 0 {
			self.grow(0);
		}
		self.owned = false;
		Some((self.vec.as_mut_ptr(), self.vec.capacity()))
	}

	/// Starts the next answer on a fresh allocation of at least `cap` words.
	pub fn renew(&mut self, cap: usize) {
		let old = std::mem::replace(&mut self.vec, Vec::with_capacity(cap.max(Self::LEAST)));
		if !self.owned {
			std::mem::forget(old);
		}
		self.owned = true;
	}
}

impl Default for Words {
	fn default() -> Self {
		Self::fresh()
	}
}

impl Drop for Words {
	fn drop(&mut self) {
		if !self.owned {
			std::mem::forget(std::mem::take(&mut self.vec));
		}
	}
}

impl std::ops::Deref for Words {
	type Target = [u32];
	fn deref(&self) -> &[u32] {
		&self.vec
	}
}

impl std::ops::DerefMut for Words {
	fn deref_mut(&mut self) -> &mut [u32] {
		&mut self.vec
	}
}

impl Extend<u32> for Words {
	fn extend<I: IntoIterator<Item = u32>>(&mut self, iter: I) {
		let iter = iter.into_iter();
		match iter.size_hint() {
			// an exact size fills the room in one copy; a loose one must not let the Vec grow itself
			(lower, Some(upper)) if lower == upper => {
				self.room(upper);
				self.vec.extend(iter);
			}
			_ => {
				for word in iter {
					self.push(word);
				}
			}
		}
	}
}

pub struct Binary {
	words: Words,
	text: Vec<u8>,
	/// UTF-16 units of text so far.
	units: u32,
	ends: Vec<u32>,
	floats: Vec<f64>,
	frames: Vec<Frame>,
	// the open nodes' records; a list or a table pushes a scratch word so a value never checks where it is
	seq: Vec<u32>,
	tables_at: u32,
	tables: usize,
	start: u32,
	end: u32,
	loc: u32,
	constants: Constants,
	shapes: Shapes,
}

enum Frame {
	Node { slot: u32, record: u32 },
	List,
}

impl Default for Binary {
	fn default() -> Self {
		Self::new()
	}
}

impl Binary {
	pub fn new() -> Self {
		let mut binary = Binary {
			words: Words::new(),
			text: Vec::new(),
			units: 0,
			ends: Vec::new(),
			floats: Vec::new(),
			frames: Vec::new(),
			seq: Vec::new(),
			tables_at: 0,
			tables: 0,
			start: 0,
			end: 0,
			loc: 0,
			constants: Constants::new(),
			shapes: Shapes::new(),
		};
		binary.start = binary.constant("start") << 4 | kind::INT;
		binary.end = binary.constant("end") << 4 | kind::INT;
		binary.loc = binary.constant("loc") << 4 | kind::LOC;
		binary.reset();
		binary
	}

	pub(crate) fn constant(&mut self, value: &'static str) -> u32 {
		self.constants.id(value)
	}

	/// The constant strings numbered so far.
	pub fn constants(&self) -> &[&'static str] {
		&self.constants.names
	}

	/// The shape records numbered so far.
	pub fn shapes(&self) -> &[u32] {
		&self.shapes.words
	}

	pub fn words(&mut self) -> &mut Words {
		&mut self.words
	}

	/// Ready for an answer: the header's room in `words`, the sentinel in `seq`, the rest empty.
	pub fn reset(&mut self) {
		self.words.clear();
		self.words.extend_from_slice(&[0; 7]);
		self.text.clear();
		self.units = 0;
		self.ends.clear();
		self.floats.clear();
		self.frames.clear();
		self.seq.clear();
		self.seq.push(0);
		self.tables_at = 0;
		self.tables = 0;
	}

	fn push_text(&mut self, value: &str) -> u32 {
		self.text.extend_from_slice(value.as_bytes());
		self.units += if value.is_ascii() {
			value.len()
		} else {
			value.encode_utf16().count()
		} as u32;
		self.ends.push(self.units);
		self.ends.len() as u32 - 1
	}

	fn value(&mut self, kind: u32) {
		let last = self.seq.len() - 1;
		self.seq[last] |= kind;
	}

	fn open(&mut self, ty: u32) {
		self.value(kind::NODE);
		self.frames.push(Frame::Node {
			slot: self.words.len() as u32,
			record: self.seq.len() as u32,
		});
		self.seq.push(ty);
		self.words.push(0);
	}

	/// Completes the answer in `words`: the header, then the strings and floats after the tree.
	pub fn finish(&mut self) {
		debug_assert!(self.frames.is_empty() && self.seq.len() == 1);
		let tree = self.words.len() as u32 - 7;
		self.words[..7].copy_from_slice(&[
			tree,
			self.ends.len() as u32,
			self.floats.len() as u32,
			self.text.len() as u32,
			self.constants.names.len() as u32,
			self.shapes.starts.len() as u32,
			self.tables_at,
		]);
		self.words.extend_from_slice(&self.ends);
		let (chunks, rest) = self.text.as_chunks::<4>();
		self.words.extend(chunks.iter().map(|chunk| u32::from_ne_bytes(*chunk)));
		if !rest.is_empty() {
			let mut last = [0; 4];
			last[..rest.len()].copy_from_slice(rest);
			self.words.push(u32::from_ne_bytes(last));
		}
		if self.words.len() % 2 == 1 {
			self.words.push(0);
		}
		for &float in &self.floats {
			let bits = float.to_bits();
			self.words.extend_from_slice(&[bits as u32, (bits >> 32) as u32]);
		}
	}
}

impl Sink for Binary {
	fn strings(&mut self, interner: &Interner) {
		for i in 0..interner.len() {
			self.push_text(interner.get(StrId(i as u32)));
		}
	}

	fn begin(&mut self, ty: &'static str) {
		let id = self.constants.id(ty);
		self.open(id + 1);
	}

	fn object(&mut self) {
		self.open(0);
	}

	fn list(&mut self) {
		self.value(kind::NODES);
		self.frames.push(Frame::List);
		self.seq.push(0);
	}

	fn end(&mut self) {
		match self.frames.pop().expect("a container is open") {
			Frame::List => {
				self.words.push(kind::END);
				self.seq.pop();
			}
			Frame::Node { slot, record } => {
				let stop = self.seq.len() - if self.frames.is_empty() { self.tables } else { 0 };
				self.words[slot as usize] = self.shapes.id(&self.seq[record as usize..stop]);
				self.seq.truncate(record as usize);
			}
		}
	}

	fn key(&mut self, key: &'static str) {
		debug_assert!(self.tables_at == 0 || self.frames.len() > 1, "the tables come last");
		let id = self.constants.id(key);
		self.seq.push(id << 4);
	}

	fn int(&mut self, value: u32) {
		self.value(kind::INT);
		self.words.push(value);
	}

	fn float(&mut self, value: f64) {
		self.value(kind::FLOAT);
		self.words.push(self.floats.len() as u32);
		self.floats.push(value);
	}

	fn bool(&mut self, value: bool) {
		self.value(kind::BOOL);
		self.words.push(value as u32);
	}

	fn null(&mut self) {
		self.value(kind::NODE);
		self.words.push(kind::NULL);
	}

	fn str(&mut self, value: &'static str) {
		self.value(kind::CONST);
		let id = self.constants.id(value);
		self.words.push(id);
	}

	fn text(&mut self, value: &str) {
		self.value(kind::STR);
		let id = self.push_text(value);
		self.words.push(id);
	}

	fn interned(&mut self, id: StrId, _value: &str) {
		self.value(kind::STR);
		self.words.push(id.0);
	}

	fn slice(&mut self, _value: &str, start: u32, end: u32) {
		self.value(kind::SLICE);
		self.words.extend_from_slice(&[start, end]);
	}

	fn span(&mut self, start: u32, end: u32) {
		self.seq.extend([self.start, self.end]);
		self.words.extend_from_slice(&[start, end]);
	}

	fn loc(&mut self, start_line: u32, start_column: u32, end_line: u32, end_column: u32) {
		self.seq.push(self.loc);
		self.words
			.extend_from_slice(&[start_line, start_column, end_line, end_column]);
	}

	fn table(&mut self, _key: &'static str) {
		debug_assert!(self.frames.len() == 1, "a table is the root's entry");
		if self.tables_at == 0 {
			self.tables_at = self.words.len() as u32 - 7;
		}
		self.tables += 1;
		self.seq.push(0);
	}

	fn strs(&mut self, strings: &[(StrId, &str)]) {
		self.value(kind::STRS);
		self.words.push(strings.len() as u32);
		self.words.extend(strings.iter().map(|(id, _)| id.0));
	}

	fn ints(&mut self, values: &[u32]) {
		self.value(kind::INTS);
		self.words.push(values.len() as u32);
		self.words.extend_from_slice(values);
	}
}

/// Serializes an answer into `sink`: the roots as `node`, one node or the patterns of a
/// parameter list, then `end` and what the options add.
#[allow(clippy::too_many_arguments)]
pub fn answer<X: Emit, S: Sink>(
	ast: &Ast<X>,
	entry: Entry,
	roots: List,
	end: u32,
	source: &str,
	positions: &Positions,
	output: Output,
	sink: S,
) -> S {
	let mut w = Writer::new(ast, source, positions, sink);
	w.sink.strings(&ast.strings);
	w.output = output;
	w.sink.object();
	if entry == Entry::Params {
		w.list("node", roots);
	} else {
		w.field("node", ast.list(roots)[0].unwrap());
	}
	w.key("end");
	let end = w.positions.offset(&mut w.cursor, end);
	w.sink.int(end);
	w.trailers();
	w.sink.end();
	w.sink
}

/// Serializes a syntax error: its code and message, UTF-16 `pos` and `end`, and a `loc`; the
/// line table is built up to the error when `positions` has none.
pub fn error_to_json(error: &crate::SyntaxError, source: &str, positions: &Positions) -> String {
	let upto;
	let positions = if positions.lines {
		positions
	} else {
		let mut end = (error.pos.max(error.end) as usize).min(source.len());
		while !source.is_char_boundary(end) {
			end += 1;
		}
		upto = Positions::new(&source[..end], true);
		&upto
	};
	let mut cursor = Cursor::default();
	let pos = positions.offset(&mut cursor, error.pos);
	let (line, column) = positions.line_column(cursor.line, error.pos, pos);
	let end = positions.offset(&mut cursor, error.end);
	let mut out = format!("{{\"error\":{{\"code\":\"{}\",\"message\":", error.code.name());
	write_json_string(&mut out, &error.message);
	write!(
		out,
		",\"pos\":{pos},\"end\":{end},\"loc\":{{\"line\":{line},\"column\":{column}}}}}}}"
	)
	.unwrap();
	out
}

pub struct Writer<'a, X = (), S: Sink = Json> {
	ast: &'a Ast<X>,
	source: &'a str,
	pub(crate) sink: S,
	positions: &'a Positions,
	cursor: Cursor,
	pub(crate) output: Output,
	/// The node being written names something bound by another node: no scope facts on it.
	name_only: bool,
	/// What erasure left in place, in emission order.
	kept: Vec<(&'static str, NodeId)>,
	/// Nodes erasure skipped whose facts the next node written takes over.
	adopted: Vec<NodeId>,
}

/// Maps byte offsets to UTF-16 offsets and line/column pairs, the positions JavaScript counts,
/// and UTF-16 offsets back to bytes. Built once per source; a host that parses many expressions out of one
/// source keeps it.
pub struct Positions {
	/// After each non-ASCII character: its end byte offset and the bytes-minus-code-units gap so far.
	gaps: Vec<(u32, u32)>,
	/// Each line's start as a byte offset and a UTF-16 offset; only with `lines`.
	line_starts: Vec<(u32, u32)>,
	len: u32,
	lines: bool,
}

/// Where the last node's start landed: starts come in source order, and a node's end follows
/// its start, so each lookup tries a few entries on from its hint before a binary search.
#[derive(Default)]
struct Cursor {
	gap: usize,
	line: usize,
}

impl Positions {
	/// `lines` builds the line table, which only `loc` needs.
	pub fn new(source: &str, lines: bool) -> Self {
		let bytes = source.as_bytes();
		let mut gaps = Vec::new();
		let mut line_starts = vec![(0, 0)];
		let mut gap = 0u32;
		let mut i = 0;
		if !lines && bytes.is_ascii() {
			i = bytes.len();
		}
		while i < bytes.len() {
			i = if lines {
				crate::lexer::scan::find(bytes, i, *b"\n\r", true)
			} else {
				crate::lexer::scan::find(bytes, i, [], true)
			};
			let Some(&b) = bytes.get(i) else { break };
			if b < 0x80 {
				i += 1;
				if lines && (b == b'\n' || (b == b'\r' && bytes.get(i) != Some(&b'\n'))) {
					line_starts.push((i as u32, i as u32 - gap));
				}
			} else {
				let len = if b >= 0xf0 {
					4
				} else if b >= 0xe0 {
					3
				} else {
					2
				};
				let separator = len == 3 && crate::lexer::is_separator(&bytes[i..]);
				i += len;
				gap += len as u32 - if len == 4 { 2 } else { 1 };
				gaps.push((i as u32, gap));
				if lines && separator {
					line_starts.push((i as u32, i as u32 - gap));
				}
			}
		}
		Self {
			gaps,
			line_starts,
			len: source.len() as u32,
			lines,
		}
	}

	/// The byte offset of a UTF-16 offset, or why there is none; a position inside a surrogate
	/// pair maps to the byte after the character's first, which is not a character boundary.
	pub fn byte_offset(&self, utf16: f64) -> Result<u32, String> {
		if !(utf16 >= 0.0 && utf16.fract() == 0.0 && utf16 <= u32::MAX as f64) {
			return Err(format!("offset {utf16} is not a valid position"));
		}
		let target = utf16 as u32;
		let total_gap = self.gaps.last().map_or(0, |g| g.1);
		if target > self.len - total_gap {
			return Err(format!("offset {target} is past the end of the source"));
		}
		let i = self.gaps.partition_point(|g| g.0 - g.1 <= target);
		Ok(target + if i == 0 { 0 } else { self.gaps[i - 1].1 })
	}

	fn offset(&self, cursor: &mut Cursor, byte: u32) -> u32 {
		let (offset, gap) = self.offset_from(cursor.gap, byte);
		cursor.gap = gap;
		offset
	}

	/// The UTF-16 offset of `byte` and the gap entry it lies after, looked up from `hint`.
	fn offset_from(&self, hint: usize, byte: u32) -> (u32, usize) {
		let byte = byte.min(self.len);
		if self.gaps.is_empty() {
			return (byte, 0);
		}
		let gap = locate(&self.gaps, hint, byte, |g| g.0);
		(byte - if gap == 0 { 0 } else { self.gaps[gap - 1].1 }, gap)
	}

	/// The line of `byte` and its column, given `byte` already mapped by `offset`.
	fn line_column(&self, hint: usize, byte: u32, offset: u32) -> (usize, u32) {
		let byte = byte.min(self.len);
		let line = locate(&self.line_starts, hint.max(1), byte, |l| l.0);
		(line, offset - self.line_starts[line - 1].1)
	}
}

/// The number of `items` whose key is at most `byte`, trying `hint` and the few after it first.
fn locate<T>(items: &[T], hint: usize, byte: u32, key: impl Fn(&T) -> u32) -> usize {
	for p in hint..=(hint + 3).min(items.len()) {
		if (p == 0 || key(&items[p - 1]) <= byte) && (p == items.len() || key(&items[p]) > byte) {
			return p;
		}
	}
	items.partition_point(|item| key(item) <= byte)
}

impl<'a, X: Emit, S: Sink> Writer<'a, X, S> {
	fn new(ast: &'a Ast<X>, source: &'a str, positions: &'a Positions, sink: S) -> Self {
		Self {
			ast,
			source,
			sink,
			positions,
			cursor: Cursor::default(),
			output: Output::default(),
			name_only: false,
			kept: Vec::new(),
			adopted: Vec::new(),
		}
	}

	/// Records a node erasure had to leave in place.
	pub(crate) fn keep(&mut self, ty: &'static str, id: NodeId) {
		self.kept.push((ty, id));
	}

	/// What erasure left in place, as `typescript`, in source order.
	fn all_kept(&mut self) {
		self.key("typescript");
		self.sink.list();
		let mut kept = std::mem::take(&mut self.kept);
		kept.sort_unstable_by_key(|&(_, id)| self.ast.node(id).start);
		for &(ty, id) in &kept {
			self.sink.begin(ty);
			let node = self.ast.node(id);
			self.span(node.start, node.end);
			self.sink.end();
		}
		self.sink.end();
	}

	pub(crate) fn begin(&mut self, ty: &'static str, id: NodeId) {
		let node = self.ast.node(id);
		self.sink.begin(ty);
		self.span(node.start, node.end);
		self.scope_facts(id);
		if !self.ast.parenthesized.is_empty() && self.ast.parenthesized.binary_search(&id).is_ok() {
			self.bool("parenthesized", true);
		}
		self.ast.extension.extras(self, id);
		if let Some(&attached) = self.ast.attached.get(&id) {
			self.comments("leadingComments", attached.leading);
			self.comments("trailingComments", attached.trailing);
			self.comments("innerComments", attached.inner);
		}
	}

	/// The scope a node opens and what an identifier is: the binding it declares, or the one it
	/// refers to (null for a global) and whether it writes to it or mutates its value.
	fn scope_facts(&mut self, id: NodeId) {
		let Some(scopes) = &self.ast.scopes else { return };
		if self.name_only {
			return;
		}
		if let Some(scope) = scopes.of_node.get(id) {
			self.key("scope");
			self.sink.int(scope);
		}
		match scopes.of_identifier.get(id) {
			Some(Role::Declares(binding)) => {
				self.key("declares");
				self.sink.int(binding);
			}
			Some(Role::Reference(reference)) => {
				self.key("reference");
				self.sink.int(reference);
			}
			None => {}
		}
		let adopted = std::mem::take(&mut self.adopted);
		for node in adopted.iter().copied().chain([id]) {
			if let Some(&root) = scopes.root_of.get(&node) {
				self.key("root");
				self.sink.int(root);
			}
			let bindings = scopes.declared_by.get(node);
			if !bindings.is_empty() {
				self.key("defines");
				self.sink.ints(bindings);
			}
			let references = scopes.writes_of.get(node);
			if !references.is_empty() {
				self.key("writes");
				self.sink.ints(references);
			}
		}
	}

	/// The facts of a node erasure leaves out go on the next node written, the one standing in
	/// for it.
	pub(crate) fn adopt(&mut self, id: NodeId) {
		self.adopted.push(id);
	}

	/// The scope, binding and reference tables: what the `scope`, `declares`, `reference` and
	/// `writes` numbers index.
	fn all_scopes(&mut self) {
		let Some(scopes) = &self.ast.scopes else { return };
		self.sink.table("scopes");
		self.sink.list();
		for scope in &scopes.scopes {
			self.sink.object();
			self.string("kind", scope.kind.name());
			self.key("parent");
			match scope.parent {
				Some(parent) => self.sink.int(parent),
				None => self.sink.null(),
			}
			self.bool("topLevelAwait", scope.top_level_await);
			self.sink.end();
		}
		self.sink.end();
		self.sink.table("bindings");
		self.sink.list();
		for binding in &scopes.bindings {
			self.sink.object();
			self.interned("name", binding.name);
			self.string("kind", binding.kind.name());
			self.key("scope");
			self.sink.int(binding.scope);
			self.sink.end();
		}
		self.sink.end();
		self.sink.table("references");
		self.sink.list();
		for reference in &scopes.references {
			self.sink.object();
			self.key("scope");
			self.sink.int(reference.scope);
			self.key("binding");
			match reference.binding {
				Some(binding) => self.sink.int(binding),
				None => self.sink.null(),
			}
			self.bool("write", reference.write);
			self.bool("read", reference.read);
			self.bool("mutate", reference.mutate);
			self.sink.end();
		}
		self.sink.end();
		if self.ast.hosts.is_empty() {
			return;
		}
		self.sink.table("roots");
		self.sink.list();
		for root in &scopes.roots {
			self.sink.object();
			self.key("scope");
			self.sink.int(root.scope);
			for (key, (from, to)) in [
				("scopes", root.scopes),
				("bindings", root.bindings),
				("references", root.references),
			] {
				self.key(key);
				self.sink.ints(&[from, to]);
			}
			self.sink.end();
		}
		self.sink.end();
	}

	fn comments(&mut self, key: &'static str, comments: crate::ast::Run) {
		if !comments.is_empty() {
			self.key(key);
			self.comment_list(comments.indices());
		}
	}

	fn comment_list(&mut self, comments: impl IntoIterator<Item = u32>) {
		self.sink.list();
		for index in comments {
			let comment = self.ast.comments[index as usize];
			self.sink.begin(if comment.is_block() { "Block" } else { "Line" });
			self.key("value");
			let range = comment.text_range();
			self.slice(range.start as u32, range.end as u32);
			self.span(comment.start, comment.end);
			self.sink.end();
		}
		self.sink.end();
	}

	/// The recovered errors as the thrown one would be: code, message, `pos`, `end` and a `loc`.
	fn errors(&mut self) {
		let upto;
		let positions = if self.positions.lines || self.ast.errors.is_empty() {
			self.positions
		} else {
			let last = self.ast.errors.iter().map(|e| e.pos.max(e.end)).max().unwrap() as usize;
			let mut end = last.min(self.source.len());
			while !self.source.is_char_boundary(end) {
				end += 1;
			}
			upto = Positions::new(&self.source[..end], true);
			&upto
		};
		let mut cursor = Cursor::default();
		self.key("errors");
		self.sink.list();
		for error in &self.ast.errors {
			let pos = positions.offset(&mut cursor, error.pos);
			let (line, column) = positions.line_column(cursor.line, error.pos, pos);
			let end = positions.offset(&mut cursor, error.end);
			self.sink.object();
			self.key("code");
			self.sink.str(error.code.name());
			self.key("message");
			self.sink.text(&error.message);
			self.key("pos");
			self.sink.int(pos);
			self.key("end");
			self.sink.int(end);
			self.key("loc");
			self.sink.object();
			self.key("line");
			self.sink.int(line as u32);
			self.key("column");
			self.sink.int(column);
			self.sink.end();
			self.sink.end();
		}
		self.sink.end();
	}

	/// What the output's switches add after a root: every comment, what erasure kept, the scopes.
	fn trailers(&mut self) {
		if self.output.comments {
			self.key("comments");
			self.comment_list(0..self.ast.comments.len() as u32);
		}
		if self.output.errors {
			self.errors();
		}
		if self.output.erase {
			self.all_kept();
		}
		if self.output.scopes {
			self.all_scopes();
		}
	}

	pub(crate) fn span(&mut self, start: u32, end: u32) {
		let (start_offset, gap) = self.positions.offset_from(self.cursor.gap, start);
		self.cursor.gap = gap;
		let (end_offset, _) = self.positions.offset_from(gap, end);
		self.sink.span(start_offset, end_offset);
		if !self.positions.lines {
			return;
		}
		let (sl, sc) = self.positions.line_column(self.cursor.line, start, start_offset);
		self.cursor.line = sl;
		let (el, ec) = self.positions.line_column(sl, end, end_offset);
		self.sink.loc(sl as u32, sc, el as u32, ec);
	}

	/// The source between two byte offsets, as a string value.
	fn slice(&mut self, start: u32, end: u32) {
		let (start_offset, end_offset) = (
			self.positions.offset(&mut self.cursor, start),
			self.positions.offset(&mut self.cursor, end),
		);
		self.sink
			.slice(&self.source[start as usize..end as usize], start_offset, end_offset);
	}

	pub(crate) fn end(&mut self) {
		self.sink.end();
	}

	pub(crate) fn key(&mut self, key: &'static str) {
		self.sink.key(key);
	}

	pub(crate) fn field(&mut self, key: &'static str, id: NodeId) {
		self.key(key);
		self.node(id);
	}

	/// A specifier's other name, which is the same node as the binding one in `import { a }`
	/// and `export { a }`, and then only names: the binding facts stay on the binding one.
	fn other_name(&mut self, key: &'static str, id: NodeId, binding: NodeId) {
		let was = self.name_only;
		self.name_only = id == binding;
		self.field(key, id);
		self.name_only = was;
	}

	pub(crate) fn kind(&self, id: NodeId) -> NodeKind {
		self.ast.node(id).kind
	}

	pub(crate) fn ast(&self) -> &'a Ast<X> {
		self.ast
	}

	/// The key only when there is a node; an unset property is left out.
	pub(crate) fn opt_key(&mut self, key: &'static str, id: Option<NodeId>) {
		if let Some(id) = id {
			self.field(key, id);
		}
	}

	pub(crate) fn opt(&mut self, key: &'static str, id: Option<NodeId>) {
		self.key(key);
		match id {
			Some(id) => self.node(id),
			None => self.sink.null(),
		}
	}

	pub(crate) fn list(&mut self, key: &'static str, list: List) {
		self.key(key);
		self.sink.list();
		let ast = self.ast;
		for item in ast.list(list) {
			if self.output.erase && item.is_some_and(|id| ast.extension.erased(self, id)) {
				continue;
			}
			match item {
				Some(id) => self.node(*id),
				None => self.sink.null(),
			}
		}
		self.sink.end();
	}

	/// A parameter list; erasing drops TypeScript's `this` parameter.
	pub(crate) fn params(&mut self, key: &'static str, list: List) {
		if self.output.erase
			&& let Some(&Some(first)) = self.ast.list(list).first()
			&& let NodeKind::Identifier { name } = self.kind(first)
			&& self.ast.str(name) == "this"
		{
			let rest = List {
				start: list.start + 1,
				len: list.len - 1,
			};
			return self.list(key, rest);
		}
		self.list(key, list)
	}

	pub(crate) fn bool(&mut self, key: &'static str, value: bool) {
		self.key(key);
		self.sink.bool(value);
	}

	pub(crate) fn string(&mut self, key: &'static str, value: &'static str) {
		self.key(key);
		self.sink.str(value);
	}

	/// A string computed for this tree.
	pub(crate) fn text(&mut self, key: &'static str, value: &str) {
		self.key(key);
		self.sink.text(value);
	}

	pub(crate) fn interned(&mut self, key: &'static str, id: StrId) {
		self.key(key);
		self.sink.interned(id, self.ast.str(id));
	}

	pub(crate) fn raw(&mut self, id: NodeId) {
		let node = self.ast.node(id);
		self.key("raw");
		self.slice(node.start, node.end);
	}

	fn class(&mut self, class: Class) {
		self.opt("id", class.id);
		self.opt("superClass", class.super_class);
		self.field("body", class.body);
	}

	fn function(&mut self, f: Function, expression: bool) {
		self.opt("id", f.id);
		self.bool("expression", expression);
		self.bool("generator", f.generator);
		self.bool("async", f.is_async);
		self.params("params", f.params);
		self.field("body", f.body);
	}

	pub(crate) fn node(&mut self, id: NodeId) {
		use NodeKind::*;
		let kind = self.ast.node(id).kind;
		match kind {
			Program { body, module } => {
				self.begin("Program", id);
				self.list("body", body);
				self.string("sourceType", if module { "module" } else { "script" });
			}
			Identifier { name } => {
				self.begin("Identifier", id);
				self.interned("name", name);
			}
			PrivateIdentifier { name } => {
				self.begin("PrivateIdentifier", id);
				self.interned("name", name);
			}
			NumberLiteral { value } => {
				let value = self.ast.numbers[value as usize];
				self.begin("Literal", id);
				self.key("value");
				if value.is_finite() {
					self.sink.float(value);
				} else {
					self.sink.null();
				}
				self.raw(id);
			}
			BigIntLiteral => {
				self.begin("Literal", id);
				self.key("value");
				self.sink.null();
				self.raw(id);
				let node = self.ast.node(id);
				let raw = &self.source[node.start as usize..node.end as usize - 1];
				let bigint = bigint_decimal(raw);
				self.text("bigint", &bigint);
			}
			StringLiteral { value } => {
				self.begin("Literal", id);
				self.interned("value", value);
				self.raw(id);
			}
			BooleanLiteral { value } => {
				self.begin("Literal", id);
				self.bool("value", value);
				self.raw(id);
			}
			NullLiteral => {
				self.begin("Literal", id);
				self.key("value");
				self.sink.null();
				self.raw(id);
			}
			RegExpLiteral { pattern, flags } => {
				self.begin("Literal", id);
				self.key("value");
				self.sink.null();
				self.raw(id);
				self.key("regex");
				self.sink.object();
				self.interned("pattern", pattern);
				self.interned("flags", flags);
				self.sink.end();
			}
			TemplateLiteral { quasis, expressions } => {
				self.begin("TemplateLiteral", id);
				self.list("expressions", expressions);
				self.list("quasis", quasis);
			}
			TemplateElement { cooked, raw, tail } => {
				self.begin("TemplateElement", id);
				self.key("value");
				self.sink.object();
				self.interned("raw", raw);
				self.key("cooked");
				match cooked {
					Some(cooked) => self.sink.interned(cooked, self.ast.str(cooked)),
					None => self.sink.null(),
				}
				self.sink.end();
				self.bool("tail", tail);
			}
			TaggedTemplateExpression { tag, quasi } => {
				self.begin("TaggedTemplateExpression", id);
				self.field("tag", tag);
				self.field("quasi", quasi);
			}
			ThisExpression => self.begin("ThisExpression", id),
			Super => self.begin("Super", id),
			ArrayExpression { elements } => {
				self.begin("ArrayExpression", id);
				self.list("elements", elements);
			}
			ObjectExpression { properties } => {
				self.begin("ObjectExpression", id);
				self.list("properties", properties);
			}
			Property {
				key,
				value,
				kind,
				computed,
				method,
				shorthand,
			} => {
				self.begin("Property", id);
				self.bool("method", method);
				self.bool("shorthand", shorthand);
				self.bool("computed", computed);
				self.field("key", key);
				self.field("value", value);
				self.string(
					"kind",
					match kind {
						PropertyKind::Init => "init",
						PropertyKind::Get => "get",
						PropertyKind::Set => "set",
					},
				);
			}
			SpreadElement { argument } => {
				self.begin("SpreadElement", id);
				self.field("argument", argument);
			}
			UnaryExpression { operator, argument } => {
				self.begin("UnaryExpression", id);
				self.string("operator", operator.as_str());
				self.bool("prefix", true);
				self.field("argument", argument);
			}
			UpdateExpression {
				operator,
				prefix,
				argument,
			} => {
				self.begin("UpdateExpression", id);
				self.string("operator", operator.as_str());
				self.bool("prefix", prefix);
				self.field("argument", argument);
			}
			BinaryExpression { operator, left, right } => {
				self.begin("BinaryExpression", id);
				self.field("left", left);
				self.string("operator", operator.as_str());
				self.field("right", right);
			}
			LogicalExpression { operator, left, right } => {
				self.begin("LogicalExpression", id);
				self.field("left", left);
				self.string("operator", operator.as_str());
				self.field("right", right);
			}
			AssignmentExpression { operator, left, right } => {
				self.begin("AssignmentExpression", id);
				self.string("operator", operator.as_str());
				self.field("left", left);
				self.field("right", right);
			}
			ConditionalExpression {
				test,
				consequent,
				alternate,
			} => {
				self.begin("ConditionalExpression", id);
				self.field("test", test);
				self.field("consequent", consequent);
				self.field("alternate", alternate);
			}
			MemberExpression {
				object,
				property,
				computed,
				optional,
			} => {
				self.begin("MemberExpression", id);
				self.field("object", object);
				self.field("property", property);
				self.bool("computed", computed);
				self.bool("optional", optional);
			}
			CallExpression {
				callee,
				arguments,
				optional,
			} => {
				self.begin("CallExpression", id);
				self.field("callee", callee);
				self.list("arguments", arguments);
				self.bool("optional", optional);
			}
			ChainExpression { expression } => {
				self.begin("ChainExpression", id);
				self.field("expression", expression);
			}
			NewExpression { callee, arguments } => {
				self.begin("NewExpression", id);
				self.field("callee", callee);
				self.list("arguments", arguments);
			}
			SequenceExpression { expressions } => {
				self.begin("SequenceExpression", id);
				self.list("expressions", expressions);
			}
			ArrowFunctionExpression {
				params,
				body,
				expression,
				is_async,
			} => {
				self.begin("ArrowFunctionExpression", id);
				self.key("id");
				self.sink.null();
				self.bool("expression", expression);
				self.bool("generator", false);
				self.bool("async", is_async);
				self.params("params", params);
				self.field("body", body);
			}
			FunctionExpression { function } => {
				self.begin("FunctionExpression", id);
				self.function(function, false);
			}
			FunctionDeclaration { function } => {
				self.begin("FunctionDeclaration", id);
				self.function(function, false);
			}
			ClassExpression { class } => {
				self.begin("ClassExpression", id);
				self.class(class);
			}
			ClassDeclaration { class } => {
				self.begin("ClassDeclaration", id);
				self.class(class);
			}
			ClassBody { body } => {
				self.begin("ClassBody", id);
				self.list("body", body);
			}
			MethodDefinition {
				key,
				value,
				kind,
				computed,
				is_static,
			} => {
				self.begin("MethodDefinition", id);
				self.bool("static", is_static);
				self.bool("computed", computed);
				self.field("key", key);
				self.string(
					"kind",
					match kind {
						MethodKind::Constructor => "constructor",
						MethodKind::Method => "method",
						MethodKind::Get => "get",
						MethodKind::Set => "set",
					},
				);
				self.field("value", value);
			}
			PropertyDefinition {
				key,
				value,
				computed,
				is_static,
			} => {
				self.begin("PropertyDefinition", id);
				self.bool("static", is_static);
				self.bool("computed", computed);
				self.field("key", key);
				self.opt("value", value);
			}
			StaticBlock { body } => {
				self.begin("StaticBlock", id);
				self.list("body", body);
			}
			YieldExpression { argument, delegate } => {
				self.begin("YieldExpression", id);
				self.bool("delegate", delegate);
				self.opt("argument", argument);
			}
			AwaitExpression { argument } => {
				self.begin("AwaitExpression", id);
				self.field("argument", argument);
			}
			MetaProperty { meta, property } => {
				self.begin("MetaProperty", id);
				self.field("meta", meta);
				self.field("property", property);
			}
			ImportExpression { source, options } => {
				self.begin("ImportExpression", id);
				self.field("source", source);
				self.opt("options", options);
			}
			ObjectPattern { properties } => {
				self.begin("ObjectPattern", id);
				self.list("properties", properties);
			}
			ArrayPattern { elements } => {
				self.begin("ArrayPattern", id);
				self.list("elements", elements);
			}
			RestElement { argument } => {
				self.begin("RestElement", id);
				self.field("argument", argument);
			}
			AssignmentPattern { left, right } => {
				self.begin("AssignmentPattern", id);
				self.field("left", left);
				self.field("right", right);
			}
			ExpressionStatement { expression, directive } => {
				self.begin("ExpressionStatement", id);
				self.field("expression", expression);
				if let Some(directive) = directive {
					self.interned("directive", directive);
				}
			}
			BlockStatement { body } => {
				self.begin("BlockStatement", id);
				self.list("body", body);
			}
			EmptyStatement => self.begin("EmptyStatement", id),
			DebuggerStatement => self.begin("DebuggerStatement", id),
			WithStatement { object, body } => {
				self.begin("WithStatement", id);
				self.field("object", object);
				self.field("body", body);
			}
			ReturnStatement { argument } => {
				self.begin("ReturnStatement", id);
				self.opt("argument", argument);
			}
			LabeledStatement { label, body } => {
				self.begin("LabeledStatement", id);
				self.field("body", body);
				self.field("label", label);
			}
			BreakStatement { label } => {
				self.begin("BreakStatement", id);
				self.opt("label", label);
			}
			ContinueStatement { label } => {
				self.begin("ContinueStatement", id);
				self.opt("label", label);
			}
			IfStatement {
				test,
				consequent,
				alternate,
			} => {
				self.begin("IfStatement", id);
				self.field("test", test);
				self.field("consequent", consequent);
				self.opt("alternate", alternate);
			}
			SwitchStatement { discriminant, cases } => {
				self.begin("SwitchStatement", id);
				self.field("discriminant", discriminant);
				self.list("cases", cases);
			}
			SwitchCase { test, consequent } => {
				self.begin("SwitchCase", id);
				self.list("consequent", consequent);
				self.opt("test", test);
			}
			ThrowStatement { argument } => {
				self.begin("ThrowStatement", id);
				self.field("argument", argument);
			}
			TryStatement {
				block,
				handler,
				finalizer,
			} => {
				self.begin("TryStatement", id);
				self.field("block", block);
				self.opt("handler", handler);
				self.opt("finalizer", finalizer);
			}
			CatchClause { param, body } => {
				self.begin("CatchClause", id);
				self.opt("param", param);
				self.field("body", body);
			}
			WhileStatement { test, body } => {
				self.begin("WhileStatement", id);
				self.field("test", test);
				self.field("body", body);
			}
			DoWhileStatement { body, test } => {
				self.begin("DoWhileStatement", id);
				self.field("body", body);
				self.field("test", test);
			}
			ForStatement {
				init,
				test,
				update,
				body,
			} => {
				self.begin("ForStatement", id);
				self.opt("init", init);
				self.opt("test", test);
				self.opt("update", update);
				self.field("body", body);
			}
			ForInStatement { left, right, body } => {
				self.begin("ForInStatement", id);
				self.field("left", left);
				self.field("right", right);
				self.field("body", body);
			}
			ForOfStatement {
				left,
				right,
				body,
				is_await,
			} => {
				self.begin("ForOfStatement", id);
				self.bool("await", is_await);
				self.field("left", left);
				self.field("right", right);
				self.field("body", body);
			}
			VariableDeclaration { declarations, kind } => {
				self.begin("VariableDeclaration", id);
				self.list("declarations", declarations);
				self.string("kind", kind.as_str());
			}
			VariableDeclarator { id: pattern, init } => {
				self.begin("VariableDeclarator", id);
				self.field("id", pattern);
				self.opt("init", init);
			}
			ImportDeclaration {
				specifiers,
				source,
				attributes,
			} => {
				self.begin("ImportDeclaration", id);
				self.list("specifiers", specifiers);
				self.field("source", source);
				self.list("attributes", attributes);
			}
			ImportSpecifier { imported, local } => {
				self.begin("ImportSpecifier", id);
				self.other_name("imported", imported, local);
				self.field("local", local);
			}
			ImportDefaultSpecifier { local } => {
				self.begin("ImportDefaultSpecifier", id);
				self.field("local", local);
			}
			ImportNamespaceSpecifier { local } => {
				self.begin("ImportNamespaceSpecifier", id);
				self.field("local", local);
			}
			ImportAttribute { key, value } => {
				self.begin("ImportAttribute", id);
				self.field("key", key);
				self.field("value", value);
			}
			ExportDeclaration { declaration } => {
				self.begin("ExportNamedDeclaration", id);
				self.field("declaration", declaration);
				self.list("specifiers", List::EMPTY);
				self.opt("source", None);
				self.list("attributes", List::EMPTY);
			}
			ExportNamedDeclaration {
				specifiers,
				source,
				attributes,
			} => {
				self.begin("ExportNamedDeclaration", id);
				self.opt("declaration", None);
				self.list("specifiers", specifiers);
				self.opt("source", source);
				self.list("attributes", attributes);
			}
			ExportSpecifier { local, exported } => {
				self.begin("ExportSpecifier", id);
				self.field("local", local);
				self.other_name("exported", exported, local);
			}
			ExportDefaultDeclaration { declaration } => {
				self.begin("ExportDefaultDeclaration", id);
				self.field("declaration", declaration);
			}
			ExportAllDeclaration {
				exported,
				source,
				attributes,
			} => {
				self.begin("ExportAllDeclaration", id);
				self.opt("exported", exported);
				self.field("source", source);
				self.list("attributes", attributes);
			}
			// the extension closes its own node, since erasing may put another in its place
			Extension(index) => return self.ast.extension.node(self, id, index),
			Host(index) => {
				let host = self.ast.hosts[index as usize];
				if host.ty.is_empty() {
					// an object of the host's without a type, positions and all
					let node = self.ast.node(id);
					self.sink.object();
					self.span(node.start, node.end);
				} else if host.span {
					self.begin(host.ty, id);
				} else {
					self.sink.begin(host.ty);
					self.scope_facts(id);
				}
				let (from, len) = host.fields;
				for i in from..from + len {
					let (key, value) = self.ast.host_fields[i as usize];
					match value {
						Value::Node(child) => self.field(key, child),
						Value::Nodes(children) => self.list(key, children),
						Value::Str(string) => self.interned(key, string),
						Value::Slice(start, end) => {
							self.key(key);
							self.slice(start, end);
						}
						Value::Strs(start, len) => {
							let strings: Vec<(StrId, &str)> = self.ast.host_strings
								[start as usize..(start + len) as usize]
								.iter()
								.map(|&string| (string, self.ast.str(string)))
								.collect();
							self.key(key);
							self.sink.strs(&strings);
						}
						Value::Bool(value) => self.bool(key, value),
						Value::Int(value) => {
							self.key(key);
							self.sink.int(value);
						}
						Value::Null => {
							self.key(key);
							self.sink.null();
						}
						Value::Comments => {
							self.key(key);
							self.comment_list(0..self.ast.comments.len() as u32);
						}
					}
				}
			}
		}
		self.end();
	}
}

pub(crate) fn push_int(out: &mut String, mut value: u32) {
	const DIGITS: &[u8; 200] = b"0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";
	let mut buf = [0u8; 10];
	let mut i = buf.len();
	while value >= 100 {
		let pair = (value % 100) as usize * 2;
		i -= 2;
		buf[i..i + 2].copy_from_slice(&DIGITS[pair..pair + 2]);
		value /= 100;
	}
	if value >= 10 {
		let pair = value as usize * 2;
		i -= 2;
		buf[i..i + 2].copy_from_slice(&DIGITS[pair..pair + 2]);
	} else {
		i -= 1;
		buf[i] = b'0' + value as u8;
	}
	out.push_str(std::str::from_utf8(&buf[i..]).unwrap());
}

/// The decimal digits of a BigInt literal's text, without the `n`.
fn bigint_decimal(raw: &str) -> String {
	let (radix, digits) = match raw.get(..2) {
		Some("0x" | "0X") => (16, &raw[2..]),
		Some("0o" | "0O") => (8, &raw[2..]),
		Some("0b" | "0B") => (2, &raw[2..]),
		_ => return raw.replace('_', ""),
	};
	let mut limbs: Vec<u32> = vec![0];
	for digit in digits.bytes().filter(|b| *b != b'_') {
		let mut carry = (digit as char).to_digit(radix).unwrap() as u64;
		for limb in limbs.iter_mut() {
			let v = *limb as u64 * radix as u64 + carry;
			*limb = (v % 1_000_000_000) as u32;
			carry = v / 1_000_000_000;
		}
		if carry > 0 {
			limbs.push(carry as u32);
		}
	}
	use std::fmt::Write;
	let mut out = String::with_capacity(limbs.len() * 9);
	write!(out, "{}", limbs.last().unwrap()).unwrap();
	for limb in limbs.iter().rev().skip(1) {
		write!(out, "{limb:09}").unwrap();
	}
	out
}

pub fn write_number(out: &mut String, value: f64) {
	if !value.is_finite() {
		out.push_str("null");
		return;
	}
	if value == 0.0 {
		out.push('0');
		return;
	}
	let mut formatted = Digits::<32>::new();
	write!(formatted, "{value:e}").unwrap();
	let (mantissa, exponent) = formatted.as_str().split_once('e').unwrap();
	let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
	let mut digits = Digits::<32>::new();
	for byte in mantissa.bytes().filter(|b| *b != b'.') {
		digits.buf[digits.len] = byte;
		digits.len += 1;
	}
	even_on_tie(value.abs(), &mut digits.buf[..digits.len]);
	let digits = digits.as_str();
	let k = digits.len() as i32;
	let n = exponent.parse::<i32>().unwrap() + 1;
	out.push_str(sign);
	if k <= n && n <= 21 {
		out.push_str(digits);
		out.extend(std::iter::repeat_n('0', (n - k) as usize));
	} else if 0 < n && n <= 21 {
		out.push_str(&digits[..n as usize]);
		out.push('.');
		out.push_str(&digits[n as usize..]);
	} else if -6 < n && n <= 0 {
		out.push_str("0.");
		out.extend(std::iter::repeat_n('0', (-n) as usize));
		out.push_str(digits);
	} else {
		out.push_str(&digits[..1]);
		if k > 1 {
			out.push('.');
			out.push_str(&digits[1..]);
		}
		out.push('e');
		out.push(if n > 1 { '+' } else { '-' });
		push_int(out, (n - 1).unsigned_abs());
	}
}

/// The digits of one number, written on the stack.
struct Digits<const N: usize> {
	buf: [u8; N],
	len: usize,
}

impl<const N: usize> Digits<N> {
	fn new() -> Self {
		Digits { buf: [0; N], len: 0 }
	}

	fn as_str(&self) -> &str {
		std::str::from_utf8(&self.buf[..self.len]).unwrap()
	}
}

impl<const N: usize> std::fmt::Write for Digits<N> {
	fn write_str(&mut self, s: &str) -> std::fmt::Result {
		let end = self.len + s.len();
		if end > N {
			return Err(std::fmt::Error);
		}
		self.buf[self.len..end].copy_from_slice(s.as_bytes());
		self.len = end;
		Ok(())
	}
}

/// Rust rounds the shortest digits away from zero on an exact tie; JavaScript takes the even ones.
fn even_on_tie(value: f64, digits: &mut [u8]) {
	let k = digits.len();
	// two shortest forms sit at the same distance only at the edge of what a double resolves
	if k < 16 || digits[k - 1].is_multiple_of(2) {
		return;
	}
	let exact = format!("{value:.*e}", 1100);
	let all: Vec<u8> = exact
		.split_once('e')
		.unwrap()
		.0
		.bytes()
		.filter(|b| *b != b'.')
		.collect();
	if all[k] != b'5' || all[k + 1..].iter().any(|&b| b != b'0') {
		return;
	}
	let mut lower = digits.to_vec();
	lower[k - 1] -= 1;
	let lower = std::str::from_utf8(&lower).unwrap();
	let exponent = exact.split_once('e').unwrap().1;
	if let Ok(back) = format!("{}.{}e{exponent}", &lower[..1], &lower[1..]).parse::<f64>()
		&& back == value
	{
		digits[k - 1] -= 1;
	}
}

pub(crate) fn write_json_string(out: &mut String, s: &str) {
	out.push('"');
	let mut from = 0;
	for (i, b) in s.bytes().enumerate() {
		if b >= 0x20 && b != b'"' && b != b'\\' {
			continue;
		}
		out.push_str(&s[from..i]);
		match b {
			b'"' => out.push_str("\\\""),
			b'\\' => out.push_str("\\\\"),
			b'\n' => out.push_str("\\n"),
			b'\r' => out.push_str("\\r"),
			b'\t' => out.push_str("\\t"),
			8 => out.push_str("\\b"),
			12 => out.push_str("\\f"),
			_ => write!(out, "\\u{b:04x}").unwrap(),
		}
		from = i + 1;
	}
	out.push_str(&s[from..]);
	out.push('"');
}

#[cfg(test)]
mod tests {
	#[test]
	#[allow(clippy::excessive_precision)]
	fn numbers_as_javascript_writes_them() {
		for (value, text) in [
			(1e-14, "1e-14"),
			(1e-7, "1e-7"),
			(0.000001, "0.000001"),
			(0.1, "0.1"),
			(1.5, "1.5"),
			(123.456, "123.456"),
			(100.0, "100"),
			(1e20, "100000000000000000000"),
			(1e21, "1e+21"),
			(1.5e300, "1.5e+300"),
			(5e-324, "5e-324"),
			(-0.0, "0"),
			(-2.5e-8, "-2.5e-8"),
			(132405809496.45312, "132405809496.45312"),
			(0.3, "0.3"),
			(9007199254740993.0, "9007199254740992"),
			(1658206780088562.25, "1658206780088562.2"),
			(662936471232937.25, "662936471232937.2"),
			(f64::INFINITY, "null"),
		] {
			let mut out = String::new();
			super::write_number(&mut out, value);
			assert_eq!(out, text, "{value:?}");
		}
	}

	use super::Positions;

	#[test]
	fn byte_offsets() {
		let positions = Positions::new("aé𝒳b", false);
		let bytes: Vec<_> = (0..=6).map(|n| positions.byte_offset(n as f64).ok()).collect();
		assert_eq!(bytes, [Some(0), Some(1), Some(3), Some(4), Some(7), Some(8), None]);
		assert!(positions.byte_offset(-1.0).is_err());
		assert!(positions.byte_offset(1.5).is_err());
		assert_eq!(Positions::new("abc", true).byte_offset(3.0), Ok(3));
	}

	#[test]
	fn binary_layout() {
		use super::{Binary, Sink, kind};
		use crate::interner::Interner;
		let mut interner = Interner::default();
		interner.intern("a");
		let mut b = Binary::new();
		b.strings(&interner);
		b.object();
		b.key("node");
		b.begin("Identifier");
		b.span(1, 2);
		b.key("name");
		b.interned(crate::interner::StrId(0), "a");
		b.key("value");
		b.float(1.5);
		b.key("raw");
		b.text("\u{1F600}b");
		b.key("list");
		b.list();
		b.int(3);
		b.null();
		b.end();
		b.end();
		b.table("scopes");
		b.list();
		b.object();
		b.key("through");
		b.ints(&[7]);
		b.end();
		b.end();
		b.end();
		b.finish();
		let words = b.words().to_vec();
		let [tree, strings, floats, bytes, known, known_shapes, tables_at] = words[..7] else {
			unreachable!()
		};
		assert_eq!((tree, strings, floats, bytes), (14, 2, 1, 6));
		assert!(known >= 5 && known_shapes >= kind::FIRST + 3);
		let body = &words[7..7 + tree as usize];
		let (root, node, scope) = (body[0], body[1], body[10]);
		assert_eq!(&body[2..10], &[1, 2, 0, 0, 1, 3, kind::NULL, kind::END]);
		assert_eq!(&body[11..], &[1, 7, kind::END]);
		assert_eq!(tables_at, 10);
		let all = b.shapes().to_vec();
		let record = |id: u32| {
			let mut at = 0;
			for _ in kind::FIRST..id {
				at += all[at] as usize + 1;
			}
			&all[at + 1..at + 1 + all[at] as usize]
		};
		assert_eq!(record(root), &[0, b.constant("node") << 4 | kind::NODE]);
		assert_eq!(
			record(node),
			&[
				b.constant("Identifier") + 1,
				b.constant("start") << 4 | kind::INT,
				b.constant("end") << 4 | kind::INT,
				b.constant("name") << 4 | kind::STR,
				b.constant("value") << 4 | kind::FLOAT,
				b.constant("raw") << 4 | kind::STR,
				b.constant("list") << 4 | kind::NODES,
			]
		);
		assert_eq!(record(scope), &[0, b.constant("through") << 4 | kind::INTS]);
		let ends = &words[7 + tree as usize..][..2];
		assert_eq!(ends, &[1, 4]);
		let text_at = 7 + tree as usize + 2;
		assert_eq!(&words[text_at].to_ne_bytes(), &[b'a', 0xf0, 0x9f, 0x98]);
		assert_eq!(&words[text_at + 1].to_ne_bytes(), &[0x80, b'b', 0, 0]);
		let floats_at = (text_at + 2).next_multiple_of(2);
		let bits = words[floats_at] as u64 | (words[floats_at + 1] as u64) << 32;
		assert_eq!(f64::from_bits(bits), 1.5);
		assert_eq!(words.len(), floats_at + 2);
	}

	// cargo test --release hot_paths -- --ignored --nocapture
	#[test]
	#[ignore]
	fn hot_paths() {
		use super::{Binary, kind};
		let mut b = Binary::new();
		let record = [
			b.constant("Identifier") + 1,
			b.constant("start") << 4 | kind::INT,
			b.constant("end") << 4 | kind::INT,
			b.constant("binding") << 4 | kind::INT,
			b.constant("name") << 4 | kind::STR,
		];
		let other = [
			b.constant("Literal") + 1,
			b.constant("start") << 4 | kind::INT,
			b.constant("end") << 4 | kind::INT,
			b.constant("value") << 4 | kind::STR,
			b.constant("raw") << 4 | kind::SLICE,
		];
		let n = 10_000_000u32;
		let mut sink = 0u32;
		let t = std::time::Instant::now();
		for i in 0..n {
			sink = sink.wrapping_add(b.shapes.id(if i & 1 == 0 { &record } else { &other }));
		}
		eprintln!("shape hit {:.1} ns", t.elapsed().as_nanos() as f64 / n as f64);
		let keys = ["name", "start", "end", "body", "expression", "value", "raw", "id"];
		let t = std::time::Instant::now();
		for i in 0..n {
			sink = sink.wrapping_add(b.constant(keys[(i & 7) as usize]));
		}
		eprintln!(
			"constant hit {:.1} ns  ({sink})",
			t.elapsed().as_nanos() as f64 / n as f64
		);
	}
}
