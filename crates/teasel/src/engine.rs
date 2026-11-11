//! What the Node addon and the WebAssembly module call.

use crate::Options;
use crate::ast::{Ast, List};
use crate::comments::attach;
use crate::handed::{Raw, Views};
use crate::host::{self, Grammar};
use crate::json::Json;
use crate::parser::{Entry, parse_at};
use crate::positions::{Cursor, Positions};
use crate::scopes;

/// The answer's words, read in place by the decoder that takes the allocation over.
pub type Words = crate::handed::Handed<u32>;

/// The tree's layout, the names of its views and the recipes, as JSON: what the decoder reading
/// the tree in place builds its readers from.
pub fn layout_json() -> String {
	use crate::ast::Extras;
	use crate::ast::Node;
	use crate::ast::node_layout::VARIANTS;
	use crate::layout::{Field, Ty};
	use crate::recipe::{Op, Rest, Slot};
	fn ty(ty: Ty) -> &'static str {
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
			Ty::Enum(_) => "enum",
			Ty::OptEnum(_) => "?enum",
			Ty::Struct(_) => "struct",
		}
	}
	fn field(w: &mut Json, field: &Field) {
		w.object();
		w.key("name");
		w.text(field.name);
		w.key("at");
		w.int(field.at as u32);
		w.key("ty");
		w.text(ty(field.ty));
		match field.ty {
			Ty::Enum(names) | Ty::OptEnum(names) => {
				w.key("names");
				w.list();
				names.iter().for_each(|name| w.text(name));
				w.end();
			}
			Ty::Struct(inner) => {
				w.key("fields");
				fields(w, inner);
			}
			_ => {}
		}
		w.end();
	}
	fn fields(w: &mut Json, list: &[Field]) {
		w.list();
		list.iter().for_each(|f| field(w, f));
		w.end();
	}
	fn record(w: &mut Json, size: usize, list: &[Field]) {
		w.object();
		w.key("size");
		w.int(size as u32);
		w.key("fields");
		fields(w, list);
		w.end();
	}
	// an operation is its name, its key, where its field sits, the field's type and an enum's
	// names, where its second field sits, what a constant spells or the name for true, the name
	// for false or for none, and the operations inside it
	fn ops(w: &mut Json, list: &[Op]) {
		w.list();
		for op in list {
			let (name, key, slot, rest) = op.told();
			w.list();
			w.text(name);
			w.text(key);
			w.int(slot.map_or(0, |slot| slot.at as u32));
			w.text(slot.map_or("", |slot| ty(*slot.ty)));
			w.list();
			if let Some(Slot {
				ty: Ty::Enum(names) | Ty::OptEnum(names),
				..
			}) = slot
			{
				names.iter().for_each(|name| w.text(name));
			}
			w.end();
			w.int(if let Rest::Second(slot) = rest {
				slot.at as u32
			} else {
				0
			});
			match rest {
				Rest::Bool(value) => w.bool(value),
				Rest::Names(yes, _) => w.text(yes),
				_ => w.null(),
			}
			w.text(if let Rest::Names(_, other) | Rest::Other(other) = rest {
				other
			} else {
				""
			});
			ops(w, if let Rest::Ops(inner) = rest { inner } else { &[] });
			w.end();
		}
		w.end();
	}
	let mut w = Json::default();
	w.object();
	w.key("node");
	w.object();
	w.key("size");
	w.int(size_of::<Node>() as u32);
	w.key("start");
	w.int(std::mem::offset_of!(Node, start) as u32);
	w.key("end");
	w.int(std::mem::offset_of!(Node, end) as u32);
	w.key("kind");
	w.int(std::mem::offset_of!(Node, kind) as u32);
	w.key("payload");
	w.int((std::mem::offset_of!(Node, kind) + crate::ast::node_layout::Host::FIELDS[0].at) as u32);
	w.end();
	w.key("kinds");
	texts(&mut w, VARIANTS.iter().map(|variant| variant.name));
	w.key("extras");
	record(&mut w, size_of::<Extras>(), Extras::FIELDS);
	w.key("records");
	w.object();
	for (name, size, list) in crate::ast::RECORDS {
		w.key(name);
		record(&mut w, *size, list);
	}
	w.end();
	let none = crate::layout::missing();
	w.key("none");
	w.object();
	w.key("enum");
	w.int(none.r#enum as u32);
	for (name, tagged) in [("list", none.list), ("str", none.str), ("int", none.int)] {
		w.key(name);
		w.object();
		w.key("tag");
		w.int(tagged.tag as u32);
		w.key("missing");
		w.int(tagged.missing);
		w.end();
	}
	w.end();
	w.key("views");
	w.object();
	w.key("js");
	texts(&mut w, view_names(false));
	w.key("ts");
	texts(&mut w, view_names(true));
	w.end();
	w.key("names");
	texts(&mut w, host::grammar::own::NAMES.iter().copied());
	w.key("stylesheet");
	children_table(&mut w, host::stylesheet_children());
	w.key("recipes");
	w.object();
	// by a kind's tag
	w.key("kinds");
	w.list();
	crate::recipe::KINDS.iter().for_each(|list| ops(&mut w, list));
	w.end();
	// each table's name, its record's size and its rows' recipe
	w.key("rows");
	w.list();
	for (name, size, list) in crate::recipe::ROWS {
		w.list();
		w.text(name);
		w.int(*size as u32);
		ops(&mut w, list);
		w.end();
	}
	w.end();
	w.key("adds");
	w.list();
	crate::recipe::ADDS.iter().for_each(|list| ops(&mut w, list));
	w.end();
	w.key("erased");
	ops(&mut w, crate::recipe::EXTRAS_ERASED);
	w.end();
	w.end();
	w.finish()
}

