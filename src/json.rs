//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error`: its code, message, span and location.

use std::rc::Rc;

use crate::Options;
use crate::ast::{Ast, Reuse};
use crate::comments::attach;
use crate::error::Code;
use crate::estree::{Binary, Emit, Json, Output, Positions, Sink, answer, error_to_json};
use crate::host::{self, Grammar};
use crate::parser::{Entry, parse_at};
use crate::scopes::{self, Bind};

#[derive(Clone, Copy, Debug, Default)]
pub struct Request {
	pub entry: Entry,
	/// Byte offset into the source; the JSON reports UTF-16 offsets like acorn.
	pub offset: u32,
	pub typescript: bool,
	pub comments: bool,
	/// Scope analysis on the answer.
	pub scopes: bool,
	/// Line and column on every node, as acorn's `locations` option.
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

	/// A source's request from its switches named, separated by spaces, as acorn spells them and
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

pub fn constants_json() -> String {
	let mut json = String::from("[");
	for (i, name) in crate::estree::constants().iter().enumerate() {
		if i > 0 {
			json.push(',');
		}
		crate::estree::write_json_string(&mut json, name);
	}
	json.push(']');
	json
}

pub fn shapes_json() -> String {
	let words = crate::estree::shapes();
	let mut json = String::with_capacity(words.len() * 8 + 2);
	json.push('[');
	for (i, word) in words.iter().enumerate() {
		if i > 0 {
			json.push(',');
		}
		crate::estree::push_int(&mut json, *word);
	}
	json.push(']');
	json
}

/// `stop` lists the host's tokens for an entry at an offset; see `parser::parse_at`.
pub fn parse(source: &str, request: &Request, stop: &str) -> String {
	parse_with(source, &Positions::new(source, request.locations), request, stop, None)
}

/// A whole document of a host language by its grammar, as JSON; see `host::parse_document`.
pub fn parse_document(source: &str, grammar: &str, request: &Request) -> String {
	match grammar_named(grammar) {
		Ok(grammar) => {
			let mut request = *request;
			request.entry = Entry::Program;
			request.typescript |= host::typescript(source, &grammar);
			parse_with(
				source,
				&Positions::new(source, request.locations),
				&request,
				"",
				Some(&grammar),
			)
		}
		Err(message) => error_json(&message, 0),
	}
}

/// A source with its position tables and switches, for hosts that parse many pieces of one
/// source: offsets come in as UTF-16 the way acorn takes them.
pub struct Prepared<'a> {
	source: std::borrow::Cow<'a, str>,
	positions: Positions,
	request: Request,
	/// The grammar a program entry reads the whole source by.
	host: Option<Rc<Grammar>>,
}

