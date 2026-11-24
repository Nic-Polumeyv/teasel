//! What the Node addon and the WebAssembly module call.

use crate::Options;
use crate::ast::{Ast, List};
use crate::comments::attach;
use crate::handed::Raw;
use crate::json::Json;
use crate::parser::{Entry, parse_at};
use crate::positions::Positions;
use crate::scopes;
use crate::svelte;

/// The answer's words, read in place by the decoder that takes the allocation over.
pub type Words = crate::handed::Handed<u32>;

/// The tree's layout, the names of its views and the recipes, as JSON: what the decoder reading
/// the tree in place builds its readers from. The package carries it as `npm/src/layout.ts`,
/// which `tests/fixtures.rs` writes.
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
			Ty::Slice => "slice",
			Ty::OptSlice => "?slice",
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
			w.int(match (slot, &rest) {
				(Some(slot), _) => slot.at as u32,
				(None, Rest::Cut(front, _)) => *front as u32,
				_ => 0,
			});
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
			w.int(match rest {
				Rest::Second(slot) => slot.at as u32,
				Rest::Cut(_, back) => back as u32,
				Rest::Place(place) => place as u32,
				_ => 0,
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
	let mut w = Json::wrapped(3);
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
	w.key("recipes");
	w.object();
	w.key("kinds");
	w.list();
	crate::recipe::KINDS.iter().for_each(|list| ops(&mut w, list));
	w.end();
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

/// The stamp of a layout file: the number on its line `export const stamp = `, which a build
/// carries so that the package refuses a build whose layout is not its own.
pub const fn stamp(file: &[u8]) -> u32 {
	const LINE: &[u8] = b"\nexport const stamp = ";
	let mut at = 0;
	let mut matched = 0;
	while matched < LINE.len() {
		assert!(at < file.len(), "a layout file names its stamp");
		matched = if file[at] == LINE[matched] { matched + 1 } else { (file[at] == LINE[0]) as usize };
		at += 1;
	}
	let mut stamp = 0u32;
	while file[at].is_ascii_digit() {
		stamp = stamp * 10 + (file[at] - b'0') as u32;
		at += 1;
	}
	stamp
}

fn texts<'a>(w: &mut Json, list: impl IntoIterator<Item = &'a str>) {
	w.list();
	list.into_iter().for_each(|text| w.text(text));
	w.end();
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
		let mut note = |_, buffer: Option<&mut dyn Raw>| {
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
pub fn tree(mut f: &mut dyn FnMut(&'static str, Option<&mut dyn Raw>)) -> Option<bool> {
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		let typescript = session.typescript;
		session.trees[typescript as usize]
			.as_deref_mut()?
			.views(typescript, &mut f);
		Some(typescript)
	})
}

/// The names of a tree's views in order.
fn view_names(typescript: bool) -> Vec<&'static str> {
	let mut names = Vec::new();
	Ast::default().views(typescript, &mut |name, _| names.push(name));
	names
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