/// What the decoder learns of a grammar once, as JSON: every type of node its documents hold, each
/// with the fields that hold nodes (see `Grammar::children`), and its names by the numbers the
/// hosts' rows give them.
pub fn about(grammar: &Grammar) -> String {
	let mut w = Json::default();
	w.object();
	w.key("children");
	children_table(&mut w, grammar.children());
	w.key("names");
	texts(&mut w, grammar.names.iter().copied());
	w.end();
	w.finish()
}

fn children_table(w: &mut Json, table: Vec<(&'static str, Vec<&'static str>)>) {
	w.object();
	for (ty, fields) in table {
		w.key(ty);
		texts(w, fields);
	}
	w.end();
}

fn texts<'a>(w: &mut Json, list: impl IntoIterator<Item = &'a str>) {
	w.list();
	list.into_iter().for_each(|text| w.text(text));
	w.end();
}

/// One entry at a UTF-16 offset, as JavaScript counts; with a grammar, the whole source as a
/// document of its language. Its words at `words`; an offset the source has no place for is refused.
pub fn parse(
	source: &str,
	options: Options,
	entry: Entry,
	start: f64,
	stop: &str,
	grammar: Option<&Grammar>,
) -> Result<(), String> {
	let positions = Positions::of(source, options);
	let offset = positions.byte_offset(start)?;
	if !source.is_char_boundary(offset as usize) {
		return Err(format!("offset {start} is inside a surrogate pair"));
	}
	read(
		source,
		options,
		entry,
		offset,
		stop,
		grammar,
		|ast, roots, end, options, words| {
			let failed = roots.is_none();
			prepare(ast, source, &positions, options, failed);
			// `end`, the roots by number, a word of what the answer is, each view's length, then where
			// the tree's buffers sit folded into two words: the decoder keeps its views while that holds
			words.clear();
			let roots = roots.map_or(&[][..], |roots| ast.list(roots));
			words.extend_from_slice(&[positions.offset(&mut Cursor::default(), end), roots.len() as u32]);
			words.extend(roots.iter().map(|root| root.unwrap().index()));
			let typescript = if options.typescript() { TYPESCRIPT } else { 0 };
			words.push(typescript | if failed { FAILED } else { 0 });
			let mut sits = 0u64;
			let mut note = |_, buffer: Option<&mut dyn Raw>| {
				words.push(buffer.as_ref().map_or(0, |buffer| buffer.elements() as u32));
				if let Some(buffer) = buffer {
					sits = (sits ^ buffer.as_ptr() as u64 ^ (buffer.capacity_bytes() as u64) << 32)
						.wrapping_mul(0x9e37_79b9_7f4a_7c15);
				}
			};
			ast.views(options.typescript(), &mut Views(&mut note));
			words.extend_from_slice(&[sits as u32, (sits >> 32) as u32]);
		},
	);
	Ok(())
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
	/// Grammars by their JSON, read once each.
	static GRAMMARS: std::cell::RefCell<Vec<(&'static str, &'static Grammar)>> = const { std::cell::RefCell::new(Vec::new()) };
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
		session.trees[typescript as usize]
			.as_deref_mut()?
			.views(typescript, &mut Views(f));
		Some(typescript)
	})
}

