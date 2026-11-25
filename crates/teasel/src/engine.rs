//! What the Node addon and the WebAssembly module call.

use crate::Options;
use crate::ast::{Ast, List};
use crate::comments::attach;
use crate::handed::Raw;
use crate::layout::{Field, Ty};
use crate::parser::{Entry, parse_at};
use crate::positions::Positions;
use crate::recipe::{Op, Rest, Slot};
use crate::scopes;
use crate::svelte;

/// The answer's words, read in place by the decoder that takes the allocation over.
pub type Words = crate::handed::Handed<u32>;

/// The tree's layout, the names of its views and the recipes, as JSON: what the decoder reading
/// the tree in place builds its readers from.
pub fn layout_json() -> &'static str {
	LAYOUT
}

const LAYOUT: &str = match std::str::from_utf8(&LAYOUT_BYTES) {
	Ok(text) => text,
	Err(_) => panic!("the layout is UTF-8"),
};

const LAYOUT_BYTES: [u8; LAYOUT_LEN] = {
	let mut out = [0; LAYOUT_LEN];
	layout(&mut out);
	out
};

const LAYOUT_LEN: usize = layout(&mut []);

/// Writes the layout into `out` as far as it has room, and says how long it is.
const fn layout(out: &mut [u8]) -> usize {
	use crate::ast::node_layout::VARIANTS;
	use crate::ast::{Extras, Node, RECORDS, VIEWS};
	use crate::recipe::{ADDS, EXTRAS_ERASED, KINDS, ROWS};
	let mut w = Text {
		out,
		len: 0,
		first: true,
	};
	w.open(b'{');
	w.key("node");
	w.open(b'{');
	w.key("size");
	w.int(size_of::<Node>());
	w.key("start");
	w.int(std::mem::offset_of!(Node, start));
	w.key("end");
	w.int(std::mem::offset_of!(Node, end));
	w.key("kind");
	w.int(std::mem::offset_of!(Node, kind));
	w.close(b'}');
	w.key("kinds");
	w.open(b'[');
	let mut i = 0;
	while i < VARIANTS.len() {
		w.text(VARIANTS[i].name);
		i += 1;
	}
	w.close(b']');
	w.key("extras");
	w.record(size_of::<Extras>(), Extras::FIELDS);
	w.key("records");
	w.open(b'{');
	let mut i = 0;
	while i < RECORDS.len() {
		let (name, size, fields) = RECORDS[i];
		w.key(name);
		w.record(size, fields);
		i += 1;
	}
	w.close(b'}');
	let none = crate::layout::missing();
	w.key("none");
	w.open(b'{');
	w.key("enum");
	w.int(none.r#enum as usize);
	w.tagged("list", &none.list);
	w.tagged("str", &none.str);
	w.tagged("int", &none.int);
	w.close(b'}');
	w.key("views");
	w.open(b'{');
	w.key("js");
	w.texts(VIEWS.split_at(VIEWS.len() - 2).0);
	w.key("ts");
	w.texts(&VIEWS);
	w.close(b'}');
	w.key("recipes");
	w.open(b'{');
	w.key("kinds");
	w.open(b'[');
	let mut i = 0;
	while i < KINDS.len() {
		w.ops(KINDS[i]);
		i += 1;
	}
	w.close(b']');
	w.key("rows");
	w.open(b'[');
	let mut i = 0;
	while i < ROWS.len() {
		let (name, size, ops) = ROWS[i];
		w.open(b'[');
		w.text(name);
		w.int(size);
		w.ops(ops);
		w.close(b']');
		i += 1;
	}
	w.close(b']');
	w.key("adds");
	w.open(b'[');
	let mut i = 0;
	while i < ADDS.len() {
		w.ops(ADDS[i]);
		i += 1;
	}
	w.close(b']');
	w.key("erased");
	w.ops(EXTRAS_ERASED);
	w.close(b'}');
	w.close(b'}');
	w.len
}

/// JSON written into `out` as far as it has room, its whole length counted.
struct Text<'a> {
	out: &'a mut [u8],
	len: usize,
	/// Whether the next entry of the open container is its first.
	first: bool,
}

