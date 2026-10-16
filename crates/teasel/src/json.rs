//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, with `node` null and the one error listed when the parse failed; a request the
//! engine cannot take answers a JSON object with an `error`: its code, the reason as its argument,
//! and the offset.

use std::rc::Rc;

use crate::Options;
use crate::ast::{Ast, Reuse};
use crate::comments::attach;
use crate::error::Code;
use crate::estree::{Emit, Json, Positions, Words, answer};
use crate::handed::{Raw, Views};
use crate::host::{self, Grammar};
use crate::interner::StrId;
use crate::parser::{Entry, parse_at};
use crate::scopes::{self, Bind};

#[derive(Clone, Copy, Debug, Default)]
pub struct Request {
	pub entry: Entry,
	/// Byte offset into the source; the JSON reports UTF-16 offsets.
	pub offset: u32,
	/// Where the source is cut, as a byte offset, for a program inside a larger source.
	pub end: Option<u32>,
	pub options: Options,
}

impl Request {
	/// A source's request from its option word; the entry and offset come with each parse.
	pub fn from_flags(flags: u32) -> Request {
		Request {
			options: Options(flags),
			..Request::default()
		}
	}
}

/// The error answer for a request the parser never ran: a host's offsets or switches, what is
/// wrong with them as the one argument.
pub fn error_json(reason: &str, pos: u32) -> String {
	use std::fmt::Write;
	let mut out = format!("{{\"error\":{{\"code\":{},\"args\":[", Code::InvalidRequest as u8);
	crate::estree::write_json_string(&mut out, reason);
	write!(out, "],\"pos\":{pos},\"end\":{pos}}}}}").unwrap();
	out
}

