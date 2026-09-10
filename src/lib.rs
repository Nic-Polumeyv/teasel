//! A JavaScript parser in Rust.

#![warn(unreachable_pub)]

pub mod ast;
pub mod comments;
pub mod error;
pub mod estree;
pub mod host;
pub mod interner;
pub mod json;
pub(crate) mod lexer;
pub mod parser;
pub mod scopes;
#[cfg(feature = "typescript")]
pub mod typescript;

pub use error::{Code, SyntaxError};
pub use interner::{Interner, StrId};
pub use parser::{Entry, Options};

/// Parses one `entry` at `start` of `src` cut at `end`; see `parser::parse_at`.
pub fn parse_at(
	src: &str,
	start: u32,
	end: Option<u32>,
	entry: Entry,
	options: Options,
	stop: &str,
) -> Result<(ast::Ast, ast::List, u32), SyntaxError> {
	parser::parse_at::<()>(src, start, end, entry, options, stop, None).map_err(|e| *e)
}

#[cfg(test)]
#[global_allocator]
static COUNTING: parser::tests::Counting = parser::tests::Counting;