impl Text<'_> {
	const fn byte(&mut self, byte: u8) {
		if self.len < self.out.len() {
			self.out[self.len] = byte;
		}
		self.len += 1;
	}

	const fn sep(&mut self) {
		if !self.first {
			self.byte(b',');
		}
		self.first = false;
	}

	const fn open(&mut self, bracket: u8) {
		self.sep();
		self.byte(bracket);
		self.first = true;
	}

	const fn close(&mut self, bracket: u8) {
		self.byte(bracket);
		self.first = false;
	}

	const fn key(&mut self, key: &str) {
		self.text(key);
		self.byte(b':');
		self.first = true;
	}

	const fn word(&mut self, word: &str) {
		self.sep();
		let mut i = 0;
		while i < word.len() {
			self.byte(word.as_bytes()[i]);
			i += 1;
		}
	}

	const fn text(&mut self, text: &str) {
		self.sep();
		self.byte(b'"');
		let mut i = 0;
		while i < text.len() {
			let byte = text.as_bytes()[i];
			assert!(byte >= b' ' && byte != b'"' && byte != b'\\', "a name needs no escape");
			self.byte(byte);
			i += 1;
		}
		self.byte(b'"');
	}

	const fn texts(&mut self, texts: &[&str]) {
		self.open(b'[');
		let mut i = 0;
		while i < texts.len() {
			self.text(texts[i]);
			i += 1;
		}
		self.close(b']');
	}

	const fn int(&mut self, value: usize) {
		self.sep();
		let mut unit = 1;
		while value / unit >= 10 {
			unit *= 10;
		}
		while unit > 0 {
			self.byte(b'0' + (value / unit % 10) as u8);
			unit /= 10;
		}
	}

	const fn record(&mut self, size: usize, fields: &[Field]) {
		self.open(b'{');
		self.key("size");
		self.int(size);
		self.key("fields");
		self.fields(fields);
		self.close(b'}');
	}

	const fn fields(&mut self, fields: &[Field]) {
		self.open(b'[');
		let mut i = 0;
		while i < fields.len() {
			let field = &fields[i];
			self.open(b'{');
			self.key("name");
			self.text(field.name);
			self.key("at");
			self.int(field.at);
			self.key("ty");
			self.text(ty(field.ty));
			match field.ty {
				Ty::Enum(names) | Ty::OptEnum(names) => {
					self.key("names");
					self.texts(names);
				}
				Ty::Struct(inner) => {
					self.key("fields");
					self.fields(inner);
				}
				_ => {}
			}
			self.close(b'}');
			i += 1;
		}
		self.close(b']');
	}

	const fn tagged(&mut self, name: &str, tagged: &crate::layout::Tagged) {
		self.key(name);
		self.open(b'{');
		self.key("tag");
		self.int(tagged.tag);
		self.key("missing");
		self.int(tagged.missing as usize);
		self.close(b'}');
	}

	// an operation is its name, its key, where its field sits, the field's type and an enum's
	// names, where its second field sits, what a constant spells or the name for true, the name
	// for false or for none, and the operations inside it
	const fn ops(&mut self, ops: &[Op]) {
		self.open(b'[');
		let mut i = 0;
		while i < ops.len() {
			let (name, key, slot, rest) = ops[i].told();
			self.open(b'[');
			self.text(name);
			self.text(key);
			self.int(match (slot, &rest) {
				(Some(slot), _) => slot.at,
				(None, Rest::Cut(front, _)) => *front as usize,
				_ => 0,
			});
			match slot {
				Some(slot) => self.text(ty(*slot.ty)),
				None => self.text(""),
			}
			match slot {
				Some(Slot {
					ty: Ty::Enum(names) | Ty::OptEnum(names),
					..
				}) => self.texts(names),
				_ => self.texts(&[]),
			}
			self.int(match &rest {
				Rest::Second(slot) => slot.at,
				Rest::Cut(_, back) => *back as usize,
				Rest::Place(place) => *place as usize,
				_ => 0,
			});
			match &rest {
				Rest::Bool(true) => self.word("true"),
				Rest::Bool(false) => self.word("false"),
				Rest::Names(yes, _) => self.text(yes),
				_ => self.word("null"),
			}
			match &rest {
				Rest::Names(_, other) | Rest::Other(other) => self.text(other),
				_ => self.text(""),
			}
			match &rest {
				Rest::Ops(inner) => self.ops(inner),
				_ => self.ops(&[]),
			}
			self.close(b']');
			i += 1;
		}
		self.close(b']');
	}
}

