//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error` the way acorn reports one.

use crate::ast::{Ast, NodeId};
use crate::comments::{attach, attach_all};
use crate::estree::{Emit, error_to_json, list_to_json, to_json};
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
	pub options: Options,
}

/// The names front ends accept for the request's switches, as acorn spells them.
pub const FLAGS: [&str; 9] = [
	"typescript",
	"comments",
	"locations",
	"script",
	"preserveParens",
	"allowReturnOutsideFunction",
	"allowAwaitOutsideFunction",
	"allowSuperOutsideMethod",
	"allowUndeclaredExports",
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
			_ => {}
		}
	}
}

/// The byte offset of a UTF-16 offset, so front ends can take positions the way acorn does.
pub fn byte_offset(source: &str, utf16: f64) -> Result<u32, String> {
	if !(utf16 >= 0.0 && utf16.fract() == 0.0 && utf16 <= u32::MAX as f64) {
		return Err(format!("offset {utf16} is not a valid position"));
	}
	let target = utf16 as usize;
	let mut units = 0;
	for (byte, c) in source.char_indices() {
		if units == target {
			return Ok(byte as u32);
		}
		if units > target {
			return Err(format!("offset {target} is inside a surrogate pair"));
		}
		units += c.len_utf16();
	}
	if units < target {
		return Err(format!("offset {target} is past the end of the source"));
	}
	Ok(source.len() as u32)
}

pub fn error_json(message: &str, pos: u32) -> String {
	let mut out = String::from("{\"error\":{\"message\":");
	crate::estree::write_json_string(&mut out, message);
	out.push_str(&format!(",\"pos\":{pos}}}}}"));
	out
}

pub fn parse(source: &str, request: &Request) -> String {
	if !source.is_char_boundary(request.offset as usize) {
		return error_json(
			&format!("offset {} is not a character boundary", request.offset),
			request.offset,
		);
	}
	#[cfg(feature = "typescript")]
	if request.typescript {
		return run::<crate::typescript::TypeScript>(source, request);
	}
	#[cfg(not(feature = "typescript"))]
	if request.typescript {
		return error_json("built without TypeScript", 0);
	}
	run::<()>(source, request)
}

fn run<E: crate::parser::Extension>(source: &str, request: &Request) -> String
where
	E::Data: Emit,
{
	let (offset, options, locations) = (request.offset, request.options, request.locations);
	let one = |result: Result<(Ast<E::Data>, NodeId), SyntaxError>| {
		result.map(|(mut ast, root)| {
			if request.comments {
				attach(&mut ast, source, root, offset);
			}
			to_json(&ast, root, source, locations)
		})
	};
	let result = match request.entry {
		Entry::Program => one(crate::parser::parse::<E>(source, options).map(|ast| {
			let root = ast.last();
			(ast, root)
		})),
		Entry::Expression => one(crate::parser::parse_expression_at::<E>(source, offset, options)),
		Entry::Pattern => one(crate::parser::parse_pattern_at::<E>(source, offset, options)),
		Entry::Statement => one(crate::parser::parse_statement_at::<E>(source, offset, options)),
		Entry::Params => crate::parser::parse_params_at::<E>(source, offset, options).map(|(mut ast, ids, end)| {
			if request.comments {
				attach_all(&mut ast, source, &ids, offset);
			}
			let mut out = String::from("{\"params\":");
			out.push_str(&list_to_json(&ast, &ids, source, locations));
			out.push_str(&format!(",\"end\":{}}}", crate::estree::utf16_offset(source, end)));
			out
		}),
	};
	result.unwrap_or_else(|error| error_to_json(&error, source))
}