/// The tree's layout, the names of its views and the recipes, as JSON: what a front end reading
/// the tree in place builds its readers from.
pub fn layout_json() -> String {
	use crate::ast::Node;
	use crate::layout::{Field, Ty, Variant};
	use crate::names::Name;
	use crate::recipe::{Op, Rest};
	let key = Name::dynamic;
	fn field(w: &mut Json, field: &Field) {
		w.object();
		w.key(Name::dynamic("name"));
		w.text(field.name);
		w.key(Name::dynamic("at"));
		w.int(field.at as u32);
		w.key(Name::dynamic("ty"));
		w.text(match field.ty {
			Ty::Node => "node",
			Ty::OptNode => "?node",
			Ty::List => "list",
			Ty::OptList => "?list",
			Ty::Str => "str",
			Ty::OptStr => "?str",
			Ty::Bool => "bool",
			Ty::OptBool => "?bool",
			Ty::U32 => "u32",
			Ty::OptU32 => "?u32",
			Ty::Pair => "pair",
			Ty::Enum(_) => "enum",
			Ty::OptEnum(_) => "?enum",
			Ty::Struct(_) => "struct",
		});
		match field.ty {
			Ty::Enum(names) | Ty::OptEnum(names) => {
				w.key(Name::dynamic("names"));
				w.list();
				names.iter().for_each(|name| w.str(*name));
				w.end();
			}
			Ty::Struct(inner) => {
				w.key(Name::dynamic("fields"));
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
		w.key(Name::dynamic("size"));
		w.int(size as u32);
		w.key(Name::dynamic("fields"));
		fields(w, list);
		w.end();
	}
	fn kinds(w: &mut Json, size: usize, variants: &[Variant]) {
		w.object();
		w.key(Name::dynamic("size"));
		w.int(size as u32);
		w.key(Name::dynamic("kinds"));
		w.list();
		for variant in variants {
			w.object();
			w.key(Name::dynamic("name"));
			w.text(variant.name);
			w.key(Name::dynamic("fields"));
			fields(w, variant.fields);
			w.end();
		}
		w.end();
		w.end();
	}
	// an operation is its name, then its key, its field and what else it takes
	fn ops(w: &mut Json, list: &[Op]) {
		w.list();
		for op in list {
			let (name, key, field, rest) = op.told();
			w.list();
			w.text(name);
			if let Some(key) = key {
				w.str(key);
			}
			if let Some(field) = field {
				w.text(field);
			}
			match rest {
				Rest::Nothing => {}
				Rest::Names(yes, no) => {
					w.str(yes);
					w.str(no);
				}
				Rest::Const(value) => w.str(value),
				Rest::Bool(value) => w.bool(value),
				Rest::Text(text) => w.text(text),
				Rest::Ops(inner) => ops(w, inner),
			}
			w.end();
		}
		w.end();
	}
	fn recipes(w: &mut Json, table: &[(&str, &[Op])]) {
		w.list();
		for (name, list) in table {
			w.list();
			w.text(name);
			ops(w, list);
			w.end();
		}
		w.end();
	}
	fn names(w: &mut Json, list: Vec<&str>) {
		w.list();
		list.into_iter().for_each(|name| w.text(name));
		w.end();
	}
	let mut w = Json::default();
	w.object();
	w.key(key("node"));
	w.object();
	w.key(key("size"));
	w.int(size_of::<Node>() as u32);
	w.key(key("start"));
	w.int(std::mem::offset_of!(Node, start) as u32);
	w.key(key("end"));
	w.int(std::mem::offset_of!(Node, end) as u32);
	w.key(key("kind"));
	w.int(std::mem::offset_of!(Node, kind) as u32);
	w.end();
	w.key(key("kinds"));
	w.list();
	for variant in crate::ast::node_layout::VARIANTS {
		w.object();
		w.key(key("name"));
		w.text(variant.name);
		w.key(key("fields"));
		fields(&mut w, variant.fields);
		w.end();
	}
	w.end();
	#[cfg(feature = "typescript")]
	{
		use crate::typescript::ast::{Extras, TsKind, ts_layout};
		w.key(key("ts"));
		kinds(&mut w, size_of::<TsKind>(), ts_layout::VARIANTS);
		w.key(key("extras"));
		record(&mut w, size_of::<Extras>(), Extras::FIELDS);
	}
	w.key(key("rows"));
	w.object();
	for (name, size, list) in crate::recipe::ROWS {
		w.key(key(name));
		record(&mut w, *size, list);
	}
	w.end();
	let none = crate::layout::missing();
	w.key(key("none"));
	w.object();
	w.key(key("enum"));
	w.int(none.r#enum as u32);
	w.key(key("bool"));
	w.int(none.bool as u32);
	for (name, tagged) in [("list", none.list), ("str", none.str), ("int", none.int)] {
		w.key(key(name));
		w.object();
		w.key(key("tag"));
		w.int(tagged.tag as u32);
		w.key(key("missing"));
		w.int(tagged.missing);
		w.end();
	}
	w.end();
	w.key(key("views"));
	w.object();
	w.key(key("js"));
	names(&mut w, view_names::<()>());
	#[cfg(feature = "typescript")]
	{
		w.key(key("ts"));
		names(&mut w, view_names::<crate::typescript::ast::Data>());
	}
	w.end();
	w.key(key("stylesheet"));
	children_table(&mut w, host::stylesheet_children());
	w.key(key("recipes"));
	w.object();
	w.key(key("js"));
	recipes(&mut w, crate::recipe::JS);
	w.key(key("rows"));
	recipes(&mut w, crate::recipe::RECIPES);
	#[cfg(feature = "typescript")]
	{
		w.key(key("ts"));
		recipes(&mut w, crate::recipe::TS);
		w.key(key("adds"));
		recipes(&mut w, crate::recipe::ADDS);
		w.key(key("extras"));
		recipes(
			&mut w,
			&[
				("extras", crate::recipe::EXTRAS),
				("erased", crate::recipe::EXTRAS_ERASED),
			],
		);
	}
	w.end();
	w.end();
	w.finish()
}

/// Every type of node a host's documents hold, each with the fields that hold nodes, as JSON; see
/// `Grammar::children`.
pub fn children(grammar: &Grammar) -> String {
	let mut w = Json::default();
	children_table(&mut w, grammar.children());
	w.finish()
}

fn children_table(w: &mut Json, table: Vec<(&'static str, Vec<&'static str>)>) {
	w.object();
	for (ty, fields) in table {
		w.key(crate::names::Name::dynamic(ty));
		w.list();
		fields.into_iter().for_each(|field| w.text(field));
		w.end();
	}
	w.end();
}

/// `stop` lists the host's tokens for an entry at an offset; see `parser::parse_at`.
pub fn parse(source: &str, request: &Request, stop: &str) -> String {
	parse_with(source, &Positions::of(source, request.options), request, stop, None)
}

/// A whole document of a host language by its grammar, as JSON; see `host::parse_document`.
pub fn parse_document(source: &str, grammar: &str, request: &Request) -> String {
	match self::grammar(grammar) {
		Ok(grammar) => {
			let mut request = *request;
			request.entry = Entry::Program;
			if host::typescript(source, &grammar) {
				request.options = request.options.with(Options::TYPESCRIPT);
			}
			parse_with(
				source,
				&Positions::of(source, request.options),
				&request,
				"",
				Some(&grammar),
			)
		}
		Err(message) => error_json(&message, 0),
	}
}

/// A source with its position tables and switches, for hosts that parse many pieces of one
/// source: offsets come in as UTF-16, as JavaScript counts them.
pub struct Prepared<'a> {
	source: std::borrow::Cow<'a, str>,
	positions: Positions,
	request: Request,
}

/// What every parse on a thread reuses: the trees, emptied, and the answer's buffers.
#[derive(Default)]
struct Session {
	pool: Pool,
	words: Words,
	/// Whether the last parse used the TypeScript tree.
	typescript: bool,
}

thread_local! {
	static SESSION: std::cell::RefCell<Session> = std::cell::RefCell::new(Session::default());
	/// Grammars by their JSON, read once each.
	static GRAMMARS: std::cell::RefCell<Vec<(&'static str, Rc<Grammar>)>> = const { std::cell::RefCell::new(Vec::new()) };
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
		#[cfg(feature = "typescript")]
		if session.typescript {
			session.pool.ts.as_deref_mut()?.views(&mut Views(f));
			session.pool.names[1].views(&mut Views(f));
			return Some(true);
		}
		session.pool.js.as_deref_mut()?.views(&mut Views(f));
		session.pool.names[0].views(&mut Views(f));
		Some(false)
	})
}

/// The names of a tree's views in order.
fn view_names<X: Reuse + Default>() -> Vec<&'static str> {
	let mut names = Vec::new();
	Ast::<X>::default().views(&mut Views(&mut |name, _| names.push(name)));
	Names::default().views(&mut Views(&mut |name, _| names.push(name)));
	names
}