const fn ty(ty: Ty) -> &'static str {
	match ty {
		Ty::Node => "node",
		Ty::OptNode => "?node",
		Ty::List => "list",
		Ty::OptList => "?list",
		Ty::Str => "str",
		Ty::OptStr => "?str",
		Ty::Bool => "bool",
		Ty::U32 => "u32",
		Ty::OptU32 => "?u32",
		Ty::Pair => "pair",
		Ty::Slice => "slice",
		Ty::OptSlice => "?slice",
		Ty::Enum(_) => "enum",
		Ty::OptEnum(_) => "?enum",
		Ty::Struct(_) => "struct",
	}
}

/// The whole source as `entry` reads it: a program, a stylesheet or a Svelte component. Its words
/// at `words`.
pub fn parse(source: &str, options: Options, entry: Entry) {
	let positions = Positions::new(source);
	read(source, options, entry, 0, |ast, roots, options, words| {
		let failed = roots.is_none();
		let markup = matches!(entry, Entry::Svelte | Entry::StyleSheet);
		prepare(ast, source, &positions, options, failed, markup);
		// the roots by number, the options the answer was read with, each view's length, then where
		// the tree's buffers sit folded into two words: the decoder keeps its views while that holds
		words.clear();
		let roots = roots.map_or(&[][..], |roots| ast.list(roots));
		words.push(roots.len() as u32);
		words.extend(roots.iter().map(|root| root.unwrap().index()));
		words.push((options.0 & !FAILED) | if failed { FAILED } else { 0 });
		let mut sits = 0u64;
		let mut note = |buffer: Option<&mut dyn Raw>| {
			words.push(buffer.as_ref().map_or(0, |buffer| buffer.elements() as u32));
			if let Some(buffer) = buffer {
				sits = (sits ^ buffer.as_ptr() as u64 ^ (buffer.capacity_bytes() as u64) << 32)
					.wrapping_mul(0x9e37_79b9_7f4a_7c15);
			}
		};
		ast.views(options.typescript(), &mut note);
		words.extend_from_slice(&[sits as u32, (sits >> 32) as u32]);
	});
}

/// What every parse on a thread reuses: the trees, emptied, and the answer's buffers.
#[derive(Default)]
struct Session {
	/// The JavaScript tree, then the TypeScript one: the decoder keeps the views of the two apart.
	trees: [Option<Box<Ast>>; 2],
	words: Words,
	/// Whether the last parse used the TypeScript tree.
	typescript: bool,
}

thread_local! {
	static SESSION: std::cell::RefCell<Session> = std::cell::RefCell::new(Session::default());
}

pub fn reset_session() {
	SESSION.with(|session| *session.borrow_mut() = Session::default());
}

/// The words of the last answer read in place on this thread, where they were written.
pub fn words<R>(f: impl FnOnce(&mut Words) -> R) -> R {
	SESSION.with(|session| f(&mut session.borrow_mut().words))
}

/// Visits the buffers of the last parse's tree on this thread, kept until the next parse takes
/// it, in the order `layout_json` names them; whether it is the TypeScript tree, None before any.
pub fn tree(f: &mut dyn FnMut(&'static str, Option<&mut dyn Raw>)) -> Option<bool> {
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		let typescript = session.typescript;
		let mut names = crate::ast::VIEWS.iter();
		session.trees[typescript as usize]
			.as_deref_mut()?
			.views(typescript, &mut |buffer| f(names.next().unwrap(), buffer));
		Some(typescript)
	})
}

/// The language of a number: 0 JavaScript, 1 CSS, 2 Svelte.
pub fn entry(index: f64) -> Result<Entry, String> {
	match index {
		0.0 => Ok(Entry::Program),
		1.0 => Ok(Entry::StyleSheet),
		2.0 => Ok(Entry::Svelte),
		_ => Err(format!("{index} is no language")),
	}
}

/// On the answer's word beside the options as they ended up, a component's script tags telling
/// whether it is TypeScript: the parse failed.
const FAILED: u32 = 1 << 31;

