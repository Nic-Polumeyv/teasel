//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error`: its code, message, span and location.

use std::rc::Rc;

use crate::Options;
use crate::ast::{Ast, Reuse};
use crate::comments::attach;
use crate::error::Code;
use crate::estree::{Emit, Json, Output, Positions, Words, answer, error_to_json};
use crate::handed::{Raw, Views};
use crate::host::{self, Plan};
use crate::parser::{Decorators, Entry, parse_at};
use crate::scopes::{self, Bind};

#[derive(Clone, Copy, Debug, Default)]
pub struct Request {
	pub entry: Entry,
	/// Byte offset into the source; the JSON reports UTF-16 offsets.
	pub offset: u32,
	pub typescript: bool,
	pub comments: bool,
	/// Scope analysis on the answer.
	pub scopes: bool,
	/// Line and column on every node, as `loc`.
	pub locations: bool,
	/// TypeScript erased on output; see `estree::Output`.
	pub erase: bool,
	/// Where the source is cut, as a byte offset, for a program inside a larger source.
	pub end: Option<u32>,
	pub options: Options,
}

impl Request {
	pub fn new(entry: Entry, offset: u32) -> Request {
		Request {
			entry,
			offset,
			options: Options {
				module: true,
				..Options::default()
			},
			..Request::default()
		}
	}

	/// The same from one word of `flag` bits.
	pub fn from_flags(flags: u32) -> Request {
		let mut request = Request::default();
		for &(_, name) in flag::NAMES.iter().filter(|&&(bit, _)| flags & bit != 0) {
			request.set(name);
		}
		request
	}

	/// A source's request from its switches named, separated by spaces, as the package's options spell them, and
	/// `module` for `sourceType: 'module'`; a script otherwise. The entry and offset come with
	/// each parse.
	pub fn from_names(names: &str) -> Request {
		let mut request = Request::default();
		for name in names.split_ascii_whitespace() {
			request.set(name);
		}
		request
	}

	/// Turns on one switch by name; anything else is ignored.
	pub fn set(&mut self, flag: &str) {
		match flag {
			"typescript" => self.typescript = true,
			"comments" => self.comments = true,
			"scopes" => self.scopes = true,
			"locations" => self.locations = true,
			"module" => self.options.module = true,
			"parenthesized" => self.options.parenthesized = true,
			"legacyDecorators" => self.options.decorators = Decorators::Legacy,
			"proposalDecorators" => self.options.decorators = Decorators::Proposal,
			"allowReturnOutsideFunction" => self.options.allow_return_outside_function = true,
			"allowAwaitOutsideFunction" => self.options.allow_await_outside_function = true,
			"allowSuperOutsideMethod" => self.options.allow_super_outside_method = true,
			"allowUndeclaredExports" => self.options.allow_undeclared_exports = true,
			"erase" => self.erase = true,
			"errorRecovery" => self.options.error_recovery = true,
			_ => {}
		}
	}
}

/// A source's switches as bits, one word across a binding; package/api.js spells the same numbers.
pub mod flag {
	pub const MODULE: u32 = 1;
	pub const TYPESCRIPT: u32 = 1 << 1;
	pub const ERASE: u32 = 1 << 2;
	pub const COMMENTS: u32 = 1 << 3;
	pub const SCOPES: u32 = 1 << 4;
	pub const LOCATIONS: u32 = 1 << 5;
	pub const PARENTHESIZED: u32 = 1 << 6;
	pub const LEGACY_DECORATORS: u32 = 1 << 7;
	pub const PROPOSAL_DECORATORS: u32 = 1 << 8;
	pub const ALLOW_RETURN_OUTSIDE_FUNCTION: u32 = 1 << 9;
	pub const ALLOW_AWAIT_OUTSIDE_FUNCTION: u32 = 1 << 10;
	pub const ALLOW_SUPER_OUTSIDE_METHOD: u32 = 1 << 11;
	pub const ALLOW_UNDECLARED_EXPORTS: u32 = 1 << 12;
	pub const ERROR_RECOVERY: u32 = 1 << 13;
	/// Each bit by the name `Request::set` takes.
	pub const NAMES: [(u32, &str); 14] = [
		(MODULE, "module"),
		(TYPESCRIPT, "typescript"),
		(ERASE, "erase"),
		(COMMENTS, "comments"),
		(SCOPES, "scopes"),
		(LOCATIONS, "locations"),
		(PARENTHESIZED, "parenthesized"),
		(LEGACY_DECORATORS, "legacyDecorators"),
		(PROPOSAL_DECORATORS, "proposalDecorators"),
		(ALLOW_RETURN_OUTSIDE_FUNCTION, "allowReturnOutsideFunction"),
		(ALLOW_AWAIT_OUTSIDE_FUNCTION, "allowAwaitOutsideFunction"),
		(ALLOW_SUPER_OUTSIDE_METHOD, "allowSuperOutsideMethod"),
		(ALLOW_UNDECLARED_EXPORTS, "allowUndeclaredExports"),
		(ERROR_RECOVERY, "errorRecovery"),
	];
}