thread_local! {
	/// The last parse's tree, emptied, for the next one: a host parsing every expression of
	/// every template allocates its arenas once per thread.
	static POOL: std::cell::RefCell<Pool> = std::cell::RefCell::new(Pool::default());
	/// Grammars by their text, read once each.
	static GRAMMARS: std::cell::RefCell<Vec<(String, Rc<Grammar>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The grammar of a text, read once per thread; the error names the line it stopped at.
fn grammar_named(text: &str) -> Result<Rc<Grammar>, String> {
	GRAMMARS.with(|grammars| {
		let mut grammars = grammars.borrow_mut();
		if let Some((_, grammar)) = grammars.iter().find(|(known, _)| known == text) {
			return Ok(grammar.clone());
		}
		let grammar = Rc::new(Grammar::read(text)?);
		grammars.push((text.to_string(), grammar.clone()));
		Ok(grammar)
	})
}

#[derive(Default)]
pub struct Pool {
	js: Option<Ast<()>>,
	#[cfg(feature = "typescript")]
	ts: Option<Ast<crate::typescript::ast::Data>>,
}

/// Which slot of the pool an extension's tree takes.
pub trait Pooled: Sized {
	fn take(pool: &mut Pool) -> Option<Ast<Self>>;
	fn give(pool: &mut Pool, ast: Ast<Self>);
}

impl Pooled for () {
	fn take(pool: &mut Pool) -> Option<Ast<Self>> {
		pool.js.take()
	}
	fn give(pool: &mut Pool, ast: Ast<Self>) {
		pool.js = Some(ast);
	}
}

#[cfg(feature = "typescript")]
impl Pooled for crate::typescript::ast::Data {
	fn take(pool: &mut Pool) -> Option<Ast<Self>> {
		pool.ts.take()
	}
	fn give(pool: &mut Pool, ast: Ast<Self>) {
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
			host: None,
		}
	}

	/// Reads the whole source as a document of the host language `grammar` describes when a
	/// program is asked for; the other entries read JavaScript at an offset as before. `Err` says
	/// where the grammar could not be read.
	pub fn host(mut self, grammar: &str) -> Result<Prepared<'a>, String> {
		let grammar = grammar_named(grammar)?;
		self.request.typescript |= host::typescript(&self.source, &grammar);
		self.host = Some(grammar);
		Ok(self)
	}

	/// The request for one entry at a UTF-16 offset, the source cut at `end`, on top of the
	/// source's options.
	fn request(&self, entry: Entry, start: f64, end: Option<f64>) -> Result<Request, String> {
		let offset = self.byte_offset(start)?;
		let end = match end {
			Some(end) => Some(self.byte_offset(end)?),
			None => None,
		};
		Ok(Request {
			entry,
			offset,
			end,
			..self.request
		})
	}

	/// One entry at an offset, as JSON.
	pub fn parse(&self, entry: Entry, start: f64, end: Option<f64>, stop: &str) -> String {
		match self.request(entry, start, end) {
			Ok(request) => parse_with(&self.source, &self.positions, &request, stop, self.grammar(entry)),
			Err(error) => error,
		}
	}

	/// One entry at an offset, as a token stream; the error answer stays JSON.
	pub fn binary(&self, entry: Entry, start: f64, end: Option<f64>, stop: &str) -> Result<Vec<u32>, String> {
		binary_with(
			&self.source,
			&self.positions,
			&self.request(entry, start, end)?,
			stop,
			self.grammar(entry),
		)
	}

	fn grammar(&self, entry: Entry) -> Option<&Grammar> {
		self.host.as_deref().filter(|_| entry == Entry::Program)
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

fn dispatch<S: Sink>(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Grammar>,
	sink: S,
) -> Result<S, String> {
	check(source, request)?;
	#[cfg(feature = "typescript")]
	if request.typescript {
		return run::<crate::typescript::TypeScript, S>(source, positions, request, stop, host, sink);
	}
	#[cfg(not(feature = "typescript"))]
	if request.typescript {
		return Err(error_json("built without TypeScript", 0));
	}
	run::<(), S>(source, positions, request, stop, host, sink)
}

fn parse_with(source: &str, positions: &Positions, request: &Request, stop: &str, host: Option<&Grammar>) -> String {
	match dispatch(source, positions, request, stop, host, Json::default()) {
		Ok(json) => json.finish(),
		Err(error) => error,
	}
}

/// The answer as a token stream, or the error answer as JSON.
fn binary_with(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Grammar>,
) -> Result<Vec<u32>, String> {
	dispatch(source, positions, request, stop, host, Binary::new()).map(|mut binary| binary.finish())
}

/// Runs a request into a sink; `Err` is the error answer as JSON.
fn run<E: crate::parser::Extension, S: Sink>(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	host: Option<&Grammar>,
	sink: S,
) -> Result<S, String>
where
	E::Data: Emit + Bind + Reuse + Pooled,
{
	let output = Output {
		comments: request.comments,
		scopes: request.scopes,
		erase: request.erase && request.typescript,
		errors: request.options.error_recovery,
	};
	let reused = POOL.with(|pool| Pooled::take(&mut pool.borrow_mut()));
	let parsed = match host {
		Some(grammar) => host::parse_document::<E>(source, grammar, request.options, reused).map(|(mut ast, root)| {
			let roots = ast.add_list(&[Some(root)]);
			(ast, roots, source.len() as u32)
		}),
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
	parsed
		.map(|(mut ast, roots, end)| {
			if output.comments {
				attach(&mut ast, source, roots, request.offset);
			}
			if output.scopes {
				scopes::analyze(&mut ast, request.entry, roots);
			}
			let sink = answer(&ast, request.entry, roots, end, source, positions, output, sink);
			ast.clear();
			POOL.with(|pool| Pooled::give(&mut pool.borrow_mut(), ast));
			sink
		})
		.map_err(|error| error_to_json(&error, source, positions))
}