/// The grammar its JSON describes, read once per thread.
pub fn grammar(text: &str) -> Result<Rc<Grammar>, String> {
	GRAMMARS.with(|grammars| {
		let mut grammars = grammars.borrow_mut();
		if let Some((_, grammar)) = grammars.iter().find(|(known, _)| *known == text) {
			return Ok(grammar.clone());
		}
		let text = crate::host::grammar::keep(text);
		let grammar = Rc::new(Grammar::read(text)?);
		grammars.push((text, grammar.clone()));
		Ok(grammar)
	})
}

#[derive(Default)]
pub struct Pool {
	/// The names of the hosts' types and keys, numbered once for every answer of the JavaScript
	/// tree and of the TypeScript one.
	names: [Names; 2],
	js: Option<Box<Ast<()>>>,
	#[cfg(feature = "typescript")]
	ts: Option<Box<Ast<crate::typescript::ast::Data>>>,
}

/// Names a front end knows by number across answers, and the shapes of host nodes.
#[derive(Default)]
struct Names {
	names: crate::interner::Interner,
	/// Each name by where it sits, with its number: a grammar's names are few places, met again
	/// and again, and the place hashes faster than the text.
	places: Vec<((usize, usize), u32)>,
	place_slots: crate::interner::Slots,
	/// Each shape back to back: a host node's type, whether it has a span, and each field's key
	/// and kind of value; nodes of one shape are built by one literal.
	shapes: Vec<u32>,
	shape_starts: Vec<u32>,
	shape_slots: crate::interner::Slots,
	shape: Vec<u32>,
}

impl Names {
	fn id(&mut self, name: &'static str) -> u32 {
		let place = (name.as_ptr() as usize, name.len());
		let hash = crate::interner::hash_words(&[place.0 as u32, (place.0 as u64 >> 32) as u32, place.1 as u32]);
		let places = &self.places;
		let slot = match self.place_slots.probe(hash, |i| places[i as usize].0 == place) {
			Ok(i) => return self.places[i as usize].1,
			Err(slot) => slot,
		};
		let id = self.names.intern(name).index();
		self.place_slots.insert(slot, hash, self.places.len() as u32);
		self.places.push((place, id));
		id
	}

