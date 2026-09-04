//! A JavaScript parser in Rust.

pub mod error;
pub mod interner;
pub mod lexer;

pub use error::SyntaxError;
pub use interner::{Interner, StrId};
pub use lexer::Lexer;
pub use lexer::token::{Comment, Keyword, Token, TokenKind};