/// The names of a tree's views in order.
fn view_names(typescript: bool) -> Vec<&'static str> {
	let mut names = Vec::new();
	Ast::default().views(typescript, &mut Views(&mut |name, _| names.push(name)));
	names
}

/// The number of the grammar its JSON describes, from 1: read once per thread and kept for good,
/// as its names are.
pub fn grammar(text: &str) -> Result<u32, String> {
	GRAMMARS.with(|grammars| {
		let mut grammars = grammars.borrow_mut();
		if let Some(at) = grammars.iter().position(|(known, _)| *known == text) {
			return Ok(at as u32 + 1);
		}
		let text = crate::host::grammar::keep(text);
		let grammar: &'static Grammar = Box::leak(Box::new(Grammar::read(text)?));
		grammars.push((text, grammar));
		Ok(grammars.len() as u32)
	})
}

/// The grammar of that number on this thread.
pub fn grammar_at(number: u32) -> Option<&'static Grammar> {
	GRAMMARS.with(|grammars| {
		let at = (number as usize).wrapping_sub(1);
		grammars.borrow().get(at).map(|&(_, grammar)| grammar)
	})
}

pub fn entry(index: f64) -> Result<Entry, String> {
	Entry::from_index(index as u32)
		.filter(|_| f64::from(index as u32) == index)
		.ok_or_else(|| format!("{index} is no entry"))
}

/// The answer's word, what the caller could not know: the parse failed, the tree is TypeScript's
/// (a document's language is the grammar's to tell).
const FAILED: u32 = 1;
const TYPESCRIPT: u32 = 2;

/// Parses with the session's tree of its language, in TypeScript when the grammar says so of a
/// script tag; `finish` gets the tree, its roots, `end`, the options as they ended up and the words.
pub(crate) fn read<R>(
	source: &str,
	options: Options,
	entry: Entry,
	offset: u32,
	stop: &str,
	grammar: Option<&Grammar>,
	finish: impl FnOnce(&mut Ast, Option<List>, u32, Options, &mut Words) -> R,
) -> R {
	let options = if grammar.is_some_and(|grammar| host::typescript(source, grammar)) {
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
		let (mut ast, parsed) = match grammar {
			Some(grammar) => {
				let (mut ast, root) = host::parse_document(source, grammar, options, reused);
				let parsed = root.map(|root| (ast.add_list(&[Some(root)]), source.len() as u32));
				(ast, parsed)
			}
			None => parse_at(source, offset, entry, options, stop, reused),
		};
		let (mut roots, mut end) = match parsed {
			Ok((roots, end)) => (Some(roots), end),
			Err(error) => (None, fail(&mut ast, *error)),
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
						(roots, end) = (None, fail(&mut ast, errors[0]));
					}
				}
			}
		}
		if options.has(Options::ERASE) && roots.is_some() {
			ast.mark_erased();
		} else {
			ast.erased.clear();
		}
		let answer = finish(&mut ast, roots, end, options, &mut session.words);
		*slot = Some(ast);
		answer
	})
}

/// What the decoder reading the tree in place needs beside it: every node's span and location
/// as JavaScript counts them, the nodes erasure leaves out, the comments, the errors and the
/// strings' UTF-16 starts; a failed parse prepares its error row alone.
fn prepare(ast: &mut Ast, source: &str, positions: &Positions, options: Options, failed: bool) {
	if !failed {
		prepare_tree(ast, positions, options);
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

fn prepare_tree(ast: &mut Ast, positions: &Positions, options: Options) {
	positions.map_nodes(&ast.nodes, &mut ast.spans, &mut ast.locs);
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
	positions.map_values(&mut ast.host_vals);
	if options.has(Options::COMMENTS) || !ast.host_rows.is_empty() {
		positions.map_comments(&ast.comments, &mut ast.comment_rows);
	}
}

/// A parse that failed answers as one that recovered from nothing but its error: no roots,
/// `end` at the error, the error alone in `errors`, and nothing else listed.
fn fail(ast: &mut Ast, error: crate::SyntaxError) -> u32 {
	ast.errors.clear();
	ast.errors.push(error);
	error.pos
}
