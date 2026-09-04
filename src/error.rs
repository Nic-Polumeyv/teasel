use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
	pub pos: u32,
	pub message: String,
}

impl SyntaxError {
	pub fn new(pos: u32, message: impl Into<String>) -> Self {
		Self {
			pos,
			message: message.into(),
		}
	}
}

impl fmt::Display for SyntaxError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} ({})", self.message, self.pos)
	}
}

impl std::error::Error for SyntaxError {}
