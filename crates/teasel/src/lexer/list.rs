//! What a parenthesized list that may be an arrow's parameters is, told before the parser reads
//! it: by its first tokens, or by the token after its closer.

use super::Lexer;
use super::token::{Keyword, Token, TokenKind};

/// How a list that may be an arrow's parameters is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
	Parameters,
	Expression,
	/// A `:` at the offset follows the closer: only reading the type after it tells.
	Typed(u32),
	/// The tokens do not tell: the head is tried as an arrow's.
	Unsure,
}

/// What a `/` is after a token.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slash {
	Divide,
	Regex,
	/// Only the parser can tell.
	Unsure,
}

/// A bracket open during a scan.
#[derive(Clone, Copy)]
pub(super) struct Frame {
	closer: TokenKind,
	opener: u32,
	/// What a `/` is after the closer.
	after: Slash,
}

/// What a `/` is after a `++`, `--` or TypeScript `!` read where a `/` would be `slash`.
fn postfix(slash: Slash, token: &Token) -> Slash {
	match slash {
		Slash::Divide if !token.newline_before => Slash::Divide,
		Slash::Unsure => Slash::Unsure,
		_ => Slash::Regex,
	}
}

/// A template's substitution, closed by `}` as a brace is.
const TEMPLATE: TokenKind = TokenKind::Backquote;

