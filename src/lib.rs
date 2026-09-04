//! A JavaScript parser in Rust.

pub mod error;
pub mod lexer;
pub mod token;
pub mod unicode;

pub use error::SyntaxError;
pub use lexer::Lexer;
pub use token::{Comment, Keyword, Token, TokenKind};
