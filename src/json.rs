//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error` the way acorn reports one.

use crate::error::Code;
use crate::ast::{Ast, NodeId};
use crate::comments::{attach, attach_all};
use crate::estree::{Binary, Emit, Json, Output, Positions, Sink, error_to_json, node_at, params_at, program};
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

#[derive(Clone, Copy, Debug, Default)]
pub struct Request {
	pub entry: Entry,
	/// Byte offset into the source; the JSON reports UTF-16 offsets like acorn.
	pub offset: u32,
	pub typescript: bool,
	pub comments: bool,
	/// Line and column on every node, as acorn's `locations` option.
	pub locations: bool,
	/// TypeScript erased on output; see `estree::Output`.
	pub erase: bool,
	/// Where a program ends, as a byte offset, for a program inside a larger source.
	pub end: Option<u32>,
	pub options: Options,
}

/// The names front ends accept for the request's switches, as acorn spells them.
pub const FLAGS: [&str; 11] = [
	"typescript",
	"comments",
	"locations",
	"script",
	"preserveParens",
	"allowReturnOutsideFunction",
	"allowAwaitOutsideFunction",
	"allowSuperOutsideMethod",
	"allowUndeclaredExports",
	"untilAs",
	"erase",
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

	/// Turns on one of `FLAGS`; anything else is ignored.
	pub fn set(&mut self, flag: &str) {
		match flag {
			"typescript" => self.typescript = true,
			"comments" => self.comments = true,
			"locations" => self.locations = true,
			"script" => self.options.module = false,
			"preserveParens" => self.options.preserve_parens = true,
			"allowReturnOutsideFunction" => self.options.allow_return_outside_function = true,
			"allowAwaitOutsideFunction" => self.options.allow_await_outside_function = true,
			"allowSuperOutsideMethod" => self.options.allow_super_outside_method = true,
			"allowUndeclaredExports" => self.options.allow_undeclared_exports = true,
			"untilAs" => self.options.until_as = true,
			"erase" => self.erase = true,
			_ => {}
		}
	}
}

/// The error answer for a request the parser never ran: a host's offsets or switches.
pub fn error_json(message: &str, pos: u32) -> String {
	let mut out = format!("{{\"error\":{{\"code\":\"{}\",\"message\":", Code::InvalidRequest.name());
	crate::estree::write_json_string(&mut out, message);
	out.push_str(&format!(",\"pos\":{pos},\"end\":{pos}}}}}"));
	out
}

pub fn parse(source: &str, request: &Request) -> String {
	parse_with(source, &Positions::new(source, request.locations), request)
}

/// The answer as a token stream for a binding to decode; the error answer stays JSON.
pub fn binary(source: &str, request: &Request) -> Result<Vec<u32>, String> {
	binary_with(source, &Positions::new(source, request.locations), request)
}

/// A source with its position tables and switches, for hosts that parse many pieces of one
/// source: offsets come in as UTF-16 the way acorn takes them.
pub struct Prepared {
	source: String,
	positions: Positions,
	request: Request,
}

impl Prepared {
	/// The request's entry and offset are ignored; `parse` takes them.
	pub fn new(source: String, request: Request) -> Prepared {
		let positions = Positions::new(&source, request.locations);
		Prepared {
			source,
			positions,
			request,
		}
	}

	/// The request for one entry at a UTF-16 offset; `until_as` says the host's `as` follows the
	/// expression, on top of the source's options.
	fn request(&self, entry: Entry, utf16_offset: f64, until_as: bool) -> Result<Request, String> {
		let mut request = Request {
			entry,
			offset: self.byte_offset(utf16_offset)?,
			..self.request
		};
		request.options.until_as |= until_as;
		Ok(request)
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
	pub fn parse(&self, entry: Entry, utf16_offset: f64, until_as: bool) -> String {
		match self.request(entry, utf16_offset, until_as) {
			Ok(request) => parse_with(&self.source, &self.positions, &request),
			Err(error) => error,
		}
	}

	/// The program that spans `start..end`, as JSON.
	pub fn parse_range(&self, start: f64, end: Option<f64>) -> String {
		match self.range(start, end) {
			Ok(request) => parse_with(&self.source, &self.positions, &request),
			Err(error) => error,
		}
	}

	/// One entry at an offset, as a token stream; the error answer stays JSON.
	pub fn binary(&self, entry: Entry, utf16_offset: f64, until_as: bool) -> Result<Vec<u32>, String> {
		binary_with(
			&self.source,
			&self.positions,
			&self.request(entry, utf16_offset, until_as)?,
		)
	}

	/// The program that spans `start..end`, as a token stream; the error answer stays JSON.
	pub fn binary_range(&self, start: f64, end: Option<f64>) -> Result<Vec<u32>, String> {
		binary_with(&self.source, &self.positions, &self.range(start, end)?)
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

fn dispatch<S: Sink>(source: &str, positions: &Positions, request: &Request, sink: S) -> Result<S, String> {
	check(source, request)?;
	#[cfg(feature = "typescript")]
	if request.typescript {
		return run::<crate::typescript::TypeScript, S>(source, positions, request, sink);
	}
	#[cfg(not(feature = "typescript"))]
	if request.typescript {
		return Err(error_json("built without TypeScript", 0));
	}
	run::<(), S>(source, positions, request, sink)
}

fn parse_with(source: &str, positions: &Positions, request: &Request) -> String {
	match dispatch(source, positions, request, Json::default()) {
		Ok(json) => json.finish(),
		Err(error) => error,
	}
}

/// The answer as a token stream, or the error answer as JSON.
fn binary_with(source: &str, positions: &Positions, request: &Request) -> Result<Vec<u32>, String> {
	dispatch(source, positions, request, Binary::new()).map(Binary::finish)
}

/// A tree, its root and the offset after what the parse consumed.
type Parsed<D> = Result<(Ast<D>, NodeId, u32), Box<SyntaxError>>;

/// Runs a request into a sink; `Err` is the error answer as JSON.
fn run<E: crate::parser::Extension, S: Sink>(
	source: &str,
	positions: &Positions,
	request: &Request,
	sink: S,
) -> Result<S, String>
where
	E::Data: Emit,
{
	let (offset, options, comments) = (request.offset, request.options, request.comments);
	let output = Output {
		comments,
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
				program(&ast, root, source, positions, output, sink)
			})
		}
		Entry::Expression => one(
			crate::parser::parse_expression_at::<E>(source, offset, options),
			source,
			positions,
			offset,
			output,
			sink,
		),
		Entry::Pattern => one(
			crate::parser::parse_pattern_at::<E>(source, offset, options),
			source,
			positions,
			offset,
			output,
			sink,
		),
		Entry::Statement => one(
			crate::parser::parse_statement_at::<E>(source, offset, options),
			source,
			positions,
			offset,
			output,
			sink,
		),
		Entry::Params => crate::parser::parse_params_at::<E>(source, offset, options).map(|(mut ast, ids, end)| {
			if comments {
				attach_all(&mut ast, source, &ids, offset);
			}
			params_at(&ast, &ids, end, source, positions, output, sink)
		}),
	};
	result.map_err(|error| error_to_json(&error, source))
}

/// One node parsed at an offset, into a sink.
fn one<X: Emit, S: Sink>(
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
		node_at(&ast, root, end, source, positions, output, sink)
	})
}