impl Lexer<'_> {
	/// How the list whose `(` is at `open` is read, with the lexer just past its first token
	/// `first`; `call` when it follows `async`, where it may be arguments. The lexer is left where
	/// it was, and reads ahead as the parse does: a recovering one reads past what it records.
	pub(crate) fn list_role(&mut self, open: u32, first: &Token, typescript: bool, call: bool) -> Role {
		let mark = self.mark();
		let role = match self.first_tokens(first, typescript, call) {
			Ok(Some(role)) => role,
			Ok(None) => {
				self.rewind(mark);
				match self.roles.get(&open) {
					Some(&role) => role,
					None => self.scan(open, first, typescript).unwrap_or(Role::Unsure),
				}
			}
			Err(_) => Role::Unsure,
		};
		self.rewind(mark);
		role
	}

	/// Whether a `,` or `)` comes next, past nothing but white space: a cheap look that may miss
	/// one behind a comment.
	pub(crate) fn at_item_end(&self) -> bool {
		let bytes = self.source().as_bytes();
		let mut at = self.pos;
		while let Some(b' ' | b'\t' | b'\n' | b'\r') = bytes.get(at) {
			at += 1;
		}
		matches!(bytes.get(at), Some(b',' | b')'))
	}

	/// What the first tokens tell, derived from what can start only a parameter list or never
	/// one; `None` when both readings stay open.
	fn first_tokens(&mut self, first: &Token, typescript: bool, call: bool) -> super::Result<Option<Role>> {
		use TokenKind as T;
		let mut token = Token::eof(0);
		let first = first.kind;
		match first {
			T::ParenR => {
				self.next_token_into(&mut token)?;
				return Ok(Some(match token.kind {
					_ if token.stop => Role::Expression,
					T::Arrow => Role::Parameters,
					// `a ? async() : b`
					T::Colon if typescript && call => Role::Typed(token.start),
					T::Colon | T::BraceL if typescript && !call => Role::Parameters,
					_ => Role::Expression,
				}));
			}
			// a call spreads its arguments
			T::Ellipsis if !call => return Ok(Some(Role::Parameters)),
			T::Ellipsis | T::BracketL | T::BraceL => return Ok(None),
			T::Ident(_) => {}
			T::Keyword(Keyword::This) if typescript => {}
			_ => return Ok(Some(Role::Expression)),
		}
		self.next_token_into(&mut token)?;
		Ok(match token.kind {
			T::ParenR => {
				self.next_token_into(&mut token)?;
				Some(self.after_closer(&token, typescript))
			}
			T::Comma | T::Eq => None,
			T::Colon if typescript => Some(Role::Parameters),
			T::Question if typescript => {
				self.next_token_into(&mut token)?;
				Some(match token.kind {
					T::Colon | T::Comma | T::Eq | T::ParenR => Role::Parameters,
					_ => Role::Expression,
				})
			}
			// `(public x`, which only a parameter list can hold; `(public as T)` is an assertion
			T::Ident(second) if typescript && self.is_modifier(first) => Some(match self.strings.get(second) {
				"as" | "satisfies" => Role::Expression,
				_ => Role::Parameters,
			}),
			_ => Some(Role::Expression),
		})
	}

	fn is_modifier(&self, kind: TokenKind) -> bool {
		let TokenKind::Ident(word) = kind else {
			return false;
		};
		matches!(
			self.strings.get(word),
			"public"
				| "private" | "protected"
				| "readonly" | "static"
				| "abstract" | "declare"
				| "override" | "accessor"
		)
	}

	/// What the token after a list's closer makes the list.
	fn after_closer(&self, follower: &Token, typescript: bool) -> Role {
		match follower.kind {
			// the host's syntax starts there: the list ends the expression
			_ if follower.stop => Role::Expression,
			TokenKind::Arrow if !follower.newline_before => Role::Parameters,
			TokenKind::Colon if typescript => Role::Typed(follower.start),
			_ => Role::Expression,
		}
	}

	/// Reads through the closer of the list whose `(` is at `open` and the token after it,
	/// keeping the role of every list closed on the way. `None` when a token cannot be read
	/// without parsing: a `/` only the parser can place, or source that does not tokenize.
	pub(super) fn scan(&mut self, open: u32, first: &Token, typescript: bool) -> Option<Role> {
		use TokenKind as T;
		let mut frames = std::mem::take(&mut self.frames);
		frames.clear();
		frames.push(Frame {
			closer: T::ParenR,
			opener: open,
			after: Slash::Divide,
		});
		let mut slash = Slash::Regex;
		let mut closed: Option<u32> = None;
		let (mut prev, mut prev2) = (T::ParenL, T::Eof);
		let mut token = Token::eof(0);
		let mut unread = Some(*first);
		let role = loop {
			match unread.take() {
				Some(first) => token = first,
				None => {
					if self.next_token_into(&mut token).is_err() {
						break None;
					}
				}
			}
			if let Some(at) = closed.take() {
				let role = self.after_closer(&token, typescript);
				self.roles.insert(at, role);
				if frames.is_empty() {
					break Some(role);
				}
			}
			match token.kind {
				T::Slash | T::SlashEq => match slash {
					Slash::Regex => {
						let Ok(regex) = self.read_regex(token) else { break None };
						token = regex;
						slash = Slash::Divide;
					}
					Slash::Divide => slash = Slash::Regex,
					Slash::Unsure => break None,
				},
				T::Backquote => match self.template_part() {
					Some(true) => slash = Slash::Divide,
					Some(false) => {
						frames.push(Frame {
							closer: TEMPLATE,
							opener: token.start,
							after: Slash::Divide,
						});
						slash = Slash::Regex;
					}
					None => break None,
				},
				T::ParenL => {
					let statement = matches!(
						prev,
						T::Keyword(
							Keyword::If
								| Keyword::While | Keyword::For
								| Keyword::With | Keyword::Switch
								| Keyword::Catch
						)
					) && !matches!(prev2, T::Dot | T::QuestionDot)
						|| (prev2 == T::Keyword(Keyword::For)
							&& matches!(prev, T::Ident(word) if self.strings.get(word) == "await"));
					frames.push(Frame {
						closer: T::ParenR,
						opener: token.start,
						after: if statement { Slash::Regex } else { Slash::Divide },
					});
					slash = Slash::Regex;
				}
				T::BracketL => {
					frames.push(Frame {
						closer: T::BracketR,
						opener: token.start,
						after: Slash::Divide,
					});
					slash = Slash::Regex;
				}
				T::BraceL => {
					frames.push(Frame {
						closer: T::BraceR,
						opener: token.start,
						after: Slash::Unsure,
					});
					slash = Slash::Regex;
				}
				T::ParenR | T::BracketR | T::BraceR => {
					let Some(frame) = frames.pop() else { break None };
					if frame.closer == TEMPLATE && token.kind == T::BraceR {
						match self.template_part() {
							Some(true) => slash = Slash::Divide,
							Some(false) => {
								frames.push(frame);
								slash = Slash::Regex;
							}
							None => break None,
						}
					} else if frame.closer != token.kind {
						break None;
					} else {
						if token.kind == T::ParenR {
							closed = Some(frame.opener);
						}
						slash = frame.after;
					}
				}
				T::Eof => break None,
				T::Ident(word) => {
					slash = match self.strings.get(word) {
						"yield" | "await" | "of" | "let" => Slash::Unsure,
						_ => Slash::Divide,
					}
				}
				T::PlusPlus | T::MinusMinus => slash = postfix(slash, &token),
				T::Bang if typescript => slash = postfix(slash, &token),
				T::Gt if typescript && slash == Slash::Divide => slash = Slash::Unsure,
				_ => {
					slash = if token.ends_operand() {
						Slash::Divide
					} else {
						Slash::Regex
					}
				}
			}
			prev2 = prev;
			prev = token.kind;
		};
		if role.is_none() {
			for frame in &frames {
				if frame.closer == T::ParenR {
					self.roles.insert(frame.opener, Role::Unsure);
				}
			}
		}
		self.frames = frames;
		role
	}

	/// The rest of a template after its backquote or a substitution's `}`, through the next
	/// substitution's `${` (false) or its end (true).
	fn template_part(&mut self) -> Option<bool> {
		match self.read_template().ok()?.kind {
			TokenKind::Template { tail, .. } => Some(tail),
			_ => None,
		}
	}
}