	/// The number of the shape `self.shape` spells.
	fn shape_id(&mut self) -> u32 {
		let hash = crate::interner::hash_words(&self.shape);
		let (shapes, starts) = (&self.shapes, &self.shape_starts);
		let same = |id: u32| shapes[starts[id as usize] as usize..starts[id as usize + 1] as usize] == *self.shape;
		let slot = match self.shape_slots.probe(hash, same) {
			Ok(id) => return id,
			Err(slot) => slot,
		};
		if self.shape_starts.is_empty() {
			self.shape_starts.push(0);
		}
		let id = self.shape_starts.len() as u32 - 1;
		self.shapes.extend_from_slice(&self.shape);
		self.shape_starts.push(self.shapes.len() as u32);
		self.shape_slots.insert(slot, hash, id);
		id
	}

	fn views(&mut self, out: &mut Views<'_>) {
		let (text, starts, _) = self.names.buffers();
		out.push("names", text);
		out.push("name_starts", starts);
	}
}

/// Which slot of the pool an extension's tree takes.
pub trait Pooled: Sized {
	fn take(pool: &mut Pool) -> Option<Box<Ast<Self>>>;
	fn give(pool: &mut Pool, ast: Box<Ast<Self>>);
}

impl Pooled for () {
	fn take(pool: &mut Pool) -> Option<Box<Ast<Self>>> {
		pool.js.take()
	}
	fn give(pool: &mut Pool, ast: Box<Ast<Self>>) {
		pool.js = Some(ast);
	}
}

#[cfg(feature = "typescript")]
impl Pooled for crate::typescript::ast::Data {
	fn take(pool: &mut Pool) -> Option<Box<Ast<Self>>> {
		pool.ts.take()
	}
	fn give(pool: &mut Pool, ast: Box<Ast<Self>>) {
		pool.ts = Some(ast);
	}
}

impl Prepared<'static> {
	/// The request's entry and offset are ignored; `parse` takes them.
	pub fn new(source: String, request: Request) -> Prepared<'static> {
		Prepared::of(std::borrow::Cow::Owned(source), request)
	}

	/// The same over bytes a host hands over, made valid UTF-8 where they are not.
	pub fn from_bytes(source: Vec<u8>, request: Request) -> Prepared<'static> {
		let source = String::from_utf8(source).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
		Prepared::new(source, request)
	}
}

impl<'a> Prepared<'a> {
	/// The same over a source the caller keeps for the parses.
	pub fn borrowed(source: &'a str, request: Request) -> Prepared<'a> {
		Prepared::of(std::borrow::Cow::Borrowed(source), request)
	}

	fn of(source: std::borrow::Cow<'a, str>, request: Request) -> Prepared<'a> {
		let positions = Positions::of(&source, request.options);
		Prepared {
			source,
			positions,
			request,
		}
	}

	/// The request for one entry at a UTF-16 offset, the source cut at `end`, on top of the
	/// source's options; with a grammar, the whole source as a document of its language, in
	/// TypeScript when the grammar says so of a script tag.
	fn request(&self, entry: Entry, start: f64, end: Option<f64>, host: Option<&Grammar>) -> Result<Request, String> {
		let offset = self.byte_offset(start)?;
		let end = match end {
			Some(end) => Some(self.byte_offset(end)?),
			None => None,
		};
		Ok(Request {
			entry,
			offset,
			end,
			options: if host.is_some_and(|grammar| host::typescript(&self.source, grammar)) {
				self.request.options.with(Options::TYPESCRIPT)
			} else {
				self.request.options
			},
		})
	}

	/// One entry at an offset, as JSON.
	pub fn parse(&self, entry: Entry, start: f64, end: Option<f64>, stop: &str, host: Option<&Grammar>) -> String {
		match self.request(entry, start, end, host) {
			Ok(request) => parse_with(&self.source, &self.positions, &request, stop, host),
			Err(error) => error,
		}
	}

	/// One entry at an offset, read in place: its words at `words`; the error answer stays JSON.
	pub fn in_place(
		&self,
		entry: Entry,
		start: f64,
		end: Option<f64>,
		stop: &str,
		host: Option<&Grammar>,
	) -> Result<(), String> {
		in_place_with(
			&self.source,
			&self.positions,
			&self.request(entry, start, end, host)?,
			stop,
			host,
		)
	}

	/// A UTF-16 offset as a byte offset, or the error answer for it.
	fn byte_offset(&self, utf16: f64) -> Result<u32, String> {
		match self.positions.byte_offset(utf16) {
			Ok(offset) if self.source.is_char_boundary(offset as usize) => Ok(offset),
			Ok(_) => Err(error_json(&format!("offset {utf16} is inside a surrogate pair"), 0)),
			Err(message) => Err(error_json(&message, 0)),
		}
	}
}

/// The checks every entry makes on its offsets, or the error answer.
fn check(source: &str, request: &Request) -> Result<(), String> {
	if !source.is_char_boundary(request.offset as usize) {
		return Err(error_json(
			&format!("offset {} is not a character boundary", request.offset),
			request.offset,
		));
	}
	if let Some(end) = request.end {
		if !source.is_char_boundary(end as usize) {
			return Err(error_json(&format!("offset {end} is not a character boundary"), end));
		}
		if end < request.offset {
			return Err(error_json(&format!("offset {end} is before {}", request.offset), end));
		}
	}
	Ok(())
}

fn dispatch(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Grammar>,
	pool: &mut Pool,
	words: Option<&mut Words>,
) -> Result<String, String> {
	check(source, request)?;
	#[cfg(feature = "typescript")]
	if request.options.typescript() {
		return Ok(run::<crate::typescript::TypeScript>(
			source, positions, request, stop, host, pool, words,
		));
	}
	#[cfg(not(feature = "typescript"))]
	if request.options.typescript() {
		return Err(error_json("built without TypeScript", 0));
	}
	Ok(run::<()>(source, positions, request, stop, host, pool, words))
}

fn parse_with(source: &str, positions: &Positions, request: &Request, stop: &str, host: Option<&Grammar>) -> String {
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		session.typescript = request.options.typescript();
		match dispatch(source, positions, request, stop, host, &mut session.pool, None) {
			Ok(json) | Err(json) => json,
		}
	})
}

