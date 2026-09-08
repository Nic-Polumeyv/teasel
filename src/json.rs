//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error`: its code, message, span and location.

use crate::ast::{Ast, NodeId};
use crate::comments::{attach, attach_all};
use crate::error::Code;
use crate::estree::{Binary, Emit, Json, Output, Positions, Sink, error_to_json, node_at, params_at, program};
use crate::scopes::{self, Bind};
use crate::{Options, SyntaxError};

/// What to parse; everything but a program starts at the request's offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Entry {
	#[default]
	Program,
	Expression,
	Pattern,
	Params,
	Statement,
}

impl Entry {
	pub fn from_index(index: u32) -> Entry {
		match index {
			1 => Entry::Expression,
			2 => Entry::Pattern,
			3 => Entry::Params,
			4 => Entry::Statement,
			_ => Entry::Program,
		}
	}
}

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
	/// Where a program ends, as a byte offset, for a program inside a larger source.
	pub end: Option<u32>,
	pub options: Options,
}

/// The names front ends accept for the request's switches, as acorn spells them.
pub const FLAGS: [&str; 12] = [
	"typescript",
	"comments",
	"scopes",
	"locations",
	"script",
	"preserveParens",
	"allowReturnOutsideFunction",
	"allowAwaitOutsideFunction",
	"allowSuperOutsideMethod",
	"allowUndeclaredExports",
	"erase",
	"errorRecovery",
];

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

	/// A source's request, its switches as bits in the order of `FLAGS`; the entry and offset
	/// come with each parse.
	pub fn from_bits(bits: u32) -> Request {
		let mut request = Request::new(Entry::Program, 0);
		request.set_bits(bits);
		request
	}

	pub fn set_bits(&mut self, bits: u32) {
		for (i, flag) in FLAGS.iter().enumerate() {
			if bits & (1 << i) != 0 {
				self.set(flag);
			}
		}
	}

	/// Turns on one of `FLAGS`; anything else is ignored.
	pub fn set(&mut self, flag: &str) {
		match flag {
			"typescript" => self.typescript = true,
			"comments" => self.comments = true,
			"scopes" => self.scopes = true,
			"locations" => self.locations = true,
			"script" => self.options.module = false,
			"preserveParens" => self.options.preserve_parens = true,
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

/// `stop` lists the host's tokens for a parse-at entry; see `parser::parse_expression_at`.
pub fn parse(source: &str, request: &Request, stop: &str) -> String {
	parse_with(source, &Positions::new(source, request.locations), request, stop)
}

/// A source with its position tables and switches, for hosts that parse many pieces of one
/// source: offsets come in as UTF-16 the way acorn takes them.
pub struct Prepared<'a> {
	source: std::borrow::Cow<'a, str>,
	positions: Positions,
	request: Request,
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

	/// The request for one entry at a UTF-16 offset, on top of the source's options.
	fn request(&self, entry: Entry, utf16_offset: f64) -> Result<Request, String> {
		Ok(Request {
			entry,
			offset: self.byte_offset(utf16_offset)?,
			..self.request
		})
	}

	/// The request for the program spanning `start..end`, both UTF-16 offsets; `end` defaults to
	/// the end of the source.
	fn range(&self, start: f64, end: Option<f64>) -> Result<Request, String> {
		let from = self.byte_offset(start)?;
		let to = match end {
			Some(end) => self.byte_offset(end)?,
			None => self.source.len() as u32,
		};
		if to < from {
			return Err(error_json(
				&format!("offset {} is before {start}", end.unwrap_or(0.0)),
				0,
			));
		}
		Ok(Request {
			entry: Entry::Program,
			offset: from,
			end: Some(to),
			..self.request
		})
	}

	/// One entry at an offset, as JSON.
	pub fn parse(&self, entry: Entry, utf16_offset: f64, stop: &str) -> String {
		match self.request(entry, utf16_offset) {
			Ok(request) => parse_with(&self.source, &self.positions, &request, stop),
			Err(error) => error,
		}
	}

	/// The program that spans `start..end`, as JSON.
	pub fn parse_range(&self, start: f64, end: Option<f64>) -> String {
		match self.range(start, end) {
			Ok(request) => parse_with(&self.source, &self.positions, &request, ""),
			Err(error) => error,
		}
	}

	/// One entry at an offset, as a token stream; the error answer stays JSON.
	pub fn binary(&self, entry: Entry, utf16_offset: f64, stop: &str) -> Result<Vec<u32>, String> {
		binary_with(&self.source, &self.positions, &self.request(entry, utf16_offset)?, stop)
	}

	/// The program that spans `start..end`, as a token stream; the error answer stays JSON.
	pub fn binary_range(&self, start: f64, end: Option<f64>) -> Result<Vec<u32>, String> {
		binary_with(&self.source, &self.positions, &self.range(start, end)?, "")
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

fn dispatch<S: Sink>(source: &str, positions: &Positions, request: &Request, stop: &str, sink: S) -> Result<S, String> {
	check(source, request)?;
	#[cfg(feature = "typescript")]
	if request.typescript {
		return run::<crate::typescript::TypeScript, S>(source, positions, request, stop, sink);
	}
	#[cfg(not(feature = "typescript"))]
	if request.typescript {
		return Err(error_json("built without TypeScript", 0));
	}
	run::<(), S>(source, positions, request, stop, sink)
}

fn parse_with(source: &str, positions: &Positions, request: &Request, stop: &str) -> String {
	match dispatch(source, positions, request, stop, Json::default()) {
		Ok(json) => json.finish(),
		Err(error) => error,
	}
}

/// The answer as a token stream, or the error answer as JSON.
fn binary_with(source: &str, positions: &Positions, request: &Request, stop: &str) -> Result<Vec<u32>, String> {
	dispatch(source, positions, request, stop, Binary::new()).map(|mut binary| binary.finish())
}

/// A tree, its root and the offset after what the parse consumed.
type Parsed<D> = Result<(Ast<D>, NodeId, u32), Box<SyntaxError>>;
type ParseAt<D> = fn(&str, u32, Options, &str) -> Parsed<D>;

/// Runs a request into a sink; `Err` is the error answer as JSON.
fn run<E: crate::parser::Extension, S: Sink>(
	source: &str,
	positions: &Positions,
	request: &Request,
	stop: &str,
	sink: S,
) -> Result<S, String>
where
	E::Data: Emit + Bind,
{
	let (offset, options, comments) = (request.offset, request.options, request.comments);
	let output = Output {
		comments,
		scopes: request.scopes,
		pattern: false,
		erase: request.erase && request.typescript,
	};
	let result = match request.entry {
		Entry::Program => {
			let end = request.end.unwrap_or(source.len() as u32);
			crate::parser::parse_range::<E>(source, offset, end, options).map(|mut ast| {
				let root = ast.last();
				if comments {
					attach(&mut ast, source, root, offset);
				}
				if output.scopes {
					scopes::analyze(&mut ast, root);
				}
				program(&ast, root, source, positions, output, sink)
			})
		}
		Entry::Expression | Entry::Pattern | Entry::Statement => {
			let (parse, output): (ParseAt<E::Data>, _) = match request.entry {
				Entry::Expression => (crate::parser::parse_expression_at::<E>, output),
				Entry::Statement => (crate::parser::parse_statement_at::<E>, output),
				Entry::Pattern => (
					crate::parser::parse_pattern_at::<E>,
					Output {
						pattern: true,
						..output
					},
				),
				_ => unreachable!(),
			};
			one(
				parse(source, offset, options, stop),
				source,
				positions,
				offset,
				output,
				sink,
			)
		}
		Entry::Params => {
			crate::parser::parse_params_at::<E>(source, offset, options, stop).map(|(mut ast, ids, end)| {
				if comments {
					attach_all(&mut ast, source, &ids, offset);
				}
				if output.scopes {
					scopes::analyze_params(&mut ast, &ids);
				}
				params_at(&ast, &ids, end, source, positions, output, sink)
			})
		}
	};
	result.map_err(|error| error_to_json(&error, source, positions))
}

/// One node parsed at an offset, into a sink.
fn one<X: Emit + Bind, S: Sink>(
	result: Parsed<X>,
	source: &str,
	positions: &Positions,
	offset: u32,
	output: Output,
	sink: S,
) -> Result<S, Box<SyntaxError>> {
	result.map(|(mut ast, root, end)| {
		if output.comments {
			attach(&mut ast, source, root, offset);
		}
		if output.scopes && output.pattern {
			scopes::analyze_pattern(&mut ast, root);
		} else if output.scopes {
			scopes::analyze(&mut ast, root);
		}
		node_at(&ast, root, end, source, positions, output, sink)
	})
}
