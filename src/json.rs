//! One entry for every front end: a request describes what to parse and how, and the answer is
//! ESTree JSON, or a JSON object with an `error` the way acorn reports one.

use crate::ast::{Ast, NodeId};
use crate::comments::{attach, attach_all};
use crate::estree::{Emit, error_to_json, list_to_json, to_json};
use crate::{Options, SyntaxError};

/// What to parse at `offset`.
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
	pub options: Options,
}

pub fn parse(source: &str, request: &Request) -> String {
	if !source.is_char_boundary(request.offset as usize) {
		return format!(
			"{{\"error\":{{\"message\":\"offset {} is not a character boundary\",\"pos\":{}}}}}",
			request.offset, request.offset
		);
	}
	#[cfg(feature = "typescript")]
	if request.typescript {
		return run::<crate::typescript::TypeScript>(source, request);
	}
	if request.typescript {
		return String::from("{\"error\":{\"message\":\"built without TypeScript\",\"pos\":0}}");
	}
	run::<()>(source, request)
}

fn run<E: crate::parser::Extension>(source: &str, request: &Request) -> String
where
	E::Data: Emit,
{
	let (offset, options) = (request.offset, request.options);
	let one = |result: Result<(Ast<E::Data>, NodeId), SyntaxError>| {
		result.map(|(mut ast, root)| {
			if request.comments {
				attach(&mut ast, source, root, offset);
			}
			to_json(&ast, root, source)
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
		Entry::Params => crate::parser::parse_params_at::<E>(source, offset, options).map(|(mut ast, ids, _)| {
			if request.comments {
				attach_all(&mut ast, source, &ids, offset);
			}
			list_to_json(&ast, &ids, source)
		}),
	};
	result.unwrap_or_else(|error| error_to_json(&error, source))
}
