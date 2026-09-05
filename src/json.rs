//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error` the way acorn reports one.

use crate::ast::{Ast, NodeId};
use crate::comments::{attach, attach_all};
use crate::estree::{Emit, Positions, error_to_json, node_to_json, params_to_json, program_to_json};
use crate::{Options, SyntaxError, Until};

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
	"untilIn",
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
			"untilAs" => self.options.until = Some(Until::As),
			"untilIn" => self.options.until = Some(Until::In),
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

	/// `until` ends an expression at that top-level word operator, over the source's own setting.
	pub fn parse(&self, entry: Entry, utf16_offset: f64, until: Option<Until>) -> String {
		let offset = match self.positions.byte_offset(utf16_offset) {
			Ok(offset) => offset,
			Err(message) => return error_json(&message, 0),
		};
		let mut request = Request {
			entry,
			offset,
			..self.request
		};
		if until.is_some() {
			request.options.until = until;
		}
		parse_with(&self.source, &self.positions, &request)
	}
}

fn parse_with(source: &str, positions: &Positions, request: &Request) -> String {
	if !source.is_char_boundary(request.offset as usize) {
		return error_json(
			&format!("offset {} is inside a surrogate pair", request.offset),
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
	let one = |result: Parsed<E::Data>| {
		result.map(|(mut ast, root, end)| {
			if comments {
				attach(&mut ast, source, root, offset);
			}
			node_to_json(&ast, root, end, source, positions, comments)
		})
	};
	let result = match request.entry {
		Entry::Program => crate::parser::parse::<E>(source, options).map(|mut ast| {
			let root = ast.last();
			if comments {
				attach(&mut ast, source, root, 0);
			}
			program_to_json(&ast, root, source, positions, comments)
		}),
		Entry::Expression => one(crate::parser::parse_expression_at::<E>(source, offset, options)),
		Entry::Pattern => one(crate::parser::parse_pattern_at::<E>(source, offset, options)),
		Entry::Statement => one(crate::parser::parse_statement_at::<E>(source, offset, options)),
		Entry::Params => crate::parser::parse_params_at::<E>(source, offset, options).map(|(mut ast, ids, end)| {
			if comments {
				attach_all(&mut ast, source, &ids, offset);
			}
			params_to_json(&ast, &ids, end, source, positions, comments)
		}),
	};
	result.unwrap_or_else(|error| error_to_json(&error, source))
}
