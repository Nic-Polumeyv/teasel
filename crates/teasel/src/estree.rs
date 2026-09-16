//! Serializes an `Ast` to ESTree: as JSON text, or as a token stream a binding hands to
//! JavaScript without a text round trip.

use crate::ast::{Ast, List, NodeId, NodeKind, Value};
use crate::interner::{FastMap, Interner, StrId};
use crate::layout::Ty;
use crate::names::{NAMES, Name, c};
use crate::parser::Entry;
use crate::recipe::{Op, Slot};
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
	fn begin(&mut self, ty: Name);
	fn object(&mut self);
	fn list(&mut self);
	fn end(&mut self);
	fn key(&mut self, key: Name);
	fn int(&mut self, value: u32);
	fn float(&mut self, value: f64);
	fn bool(&mut self, value: bool);
	fn null(&mut self);
	fn str(&mut self, value: Name);
	fn text(&mut self, value: &str);
	/// A string of the tree's interner.
	fn interned(&mut self, id: StrId, value: &str);
	/// A string equal to the source between two offsets.
	fn slice(&mut self, value: &str, start: u32, end: u32);
	fn span(&mut self, start: u32, end: u32);
	fn loc(&mut self, start_line: u32, start_column: u32, end_line: u32, end_column: u32);
	/// The key of a scope table: the tables are a root's last entries, and a sink may place them
	/// where a decoder finds them before the nodes that refer to them.
	fn table(&mut self, key: Name) {
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
	fn begin(&mut self, ty: Name) {
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
	fn key(&mut self, key: Name) {
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
	fn str(&mut self, value: Name) {
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
	fn table(&mut self, key: Name) {
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
	fn begin(&mut self, ty: Name) {
		self.open(false);
		self.out.push_str("\"type\":\"");
		self.out.push_str(ty.text);
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

	fn key(&mut self, key: Name) {
		self.sep();
		self.out.push('"');
		self.out.push_str(key.text);
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

	fn str(&mut self, value: Name) {
		self.text(value.text);
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
/// Strings outside `NAMES`, numbered after it in the order met.
struct Constants {
	names: Vec<&'static str>,
	ids: FastMap<&'static str, u32>,
	// a grammar's strings live for the process, so their address is a cheaper key than their text
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

	fn id(&mut self, name: Name) -> u32 {
		if name.id != u32::MAX {
			return name.id;
		}
		let address = name.text.as_ptr() as usize;
		let slot = ((address as u64).wrapping_mul(crate::interner::SEED) >> 55) as usize & 511;
		if self.recent[slot].0 == address {
			return self.recent[slot].1;
		}
		let id = match self.ids.get(name.text) {
			Some(&id) => id,
			None => {
				let id = (NAMES.len() + self.names.len()) as u32;
				self.names.push(name.text);
				self.ids.insert(name.text, id);
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
			// bcmp costs a call; a record is a handful of words
			let stored = &self.words[start..];
			if stored[0] as usize == record.len() && record.iter().zip(&stored[1..]).all(|(a, b)| a == b) {
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
/// The answer's words, read in place by a front end that takes the allocation over.
pub type Words = crate::handed::Handed<u32>;

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
	constants: Constants,
	shapes: Shapes,
}

const START: u32 = c!("start").id << 4 | kind::INT;
const END: u32 = c!("end").id << 4 | kind::INT;
const LOC: u32 = c!("loc").id << 4 | kind::LOC;

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
			words: Words::new(1 << 16),
			text: Vec::new(),
			units: 0,
			ends: Vec::new(),
			floats: Vec::new(),
			frames: Vec::new(),
			seq: Vec::new(),
			tables_at: 0,
			tables: 0,
			constants: Constants::new(),
			shapes: Shapes::new(),
		};
		binary.reset();
		binary
	}

	#[cfg(test)]
	pub(crate) fn constant(&mut self, name: Name) -> u32 {
		self.constants.id(name)
	}

	/// The constant strings numbered so far: `NAMES`, then the ones met outside it.
	pub fn constants(&self) -> Vec<&'static str> {
		NAMES.iter().chain(&self.constants.names).copied().collect()
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
			(NAMES.len() + self.constants.names.len()) as u32,
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
			self.push_text(interner.get(StrId::at(i as u32)));
		}
	}

	fn begin(&mut self, ty: Name) {
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

	fn key(&mut self, key: Name) {
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

	fn str(&mut self, value: Name) {
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
		self.words.push(id.index());
	}

	fn slice(&mut self, _value: &str, start: u32, end: u32) {
		self.value(kind::SLICE);
		self.words.extend_from_slice(&[start, end]);
	}

	fn span(&mut self, start: u32, end: u32) {
		self.seq.extend([START, END]);
		self.words.extend_from_slice(&[start, end]);
	}

	fn loc(&mut self, start_line: u32, start_column: u32, end_line: u32, end_column: u32) {
		self.seq.push(LOC);
		self.words
			.extend_from_slice(&[start_line, start_column, end_line, end_column]);
	}

	fn table(&mut self, _key: Name) {
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
		self.words.extend(strings.iter().map(|(id, _)| id.index()));
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
		w.list(c!("node"), roots);
	} else {
		w.field(c!("node"), ast.list(roots)[0].unwrap());
	}
	w.key(c!("end"));
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
	kept: Vec<(Name, NodeId)>,
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
	pub(crate) fn keep(&mut self, ty: Name, id: NodeId) {
		self.kept.push((ty, id));
	}

	/// What erasure left in place, as `typescript`, in source order.
	fn all_kept(&mut self) {
		self.key(c!("typescript"));
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

	pub(crate) fn begin(&mut self, ty: Name, id: NodeId) {
		let node = self.ast.node(id);
		self.sink.begin(ty);
		self.span(node.start, node.end);
		self.scope_facts(id);
		if self.ast.is_parenthesized(id) {
			self.bool(c!("parenthesized"), true);
		}
		self.ast.extension.extras(self, id);
		if let Some(&attached) = self.ast.attached.get(id) {
			self.comments(c!("leadingComments"), attached.leading);
			self.comments(c!("trailingComments"), attached.trailing);
			self.comments(c!("innerComments"), attached.inner);
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
			self.key(c!("scope"));
			self.sink.int(scope);
		}
		match scopes.of_identifier.get(id) {
			Some(Role::Declares(binding)) => {
				self.key(c!("declares"));
				self.sink.int(binding);
			}
			Some(Role::Reference(reference)) => {
				self.key(c!("reference"));
				self.sink.int(reference);
			}
			None => {}
		}
		let adopted = std::mem::take(&mut self.adopted);
		for node in adopted.iter().copied().chain([id]) {
			if let Some(&root) = scopes.root_of.get(node) {
				self.key(c!("root"));
				self.sink.int(root);
			}
			let bindings = scopes.declared_by.get(node);
			if !bindings.is_empty() {
				self.key(c!("defines"));
				self.sink.ints(bindings);
			}
			let writes = scopes.writes_of.get(node);
			if !writes.is_empty() {
				self.key(c!("writes"));
				self.sink.ints(writes);
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
		self.sink.table(c!("scopes"));
		self.sink.list();
		for scope in &scopes.scopes {
			self.sink.object();
			self.string(c!("kind"), scope.kind.name());
			self.key(c!("parent"));
			match scope.parent {
				Some(parent) => self.sink.int(parent),
				None => self.sink.null(),
			}
			self.bool(c!("topLevelAwait"), scope.top_level_await);
			self.sink.end();
		}
		self.sink.end();
		self.sink.table(c!("bindings"));
		self.sink.list();
		for binding in &scopes.bindings {
			self.sink.object();
			self.interned(c!("name"), binding.name);
			self.string(c!("kind"), binding.kind.name());
			self.key(c!("scope"));
			self.sink.int(binding.scope);
			self.bool(c!("write"), binding.write);
			self.sink.end();
		}
		self.sink.end();
		self.sink.table(c!("references"));
		self.sink.list();
		for reference in &scopes.references {
			self.sink.object();
			self.key(c!("scope"));
			self.sink.int(reference.scope);
			self.key(c!("binding"));
			match reference.binding {
				Some(binding) => self.sink.int(binding),
				None => self.sink.null(),
			}
			self.bool(c!("write"), reference.write);
			self.bool(c!("read"), reference.read);
			self.bool(c!("mutate"), reference.mutate);
			self.bool(c!("declares"), reference.declares);
			self.sink.end();
		}
		self.sink.end();
		if self.ast.hosts.is_empty() {
			return;
		}
		self.sink.table(c!("roots"));
		self.sink.list();
		for root in &scopes.roots {
			self.sink.object();
			self.key(c!("scope"));
			self.sink.int(root.scope);
			for (key, (from, to)) in [
				(c!("scopes"), root.scopes),
				(c!("bindings"), root.bindings),
				(c!("references"), root.references),
			] {
				self.key(key);
				self.sink.ints(&[from, to]);
			}
			self.sink.end();
		}
		self.sink.end();
	}

	fn comments(&mut self, key: Name, comments: crate::ast::Run) {
		if !comments.is_empty() {
			self.key(key);
			self.comment_list(comments.indices());
		}
	}

	fn comment_list(&mut self, comments: impl IntoIterator<Item = u32>) {
		self.sink.list();
		for index in comments {
			let comment = self.ast.comments[index as usize];
			self.sink
				.begin(if comment.is_block() { c!("Block") } else { c!("Line") });
			self.key(c!("value"));
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
		self.key(c!("errors"));
		self.sink.list();
		for error in &self.ast.errors {
			let pos = positions.offset(&mut cursor, error.pos);
			let (line, column) = positions.line_column(cursor.line, error.pos, pos);
			let end = positions.offset(&mut cursor, error.end);
			self.sink.object();
			self.key(c!("code"));
			self.sink.str(error.code.label());
			self.key(c!("message"));
			self.sink.text(&error.message);
			self.key(c!("pos"));
			self.sink.int(pos);
			self.key(c!("end"));
			self.sink.int(end);
			self.key(c!("loc"));
			self.sink.object();
			self.key(c!("line"));
			self.sink.int(line as u32);
			self.key(c!("column"));
			self.sink.int(column);
			self.sink.end();
			self.sink.end();
		}
		self.sink.end();
	}

	/// What the output's switches add after a root: every comment, what erasure kept, the scopes.
	fn trailers(&mut self) {
		if self.output.comments {
			self.key(c!("comments"));
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

	pub(crate) fn key(&mut self, key: Name) {
		self.sink.key(key);
	}

	pub(crate) fn field(&mut self, key: Name, id: NodeId) {
		self.key(key);
		self.node(id);
	}

	/// A specifier's other name, which is the same node as the binding one in `import { a }`
	/// and `export { a }`, and then only names: the binding facts stay on the binding one.
	fn other_name(&mut self, key: Name, id: NodeId, binding: NodeId) {
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
	pub(crate) fn opt_key(&mut self, key: Name, id: Option<NodeId>) {
		if let Some(id) = id {
			self.field(key, id);
		}
	}

	pub(crate) fn opt(&mut self, key: Name, id: Option<NodeId>) {
		self.key(key);
		match id {
			Some(id) => self.node(id),
			None => self.sink.null(),
		}
	}

	pub(crate) fn list(&mut self, key: Name, list: List) {
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
	pub(crate) fn params(&mut self, key: Name, list: List) {
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

	pub(crate) fn bool(&mut self, key: Name, value: bool) {
		self.key(key);
		self.sink.bool(value);
	}

	pub(crate) fn string(&mut self, key: Name, value: Name) {
		self.key(key);
		self.sink.str(value);
	}

	/// A string computed for this tree.
	pub(crate) fn text(&mut self, key: Name, value: &str) {
		self.key(key);
		self.sink.text(value);
	}

	pub(crate) fn interned(&mut self, key: Name, id: StrId) {
		self.key(key);
		self.sink.interned(id, self.ast.str(id));
	}

	pub(crate) fn raw(&mut self, id: NodeId) {
		let node = self.ast.node(id);
		self.key(c!("raw"));
		self.slice(node.start, node.end);
	}

	/// The recipes of the JavaScript kinds, resolved once.
	fn js_recipes() -> &'static [&'static [Op<Slot>]] {
		static RESOLVED: std::sync::OnceLock<&'static [&'static [Op<Slot>]]> = std::sync::OnceLock::new();
		RESOLVED.get_or_init(|| crate::recipe::resolve(crate::recipe::JS, crate::ast::node_layout::VARIANTS))
	}

	/// Follows a kind's recipe over the record at `base`, a `repr(C, u32)` enum's payload; the
	/// caller ends the node.
	pub(crate) fn run(&mut self, id: NodeId, ops: &[Op<Slot>], base: *const u8) {
		for op in ops {
			match *op {
				Op::Type(name) => self.begin(name, id),
				Op::TypeOf(slot) => {
					let Ty::Enum(names) = slot.ty else { unreachable!() };
					self.begin(names[get::<u8>(base, slot) as usize], id);
				}
				Op::Node(key, slot) => self.field(key, get(base, slot)),
				Op::Opt(key, slot) => match slot.ty {
					Ty::OptStr => {
						self.key(key);
						match get::<Option<StrId>>(base, slot) {
							Some(string) => self.sink.interned(string, self.ast.str(string)),
							None => self.sink.null(),
						}
					}
					_ => self.opt(key, get(base, slot)),
				},
				Op::OptKey(key, slot) => self.opt_key(key, get(base, slot)),
				Op::List(key, slot) => self.list(key, get(base, slot)),
				Op::OptListKey(key, slot) => {
					if let Some(list) = get::<Option<List>>(base, slot) {
						self.list(key, list);
					}
				}
				Op::Params(key, slot) => self.params(key, get(base, slot)),
				Op::Bool(key, slot) => self.bool(key, get(base, slot)),
				Op::BoolIf(key, slot) => {
					if get::<bool>(base, slot) {
						self.bool(key, true);
					}
				}
				Op::OptBoolKey(key, slot) => {
					if let Some(value) = get::<Option<bool>>(base, slot) {
						self.bool(key, value);
					}
				}
				Op::Str(key, slot) => self.interned(key, get(base, slot)),
				Op::OptStrKey(key, slot) => {
					if let Some(string) = get::<Option<StrId>>(base, slot) {
						self.interned(key, string);
					}
				}
				Op::Enum(key, slot) => {
					let Ty::Enum(names) = slot.ty else { unreachable!() };
					self.string(key, names[get::<u8>(base, slot) as usize]);
				}
				// a missing enum is a byte past the names, wherever the compiler put it
				Op::OptEnumKey(key, slot) => {
					let Ty::OptEnum(names) = slot.ty else { unreachable!() };
					if let Some(&name) = names.get(get::<u8>(base, slot) as usize) {
						self.string(key, name);
					}
				}
				Op::Modifier(key, slot) => {
					let Ty::OptEnum(names) = slot.ty else { unreachable!() };
					if let Some(&name) = names.get(get::<u8>(base, slot) as usize) {
						if name.text == "true" {
							self.bool(key, true);
						} else {
							self.string(key, name);
						}
					}
				}
				Op::BoolNames(key, slot, yes, no) => self.string(key, if get::<bool>(base, slot) { yes } else { no }),
				Op::Float(key, slot) => {
					let value = self.ast.numbers[get::<u32>(base, slot) as usize];
					self.key(key);
					if value.is_finite() {
						self.sink.float(value);
					} else {
						self.sink.null();
					}
				}
				Op::Raw => self.raw(id),
				Op::BigInt => {
					let node = self.ast.node(id);
					let raw = &self.source[node.start as usize..node.end as usize - 1];
					let bigint = bigint_decimal(raw);
					self.text(c!("bigint"), &bigint);
				}
				Op::Const(key, value) => self.string(key, value),
				Op::ConstBool(key, value) => self.bool(key, value),
				Op::Null(key) => {
					self.key(key);
					self.sink.null();
				}
				Op::EmptyList(key) => self.list(key, List::EMPTY),
				Op::Object(key, inner) => {
					self.key(key);
					self.sink.object();
					self.run(id, inner, base);
					self.sink.end();
				}
				Op::OtherName(key, name, binding) => self.other_name(key, get(base, name), get(base, binding)),
			}
		}
	}

	pub(crate) fn node(&mut self, id: NodeId) {
		use NodeKind::*;
		let kind = &self.ast.nodes[id.index() as usize].kind;
		match *kind {
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
					self.begin(Name::dynamic(host.ty), id);
				} else {
					self.sink.begin(Name::dynamic(host.ty));
					self.scope_facts(id);
				}
				let (from, len) = host.fields;
				for i in from..from + len {
					let (key, value) = self.ast.host_fields[i as usize];
					let key = Name::dynamic(key);
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
			_ => {
				let base = kind as *const NodeKind as *const u8;
				self.run(id, Self::js_recipes()[tag(base)], base);
			}
		}
		self.end();
	}
}

/// The tag of a `repr(C, u32)` enum at `base`: its first word.
pub(crate) fn tag(base: *const u8) -> usize {
	unsafe { base.cast::<u32>().read() as usize }
}

/// The field at `slot` of the record at `base`, as the type the recipe reads it by.
fn get<T: Copy>(base: *const u8, slot: Slot) -> T {
	unsafe { base.add(slot.at).cast::<T>().read_unaligned() }
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
	use crate::names::{Name, c};
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
		b.key(c!("node"));
		b.begin(c!("Identifier"));
		b.span(1, 2);
		b.key(c!("name"));
		b.interned(crate::interner::StrId::at(0), "a");
		b.key(c!("value"));
		b.float(1.5);
		b.key(c!("raw"));
		b.text("\u{1F600}b");
		b.key(Name::dynamic("list"));
		b.list();
		b.int(3);
		b.null();
		b.end();
		b.end();
		b.table(c!("scopes"));
		b.list();
		b.object();
		b.key(Name::dynamic("through"));
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
		assert_eq!(record(root), &[0, b.constant(c!("node")) << 4 | kind::NODE]);
		assert_eq!(
			record(node),
			&[
				b.constant(c!("Identifier")) + 1,
				b.constant(c!("start")) << 4 | kind::INT,
				b.constant(c!("end")) << 4 | kind::INT,
				b.constant(c!("name")) << 4 | kind::STR,
				b.constant(c!("value")) << 4 | kind::FLOAT,
				b.constant(c!("raw")) << 4 | kind::STR,
				b.constant(Name::dynamic("list")) << 4 | kind::NODES,
			]
		);
		assert_eq!(
			record(scope),
			&[0, b.constant(Name::dynamic("through")) << 4 | kind::INTS]
		);
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
			b.constant(Name::dynamic("Identifier")) + 1,
			b.constant(Name::dynamic("start")) << 4 | kind::INT,
			b.constant(Name::dynamic("end")) << 4 | kind::INT,
			b.constant(Name::dynamic("binding")) << 4 | kind::INT,
			b.constant(Name::dynamic("name")) << 4 | kind::STR,
		];
		let other = [
			b.constant(Name::dynamic("Literal")) + 1,
			b.constant(Name::dynamic("start")) << 4 | kind::INT,
			b.constant(Name::dynamic("end")) << 4 | kind::INT,
			b.constant(Name::dynamic("value")) << 4 | kind::STR,
			b.constant(Name::dynamic("raw")) << 4 | kind::SLICE,
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
			sink = sink.wrapping_add(b.constant(Name::dynamic(keys[(i & 7) as usize])));
		}
		eprintln!(
			"constant hit {:.1} ns  ({sink})",
			t.elapsed().as_nanos() as f64 / n as f64
		);
	}
}
