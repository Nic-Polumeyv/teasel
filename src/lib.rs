//! A JavaScript parser in Rust.

#![warn(unreachable_pub)]

pub mod ast;
pub mod error;
pub mod estree;
pub mod interner;
pub(crate) mod lexer;
pub mod parser;

pub use error::SyntaxError;
pub use interner::{Interner, StrId};
pub use parser::{Options, parse, parse_expression_at, parse_params_at, parse_pattern_at, parse_statement_at};
