use crate::interner::StrId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Token {
	pub(crate) kind: TokenKind,
	pub(crate) start: u32,
	pub(crate) end: u32,
	pub(crate) newline_before: bool,
	/// An identifier or keyword written with a unicode escape.
	pub(crate) escaped: bool,
	/// A string, template or regular expression the source ends or a line break cuts short.
	pub(crate) unclosed: bool,
	/// One of the host's stop tokens, read outside every bracket the parse opened.
	pub(crate) stop: bool,
}

impl Token {
	pub(crate) fn eof(pos: u32) -> Token {
		Token {
			kind: TokenKind::Eof,
			start: pos,
			end: pos,
			newline_before: false,
			escaped: false,
			unclosed: false,
			stop: false,
		}
	}

	/// Whether an expression could end after this token: it is an operand or closes one, so
	/// what follows is an operator, a subscript, or the host's own syntax.
	pub(crate) fn ends_operand(&self) -> bool {
		use TokenKind::*;
		match self.kind {
			Ident(_) | PrivateName(_) | Number(_) | BigInt | String(_) | Template { .. } | RegExp { .. } => true,
			ParenR | BracketR | BraceR | PlusPlus | MinusMinus => true,
			Keyword(k) => matches!(
				k,
				self::Keyword::This
					| self::Keyword::Null
					| self::Keyword::True
					| self::Keyword::False
					| self::Keyword::Super
			),
			_ => false,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TokenKind {
	Eof,
	Ident(StrId),
	PrivateName(StrId),
	Keyword(Keyword),
	Number(f64),
	BigInt,
	String(StrId),
	Template {
		cooked: Option<StrId>,
		raw: StrId,
		tail: bool,
	},
	RegExp {
		pattern: StrId,
		flags: StrId,
	},

	BraceL,
	BraceR,
	ParenL,
	ParenR,
	BracketL,
	BracketR,
	Semi,
	Comma,
	Dot,
	Ellipsis,
	Question,
	QuestionDot,
	Colon,
	Arrow,
	Backquote,
	At,

	Eq,
	PlusEq,
	MinusEq,
	StarEq,
	SlashEq,
	PercentEq,
	StarStarEq,
	LtLtEq,
	GtGtEq,
	GtGtGtEq,
	AmpEq,
	PipeEq,
	CaretEq,
	AmpAmpEq,
	PipePipeEq,
	QuestionQuestionEq,

	EqEq,
	BangEq,
	EqEqEq,
	BangEqEq,
	Lt,
	Gt,
	LtEq,
	GtEq,
	LtLt,
	GtGt,
	GtGtGt,
	Plus,
	Minus,
	Star,
	Slash,
	Percent,
	StarStar,
	Amp,
	Pipe,
	Caret,
	AmpAmp,
	PipePipe,
	QuestionQuestion,
	Bang,
	Tilde,
	PlusPlus,
	MinusMinus,
}

/// The reserved words, each with its spelling; `from_word` lists them again by length, and a
/// test holds the two lists together.
macro_rules! keywords {
	($($name:ident $text:literal),* $(,)?) => {
		#[derive(Clone, Copy, Debug, PartialEq, Eq)]
		pub(crate) enum Keyword {
			$($name),*
		}

		impl Keyword {
			#[cfg(test)]
			pub(crate) const ALL: &'static [Keyword] = &[$(Keyword::$name),*];

			// a table lookup measured +2.8%: it flipped inlining of `is` and `eat`
			pub(crate) fn as_str(self) -> &'static str {
				match self {
					$(Keyword::$name => $text),*
				}
			}
		}
	};
}

keywords! {
	Break "break", Case "case", Catch "catch", Class "class", Const "const", Continue "continue",
	Debugger "debugger", Default "default", Delete "delete", Do "do", Else "else", Export "export",
	Extends "extends", False "false", Finally "finally", For "for", Function "function", If "if",
	Import "import", In "in", Instanceof "instanceof", New "new", Null "null", Return "return",
	Super "super", Switch "switch", This "this", Throw "throw", True "true", Try "try",
	Typeof "typeof", Var "var", Void "void", While "while", With "with",
}

/// What a word is when it stands as an identifier, computed once per distinct string.
pub(crate) mod word {
	pub(crate) const YIELD: u8 = 1;
	pub(crate) const AWAIT: u8 = 2;
	pub(crate) const ARGUMENTS: u8 = 4;
	pub(crate) const ENUM: u8 = 8;
	/// Reserved in strict mode.
	pub(crate) const STRICT: u8 = 16;
	/// A keyword written with an escape, which the lexer read as a word.
	pub(crate) const KEYWORD: u8 = 32;
	pub(crate) const EVAL: u8 = 64;

	pub(crate) fn flags(word: &str) -> u8 {
		let mut flags = match word.as_bytes() {
			b"yield" => YIELD | STRICT,
			b"await" => AWAIT,
			b"arguments" => ARGUMENTS,
			b"eval" => EVAL,
			b"enum" => ENUM,
			b"implements" | b"interface" | b"let" | b"package" | b"private" | b"protected" | b"public" | b"static" => {
				STRICT
			}
			_ => 0,
		};
		if super::Keyword::from_word(word).is_some() {
			flags |= KEYWORD;
		}
		flags
	}
}

impl Keyword {
	// as bytes: a `str` pattern is compared through memcmp, one call for each word of its length
	// a hashed index measured +2.6%
	pub(crate) fn from_word(word: &str) -> Option<Keyword> {
		use Keyword::*;
		Some(match word.as_bytes() {
			b"do" => Do,
			b"if" => If,
			b"in" => In,
			b"for" => For,
			b"new" => New,
			b"try" => Try,
			b"var" => Var,
			b"case" => Case,
			b"else" => Else,
			b"null" => Null,
			b"this" => This,
			b"true" => True,
			b"void" => Void,
			b"with" => With,
			b"break" => Break,
			b"catch" => Catch,
			b"class" => Class,
			b"const" => Const,
			b"false" => False,
			b"super" => Super,
			b"throw" => Throw,
			b"while" => While,
			b"delete" => Delete,
			b"export" => Export,
			b"import" => Import,
			b"return" => Return,
			b"switch" => Switch,
			b"typeof" => Typeof,
			b"default" => Default,
			b"extends" => Extends,
			b"finally" => Finally,
			b"continue" => Continue,
			b"debugger" => Debugger,
			b"function" => Function,
			b"instanceof" => Instanceof,
			_ => return None,
		})
	}
}
