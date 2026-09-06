//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error` the way acorn reports one.

use crate::ast::{Ast, NodeId};
use crate::comments::{attach, attach_all};
use crate::estree::{Emit, Output, Positions, error_to_json, node_to_json, params_to_json, program_to_json};
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

pub fn error_json(message: &str, pos: u32) -> String {
	let mut out = String::from("{\"error\":{\"message\":");
	crate::estree::write_json_string(&mut out, message);
	out.push_str(&format!(",\"pos\":{pos}}}}}"));
	out
}

pub fn parse(source: &str, request: &Request) -> String {
	parse_with(source, &Positions::new(source, request.locations), request)
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

	/// `until_as` says the host's `as` follows this expression, on top of the source's options.
	pub fn parse(&self, entry: Entry, utf16_offset: f64, until_as: bool) -> String {
		let offset = match self.byte_offset(utf16_offset) {
			Ok(offset) => offset,
			Err(json) => return json,
		};
		let mut request = Request {
			entry,
			offset,
			..self.request
		};
		request.options.until_as |= until_as;
		parse_with(&self.source, &self.positions, &request)
	}
}

impl Prepared {
	/// The program that spans `start..end` of the source, both UTF-16 offsets.
	pub fn parse_range(&self, start: f64, end: f64) -> String {
		let (start, end) = match (self.byte_offset(start), self.byte_offset(end)) {
			(Ok(start), Ok(end)) if start <= end => (start, end),
			(Ok(_), Ok(_)) => return error_json(&format!("offset {end} is before {start}"), 0),
			(Err(json), _) | (_, Err(json)) => return json,
		};
		let request = Request {
			entry: Entry::Program,
			offset: start,
			end: Some(end),
			..self.request
		};
		parse_with(&self.source, &self.positions, &request)
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

fn parse_with(source: &str, positions: &Positions, request: &Request) -> String {
	if !source.is_char_boundary(request.offset as usize) {
		return error_json(
			&format!("offset {} is not a character boundary", request.offset),
			request.offset,
		);
	}
	#[cfg(feature = "typescript")]
	if request.typescript {
		return run::<crate::typescript::TypeScript>(source, positions, request);
	}
	#[cfg(not(feature = "typescript"))]
	if request.typescript {
		return error_json("built without TypeScript", 0);
	}
	run::<()>(source, positions, request)
}

/// A tree, its root and the offset after what the parse consumed.
type Parsed<D> = Result<(Ast<D>, NodeId, u32), Box<SyntaxError>>;

fn run<E: crate::parser::Extension>(source: &str, positions: &Positions, request: &Request) -> String
where
	E::Data: Emit,
{
	let (offset, options, comments) = (request.offset, request.options, request.comments);
	let output = Output {
		locations: request.locations,
		comments,
		erase: request.erase && request.typescript,
	};
	let one = |result: Parsed<E::Data>| {
		result.map(|(mut ast, root, end)| {
			if comments {
				attach(&mut ast, source, root, offset);
			}
			node_to_json(&ast, root, end, source, positions, output)
		})
	};
	let result = match request.entry {
		Entry::Program => {
			let end = request.end.unwrap_or(source.len() as u32);
			crate::parser::parse_range::<E>(source, offset, end, options).map(|mut ast| {
				let root = ast.last();
				if comments {
					attach(&mut ast, source, root, offset);
				}
				program_to_json(&ast, root, source, positions, output)
			})
		}
		Entry::Expression => one(crate::parser::parse_expression_at::<E>(source, offset, options)),
		Entry::Pattern => one(crate::parser::parse_pattern_at::<E>(source, offset, options)),
		Entry::Statement => one(crate::parser::parse_statement_at::<E>(source, offset, options)),
		Entry::Params => crate::parser::parse_params_at::<E>(source, offset, options).map(|(mut ast, ids, end)| {
			if comments {
				attach_all(&mut ast, source, &ids, offset);
			}
			params_to_json(&ast, &ids, end, source, positions, output)
		}),
	};
	result.unwrap_or_else(|error| error_to_json(&error, source))
}
