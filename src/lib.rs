//! A JavaScript parser in Rust.

#![warn(unreachable_pub)]

pub mod ast;
pub mod comments;
pub mod error;
pub mod estree;
pub mod interner;
pub mod json;
pub(crate) mod lexer;
pub mod parser;
#[cfg(feature = "typescript")]
pub mod typescript;

pub use error::SyntaxError;
pub use interner::{Interner, StrId};
pub use parser::{Options, Until};

pub fn parse(src: &str, options: Options) -> Result<ast::Ast, SyntaxError> {
	parser::parse::<()>(src, options).map_err(|e| *e)
}

pub fn parse_expression_at(
	src: &str,
	offset: u32,
	options: Options,
) -> Result<(ast::Ast, ast::NodeId, u32), SyntaxError> {
	parser::parse_expression_at::<()>(src, offset, options).map_err(|e| *e)
}

pub fn parse_pattern_at(src: &str, offset: u32, options: Options) -> Result<(ast::Ast, ast::NodeId, u32), SyntaxError> {
	parser::parse_pattern_at::<()>(src, offset, options).map_err(|e| *e)
}

pub fn parse_params_at(
	src: &str,
	offset: u32,
	options: Options,
) -> Result<(ast::Ast, Vec<ast::NodeId>, u32), SyntaxError> {
	parser::parse_params_at::<()>(src, offset, options).map_err(|e| *e)
}

pub fn parse_statement_at(
	src: &str,
	offset: u32,
	options: Options,
) -> Result<(ast::Ast, ast::NodeId, u32), SyntaxError> {
	parser::parse_statement_at::<()>(src, offset, options).map_err(|e| *e)
}