/// The answer's word, what the caller could not know: the parse failed, the tree is TypeScript's
/// (a document's language is the grammar's to tell).
const FAILED: u32 = 1;
const TYPESCRIPT: u32 = 2;

/// The answer read in place, its words at `words`, or the error answer as JSON.
fn in_place_with(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Grammar>,
) -> Result<(), String> {
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		session.typescript = request.options.typescript();
		dispatch(
			source,
			positions,
			request,
			stop,
			host,
			&mut session.pool,
			Some(&mut session.words),
		)?;
		Ok(())
	})
}

/// Runs a request: the answer as JSON, or read in place with its words in `words` and nothing
/// as text.
fn run<E: crate::parser::Extension>(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Grammar>,
	pool: &mut Pool,
	words: Option<&mut Words>,
) -> String
where
	E::Data: Emit + Bind + Reuse + Pooled,
{
	let options = request.options;
	let reused = Pooled::take(pool).map(|mut ast| {
		ast.clear();
		ast
	});
	let (mut ast, parsed) = match host {
		Some(grammar) => {
			let (mut ast, root) = host::parse_document::<E>(source, grammar, request.options, reused);
			let parsed = root.map(|root| (ast.add_list(&[Some(root)]), source.len() as u32));
			(ast, parsed)
		}
		None => parse_at::<E>(
			source,
			request.offset,
			request.end,
			request.entry,
			request.options,
			stop,
			reused,
		),
	};
	let (mut roots, mut end) = match parsed {
		Ok((roots, end)) => (Some(roots), end),
		Err(error) => (fail(&mut ast, *error), error.pos),
	};
	if let Some(list) = roots {
		if options.has(Options::COMMENTS) {
			attach(&mut ast, source, list, request.offset);
		}
		if options.has(Options::SCOPES) {
			scopes::analyze(&mut ast, request.entry, list);
			let errors = std::mem::take(&mut ast.scopes.as_mut().unwrap().errors);
			if !errors.is_empty() {
				if options.has(Options::ERROR_RECOVERY) {
					ast.errors.extend(errors);
					ast.errors.sort_by_key(|error| error.pos);
				} else {
					(roots, end) = (fail(&mut ast, errors[0]), errors[0].pos);
				}
			}
		}
	}
	let failed = roots.is_none();
	let Some(words) = words else {
		let json = answer(&ast, request.entry, roots, end, source, positions, options);
		Pooled::give(pool, ast);
		return json;
	};
	let names = &mut pool.names[request.options.typescript() as usize];
	prepare(&mut ast, source, positions, options, failed, names);
	// `end`, the roots by number, a word of what the answer is, each view's length, then where the
	// tree's buffers sit folded into two words: a front end keeps its views while that holds
	words.clear();
	let roots = roots.map_or(&[][..], |roots| ast.list(roots));
	words.extend_from_slice(&[
		positions.offset(&mut crate::estree::Cursor::default(), end),
		roots.len() as u32,
	]);
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
	ast.views(&mut Views(&mut note));
	pool.names[request.options.typescript() as usize].views(&mut Views(&mut note));
	words.extend_from_slice(&[sits as u32, (sits >> 32) as u32]);
	Pooled::give(pool, ast);
	String::new()
}