/// The error answer for a request the parser never ran: a host's offsets or switches.
pub fn error_json(message: &str, pos: u32) -> String {
	use std::fmt::Write;
	let mut out = format!(
		"{{\"error\":{{\"code\":\"{}\",\"message\":",
		Code::InvalidRequest.name()
	);
	crate::estree::write_json_string(&mut out, message);
	write!(out, ",\"pos\":{pos},\"end\":{pos}}}}}").unwrap();
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

/// `stop` lists the host's tokens for an entry at an offset; see `parser::parse_at`.
pub fn parse(source: &str, request: &Request, stop: &str) -> String {
	parse_with(source, &Positions::new(source, request.locations), request, stop, None)
}

/// A whole document of a host language by its plan, as JSON; see `host::parse_document`.
pub fn parse_document(source: &str, plan: &str, request: &Request) -> String {
	match self::plan(plan) {
		Ok(plan) => {
			let mut request = *request;
			request.entry = Entry::Program;
			request.typescript |= host::typescript(source, &plan);
			parse_with(
				source,
				&Positions::new(source, request.locations),
				&request,
				"",
				Some(&plan),
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
	/// Plans by their text, read once each.
	static PLANS: std::cell::RefCell<Vec<(String, Rc<Plan>)>> = const { std::cell::RefCell::new(Vec::new()) };
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

/// Every tree's buffers continue on fresh allocations: the views a front end held are its own.
pub fn renew_trees() {
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		let mut renew = |_, buffer: Option<&mut dyn Raw>| {
			if let Some(buffer) = buffer {
				// every tree is cleared before its next parse
				unsafe { buffer.renew() };
			}
		};
		#[cfg(feature = "typescript")]
		if let Some(ast) = session.pool.ts.as_deref_mut() {
			ast.views(&mut Views(&mut renew));
		}
		if let Some(ast) = session.pool.js.as_deref_mut() {
			ast.views(&mut Views(&mut renew));
		}
		session.pool.names.iter_mut().for_each(Names::renew);
	})
}

/// The names of a tree's views in order.
fn view_names<X: Reuse + Default>() -> Vec<&'static str> {
	let mut names = Vec::new();
	Ast::<X>::default().views(&mut Views(&mut |name, _| names.push(name)));
	Names::default().views(&mut Views(&mut |name, _| names.push(name)));
	names
}

/// The plan of a text, read once per thread; the error names the line it stopped at.
pub fn plan(text: &str) -> Result<Rc<Plan>, String> {
	PLANS.with(|plans| {
		let mut plans = plans.borrow_mut();
		if let Some((_, plan)) = plans.iter().find(|(known, _)| known == text) {
			return Ok(plan.clone());
		}
		let plan = Rc::new(Plan::read(text)?);
		plans.push((text.to_string(), plan.clone()));
		Ok(plan)
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

/// Names a front end knows by number across answers: their text, where each starts, and the
/// number of each.
#[derive(Default)]
struct Names {
	text: crate::handed::Handed<u8>,
	starts: crate::handed::Handed<u32>,
	ids: crate::interner::FastMap<&'static str, u32>,
	/// The same by where the name sits: a plan's names are few places, met again and again.
	places: crate::interner::FastMap<(usize, usize), u32>,
	/// A host node's shape by its type, whether it has a span, and each field's key and kind of
	/// value: nodes of one shape are built by one literal.
	shapes: crate::interner::FastMap<Vec<u32>, u32>,
	shape: Vec<u32>,
}

impl Names {
	fn id(&mut self, name: &'static str) -> u32 {
		let place = (name.as_ptr() as usize, name.len());
		if let Some(&id) = self.places.get(&place) {
			return id;
		}
		if let Some(&id) = self.ids.get(name) {
			self.places.insert(place, id);
			return id;
		}
		if self.starts.is_empty() {
			self.starts.push(0);
		}
		let id = self.ids.len() as u32;
		self.text.extend_from_slice(name.as_bytes());
		self.starts.push(self.text.len() as u32);
		self.ids.insert(name, id);
		self.places.insert(place, id);
		id
	}

	/// The number of the shape `self.shape` spells.
	fn shape_id(&mut self) -> u32 {
		if let Some(&id) = self.shapes.get(self.shape.as_slice()) {
			return id;
		}
		let id = self.shapes.len() as u32;
		self.shapes.insert(self.shape.clone(), id);
		id
	}

	fn views(&mut self, out: &mut Views<'_>) {
		out.push("names", &mut self.text);
		out.push("name_starts", &mut self.starts);
	}

	/// Starts over on fresh buffers: a front end that held the views keeps what it saw.
	fn renew(&mut self) {
		self.ids.clear();
		self.places.clear();
		self.shapes.clear();
		self.text.renew(0);
		self.starts.renew(0);
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
		let positions = Positions::new(&source, request.locations);
		Prepared {
			source,
			positions,
			request,
		}
	}

	/// The request for one entry at a UTF-16 offset, the source cut at `end`, on top of the
	/// source's options; with a plan, the whole source as a document of its language, in
	/// TypeScript when the plan says so of a script tag.
	fn request(&self, entry: Entry, start: f64, end: Option<f64>, host: Option<&Plan>) -> Result<Request, String> {
		let offset = self.byte_offset(start)?;
		let end = match end {
			Some(end) => Some(self.byte_offset(end)?),
			None => None,
		};
		Ok(Request {
			entry,
			offset,
			end,
			typescript: self.request.typescript || host.is_some_and(|plan| host::typescript(&self.source, plan)),
			..self.request
		})
	}

	/// One entry at an offset, as JSON.
	pub fn parse(&self, entry: Entry, start: f64, end: Option<f64>, stop: &str, host: Option<&Plan>) -> String {
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
		host: Option<&Plan>,
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
	host: Option<&Plan>,
	pool: &mut Pool,
	words: Option<&mut Words>,
) -> Result<String, String> {
	check(source, request)?;
	#[cfg(feature = "typescript")]
	if request.typescript {
		return run::<crate::typescript::TypeScript>(source, positions, request, stop, host, pool, words);
	}
	#[cfg(not(feature = "typescript"))]
	if request.typescript {
		return Err(error_json("built without TypeScript", 0));
	}
	run::<()>(source, positions, request, stop, host, pool, words)
}

fn parse_with(source: &str, positions: &Positions, request: &Request, stop: &str, host: Option<&Plan>) -> String {
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		session.typescript = request.typescript;
		match dispatch(source, positions, request, stop, host, &mut session.pool, None) {
			Ok(json) | Err(json) => json,
		}
	})
}

/// The answer read in place, its words at `words`, or the error answer as JSON.
fn in_place_with(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Plan>,
) -> Result<(), String> {
	SESSION.with(|session| {
		let session = &mut *session.borrow_mut();
		session.typescript = request.typescript;
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
/// as text; `Err` is the error answer as JSON.
fn run<E: crate::parser::Extension>(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Plan>,
	pool: &mut Pool,
	words: Option<&mut Words>,
) -> Result<String, String>
where
	E::Data: Emit + Bind + Reuse + Pooled,
{
	let output = Output {
		comments: request.comments,
		scopes: request.scopes,
		erase: request.erase && request.typescript,
		errors: request.options.error_recovery,
	};
	let reused = Pooled::take(pool).map(|mut ast| {
		ast.clear();
		ast
	});
	let (mut ast, parsed) = match host {
		Some(plan) => {
			let (mut ast, root) = host::parse_document::<E>(source, plan, request.options, reused, request.scopes);
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
	let (roots, end) = match parsed {
		Ok(parsed) => parsed,
		Err(error) => return Err(recycle(pool, ast, &error, source, positions)),
	};
	if output.comments {
		attach(&mut ast, source, roots, request.offset);
	}
	if output.scopes {
		scopes::analyze(&mut ast, request.entry, roots);
		let errors = std::mem::take(&mut ast.scopes.as_mut().unwrap().errors);
		if !errors.is_empty() {
			if !output.errors {
				let error = errors.into_iter().next().unwrap();
				return Err(recycle(pool, ast, &error, source, positions));
			}
			ast.errors.extend(errors);
			ast.errors.sort_by_key(|error| error.pos);
		}
	}
	let Some(words) = words else {
		let json = answer(&ast, request.entry, roots, end, source, positions, output);
		Pooled::give(pool, ast);
		return Ok(json);
	};
	let names = &mut pool.names[request.typescript as usize];
	prepare(&mut ast, source, positions, output, names);
	// `end`, the roots by number, a word of what the answer is, each view's length, then where the
	// tree's buffers sit folded into two words: a front end keeps its views while that holds. The
	// tree is TypeScript's, every comment is listed, TypeScript is erased, lines are on, the roots
	// are a list, the errors recovered from are listed
	words.clear();
	words.extend_from_slice(&[positions.offset(&mut crate::estree::Cursor::default(), end), roots.len]);
	words.extend(ast.list(roots).iter().map(|root| root.unwrap().index()));
	words.push(
		(request.typescript as u32) << 1
			| (output.comments as u32) << 2
			| (output.erase as u32) << 3
			| (request.locations as u32) << 4
			| ((request.entry == Entry::Params) as u32) << 5
			| (output.errors as u32) << 6,
	);
	let mut sits = 0u64;
	let mut note = |_, buffer: Option<&mut dyn Raw>| {
		words.push(buffer.as_ref().map_or(0, |buffer| buffer.elements() as u32));
		if let Some(buffer) = buffer {
			sits = (sits ^ buffer.as_ptr() as u64 ^ (buffer.capacity_bytes() as u64) << 32)
				.wrapping_mul(0x9e37_79b9_7f4a_7c15);
		}
	};
	ast.views(&mut Views(&mut note));
	pool.names[request.typescript as usize].views(&mut Views(&mut note));
	words.extend_from_slice(&[sits as u32, (sits >> 32) as u32]);
	Pooled::give(pool, ast);
	Ok(String::new())
}

/// What a front end reading the tree in place needs beside it: every node's span and location
/// as JavaScript counts them, the nodes erasure leaves out, the hosts, the comments and the
/// recovered errors as words, their names and messages among the strings, and the strings'
/// UTF-16 starts.
fn prepare<X: Emit + Reuse>(ast: &mut Ast<X>, source: &str, positions: &Positions, output: Output, names: &mut Names) {
	positions.map_nodes(&ast.nodes, &mut ast.spans, &mut ast.locs);
	ast.erased.clear();
	if output.erase {
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
		&& output.scopes
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
	if output.comments || !ast.hosts.is_empty() {
		positions.map_comments(&ast.comments, &mut ast.comment_words);
	}
	ast.error_words.clear();
	if output.errors {
		let places = positions.of_errors(source, &ast.errors);
		for (i, [pos, end, line, column]) in places.into_iter().enumerate() {
			let code = ast.strings.intern(ast.errors[i].code.label().text).index();
			let message = ast.strings.intern(&ast.errors[i].message).index();
			ast.error_words
				.extend_from_slice(&[code, message, pos, end, line, column]);
		}
	}
	ast.units.clear();
	let (text, starts, _) = ast.strings.buffers();
	if !text.is_ascii() {
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

/// The tree of a failed request goes back to the pool; the error is the answer.
fn recycle<X: Reuse + Pooled>(
	pool: &mut Pool,
	ast: Box<Ast<X>>,
	error: &crate::SyntaxError,
	source: &str,
	positions: &Positions,
) -> String {
	Pooled::give(pool, ast);
	error_to_json(error, source, positions)
}