/// Parses with the session's tree of its language, in TypeScript when a component's script tag
/// says so; `finish` gets the tree, its roots, the options as they ended up and the words.
pub(crate) fn read<R>(
	source: &str,
	options: Options,
	entry: Entry,
	offset: u32,
	finish: impl FnOnce(&mut Ast, Option<List>, Options, &mut Words) -> R,
) -> R {
	let says = entry == Entry::Svelte && svelte::typescript(source);
	let options = if !options.typescript() && says {
		options.with(Options::TYPESCRIPT)
	} else {
		options
	};
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		session.typescript = options.typescript();
		let slot = &mut session.trees[options.typescript() as usize];
		let reused = slot.take().map(|mut ast| {
			ast.clear();
			ast
		});
		let (mut ast, parsed) = parse_at(source, offset, entry, options, "", reused);
		let mut roots = match parsed {
			Ok((roots, _)) => Some(roots),
			Err(error) => fail(&mut ast, *error),
		};
		if let Some(list) = roots {
			if options.has(Options::COMMENTS) {
				attach(&mut ast, source, list, offset);
			}
			if options.has(Options::SCOPES) {
				scopes::analyze(&mut ast, entry, list);
				let errors = std::mem::take(&mut ast.scopes.as_mut().unwrap().errors);
				if !errors.is_empty() {
					if options.has(Options::ERROR_RECOVERY) {
						ast.errors.extend(errors);
						ast.errors.sort_by_key(|error| error.pos);
					} else {
						roots = fail(&mut ast, errors[0]);
					}
				}
			}
		}
		if options.has(Options::ERASE) && roots.is_some() {
			ast.mark_erased();
		} else {
			ast.erased.clear();
		}
		let answer = finish(&mut ast, roots, options, &mut session.words);
		*slot = Some(ast);
		answer
	})
}

/// What the decoder reading the tree in place needs beside it: every node's span as JavaScript
/// counts it, the nodes erasure leaves out, the comments, the errors and the strings' UTF-16
/// starts; a failed parse prepares its error row alone.
fn prepare(ast: &mut Ast, source: &str, positions: &Positions, options: Options, failed: bool, markup: bool) {
	if !failed {
		prepare_tree(ast, positions, options, markup);
	}
	ast.error_rows.clear();
	if failed || options.has(Options::ERROR_RECOVERY) {
		let places = positions.of_errors(source, &ast.errors);
		for (error, [pos, end, line, column]) in ast.errors.iter().zip(places) {
			let [first, second] = error.args;
			ast.error_rows.push(crate::ast::ErrorRow {
				code: error.code as u32,
				first,
				second,
				pos,
				end,
				line,
				column,
			});
		}
	}
	ast.units.clear();
	let (text, starts, _) = ast.strings.buffers();
	if !failed && !text.is_ascii() {
		ast.units.reserve(starts.len());
		let mut units = 0u32;
		let mut from = 0usize;
		for &start in starts.iter() {
			units += text[from..start as usize]
				.iter()
				.filter(|&&b| b & 0xc0 != 0x80)
				.map(|&b| if b >= 0xf0 { 2 } else { 1 })
				.sum::<u32>();
			from = start as usize;
			ast.units.push(units);
		}
	}
}

fn prepare_tree(ast: &mut Ast, positions: &Positions, options: Options, markup: bool) {
	positions.map_nodes(&ast.nodes, &mut ast.spans);
	let (mut rare, mut late) = (std::mem::take(&mut ast.rare), std::mem::take(&mut ast.late));
	rare.reset(ast.nodes.len());
	late.reset(ast.nodes.len());
	rare.union(&ast.parenthesized);
	for &owner in ast.attached.owners() {
		rare.insert(owner);
	}
	if options.has(Options::ERASE) {
		ast.decorated(&mut rare);
	}
	if let Some(scopes) = &ast.scopes
		&& options.has(Options::SCOPES)
	{
		scopes.mark(ast, &mut rare, &mut late);
	}
	(ast.rare, ast.late) = (rare, late);
	positions.map_slices(&mut ast.slices);
	if options.has(Options::COMMENTS) || markup {
		positions.map_comments(&ast.comments, &mut ast.comment_rows);
	}
}

/// A parse that failed answers as one that recovered from nothing but its error: no roots, the
/// error alone in `errors`, and nothing else listed.
fn fail(ast: &mut Ast, error: crate::SyntaxError) -> Option<List> {
	ast.errors.clear();
	ast.errors.push(error);
	None
}