/// What a front end reading the tree in place needs beside it: every node's span and location
/// as JavaScript counts them, the nodes erasure leaves out, the hosts, the comments, the errors
/// as words and the strings' UTF-16 starts; a failed parse prepares its error row alone, the
/// tree it read is left as the last answer cleared it.
fn prepare<X: Emit + Reuse>(
	ast: &mut Ast<X>,
	source: &str,
	positions: &Positions,
	options: Options,
	failed: bool,
	names: &mut Names,
) {
	if !failed {
		prepare_tree(ast, positions, options, names);
	}
	ast.error_words.clear();
	if failed || options.has(Options::ERROR_RECOVERY) {
		let places = positions.of_errors(source, &ast.errors);
		for (error, [pos, end, line, column]) in ast.errors.iter().zip(places) {
			let [first, second] = error.args.map(|arg| arg.map_or(u32::MAX, StrId::index));
			ast.error_words
				.extend_from_slice(&[error.code as u32, first, second, pos, end, line, column]);
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

fn prepare_tree<X: Emit + Reuse>(ast: &mut Ast<X>, positions: &Positions, options: Options, names: &mut Names) {
	positions.map_nodes(&ast.nodes, &mut ast.spans, &mut ast.locs);
	ast.erased.clear();
	if options.has(Options::ERASE) {
		for i in 0..ast.nodes.len() as u32 {
			let id = crate::ast::NodeId::at(i);
			if ast.extension.erased(ast, id) {
				ast.erased.insert(id);
			}
		}
	}
	let (mut rare, mut late) = (std::mem::take(&mut ast.rare), std::mem::take(&mut ast.late));
	rare.reset(ast.nodes.len());
	late.reset(ast.nodes.len());
	rare.union(&ast.parenthesized);
	for &owner in ast.attached.owners() {
		rare.insert(owner);
	}
	ast.extension.rare(&mut rare);
	if let Some(scopes) = &ast.scopes
		&& options.has(Options::SCOPES)
	{
		scopes.mark(ast, &mut rare, &mut late);
	}
	(ast.rare, ast.late) = (rare, late);
	ast.host_view.clear();
	ast.host_keys.clear();
	ast.host_vals.clear();
	let mut cursor = crate::estree::Cursor::default();
	for i in 0..ast.host_fields.len() {
		let (key, value) = ast.host_fields[i];
		ast.host_keys.push(names.id(key));
		let mut words = value.words();
		if let crate::ast::Value::Slice(start, end) = value {
			words = [
				words[0],
				positions.offset(&mut cursor, start),
				positions.offset(&mut cursor, end),
			];
		}
		ast.host_vals.push(words);
	}
	for i in 0..ast.hosts.len() {
		let host = ast.hosts[i];
		let ty = if host.ty.is_empty() {
			u32::MAX
		} else {
			names.id(host.ty)
		};
		let (from, len) = (host.fields.0 as usize, host.fields.1 as usize);
		names.shape.clear();
		names.shape.extend([ty, host.span as u32]);
		for field in from..from + len {
			names.shape.extend([ast.host_keys[field], ast.host_vals[field][0]]);
		}
		let shape = names.shape_id();
		ast.host_view
			.extend_from_slice(&[ty, host.fields.0, host.fields.1, host.span as u32, shape]);
	}
	if options.has(Options::COMMENTS) || !ast.hosts.is_empty() {
		positions.map_comments(&ast.comments, &mut ast.comment_words);
	}
}

/// A parse that failed answers as one that recovered from nothing but its error: no roots,
/// `end` at the error, the error alone in `errors`, and nothing else listed.
fn fail<X>(ast: &mut Ast<X>, error: crate::SyntaxError) -> Option<crate::ast::List> {
	ast.errors.clear();
	ast.errors.push(error);
	None
}
