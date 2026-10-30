//! A JavaScript parser in Rust.

pub mod ast;
pub mod comments;
pub mod error;
pub mod estree;
pub mod handed;
pub mod host;
pub mod interner;
pub mod json;
pub mod layout;
pub(crate) mod lexer;
pub mod parser;
pub mod recipe;
pub mod scopes;
pub mod typescript;

pub use error::{Code, SyntaxError};
pub use interner::{Interner, StrId};
pub use parser::{Entry, Options};

/// Parses one JavaScript `entry` at `start` of `src`, as TypeScript when `options` say so; see
/// `parser::parse_at`. The tree comes back on an error too: the error's args name its strings.
pub fn parse_at(
	src: &str,
	start: u32,
	entry: Entry,
	options: Options,
	stop: &str,
) -> (ast::Ast, Result<(ast::List, u32), SyntaxError>) {
	let (ast, parsed) = parser::parse_at(src, start, entry, options, stop, None);
	(*ast, parsed.map_err(|e| *e))
}

#[cfg(test)]
#[global_allocator]
static COUNTING: parser::tests::Counting = parser::tests::Counting;
