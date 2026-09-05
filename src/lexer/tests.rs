use super::Lexer;
use super::token::{
	Comment, Keyword, Token,
	TokenKind::{self, *},
};
use crate::error::SyntaxError;

fn tokens(src: &str) -> Vec<Token> {
	let mut lexer = Lexer::new(src);
	let mut out = Vec::new();
	loop {
		let token = lexer.next_token().unwrap();
		if token.kind == Eof {
			return out;
		}
		out.push(token);
	}
}

fn kinds(src: &str) -> Vec<TokenKind> {
	tokens(src).into_iter().map(|t| t.kind).collect()
}

fn texts(src: &str) -> Vec<&str> {
	tokens(src)
		.into_iter()
		.map(|t| &src[t.start as usize..t.end as usize])
		.collect()
}

fn error(src: &str) -> SyntaxError {
	let mut lexer = Lexer::new(src);
	loop {
		match lexer.next_token() {
			Ok(t) if t.kind == Eof => panic!("no error for {src:?}"),
			Ok(_) => {}
			Err(e) => return e,
		}
	}
}

fn single(src: &str) -> (Lexer<'_>, TokenKind) {
	let mut lexer = Lexer::new(src);
	let kind = lexer.next_token().unwrap().kind;
	(lexer, kind)
}

fn number(src: &str) -> f64 {
	match single(src).1 {
		Number { value, .. } => value,
		kind => panic!("{kind:?}"),
	}
}

fn string(src: &str) -> (std::string::String, bool) {
	let (lexer, kind) = single(src);
	match kind {
		String { value, octal } => (lexer.strings.get(value).to_owned(), octal),
		kind => panic!("{kind:?}"),
	}
}

fn ident(src: &str) -> (std::string::String, bool) {
	let (lexer, kind) = single(src);
	match kind {
		Ident { name, escaped } => (lexer.strings.get(name).to_owned(), escaped),
		kind => panic!("{kind:?}"),
	}
}

fn is_ident(kind: &TokenKind) -> bool {
	matches!(kind, Ident { .. })
}

#[test]
fn punctuators() {
	assert_eq!(
		kinds("{ } ( ) [ ] ; , : ~ @"),
		[
			BraceL, BraceR, ParenL, ParenR, BracketL, BracketR, Semi, Comma, Colon, Tilde, At
		]
	);
	assert_eq!(
		kinds(". ... ?. ?? ??= ? :"),
		[
			Dot,
			Ellipsis,
			QuestionDot,
			QuestionQuestion,
			QuestionQuestionEq,
			Question,
			Colon
		]
	);
	assert_eq!(
		kinds("= == === => != !== !"),
		[Eq, EqEq, EqEqEq, Arrow, BangEq, BangEqEq, Bang]
	);
	assert_eq!(
		kinds("< << <<= <= > >> >>> >>= >>>= >="),
		[Lt, LtLt, LtLtEq, LtEq, Gt, GtGt, GtGtGt, GtGtEq, GtGtGtEq, GtEq]
	);
	assert_eq!(
		kinds("+ ++ += - -- -= * ** *= **= / /= % %="),
		[
			Plus, PlusPlus, PlusEq, Minus, MinusMinus, MinusEq, Star, StarStar, StarEq, StarStarEq, Slash, SlashEq,
			Percent, PercentEq
		]
	);
	assert_eq!(
		kinds("& && &= &&= | || |= ||= ^ ^="),
		[
			Amp, AmpAmp, AmpEq, AmpAmpEq, Pipe, PipePipe, PipeEq, PipePipeEq, Caret, CaretEq
		]
	);
}

#[test]
fn longest_match_without_spaces() {
	assert_eq!(texts("a>>>=b"), ["a", ">>>=", "b"]);
	assert_eq!(texts("a**=b"), ["a", "**=", "b"]);
	assert_eq!(texts("a?.b"), ["a", "?.", "b"]);
	assert_eq!(texts("a?.5:b"), ["a", "?", ".5", ":", "b"]);
	assert_eq!(texts("...a"), ["...", "a"]);
	assert_eq!(texts("..a"), [".", ".", "a"]);
}

#[test]
fn words_and_keywords() {
	let kinds = kinds("let x = await y");
	assert!(
		kinds
			.iter()
			.enumerate()
			.all(|(i, k)| if i == 2 { *k == Eq } else { is_ident(k) })
	);
	assert_eq!(
		super::tests::kinds("if else while function class"),
		[
			Keyword(Keyword::If),
			Keyword(Keyword::Else),
			Keyword(Keyword::While),
			Keyword(Keyword::Function),
			Keyword(Keyword::Class)
		]
	);
	assert!(super::tests::kinds("$foo _bar a1 ünïcödé 変数").iter().all(is_ident));
	assert_eq!(texts("a\u{200d}b"), ["a\u{200d}b"]);
	assert_eq!(ident("hello"), ("hello".to_owned(), false));
}

#[test]
fn identifiers_are_interned() {
	let mut lexer = Lexer::new("a b a");
	let a1 = lexer.next_token().unwrap().kind;
	lexer.next_token().unwrap();
	let a2 = lexer.next_token().unwrap().kind;
	assert_eq!(a1, a2);
	assert_eq!(lexer.strings.len(), 2);
}

#[test]
fn escaped_identifiers() {
	assert_eq!(ident("\\u0061bc"), ("abc".to_owned(), true));
	assert_eq!(ident("\\u{62}c"), ("bc".to_owned(), true));
	assert_eq!(error("\\u0069f").message, "Escape sequence in keyword if");
	assert_eq!(error("\\u0031a").message, "Invalid Unicode escape");
	assert_eq!(error("a\\x").message, "Expecting Unicode escape sequence \\uXXXX");
}

#[test]
fn private_names() {
	let (lexer, kind) = single("#foo");
	let PrivateName(name) = kind else { panic!("{kind:?}") };
	assert_eq!(lexer.strings.get(name), "#foo");
	assert!(matches!(single("#class").1, PrivateName(_)));
	assert_eq!(error("# a").message, "Unexpected character '#'");
}

#[test]
fn hashbang() {
	assert_eq!(texts("#!/usr/bin/env node\nfoo"), ["foo"]);
	let mut lexer = Lexer::new("#!/x");
	lexer.next_token().unwrap();
	assert!(lexer.comments.is_empty());
}

#[test]
fn numbers() {
	assert_eq!(number("42"), 42.0);
	assert_eq!(number("2.75"), 2.75);
	assert_eq!(number(".5"), 0.5);
	assert_eq!(number("5."), 5.0);
	assert_eq!(number("1e3"), 1000.0);
	assert_eq!(number("1.5E-2"), 0.015);
	assert_eq!(number("0xff"), 255.0);
	assert_eq!(number("0o17"), 15.0);
	assert_eq!(number("0B101"), 5.0);
	assert_eq!(number("1_000_000"), 1_000_000.0);
	assert_eq!(number("0x1_f"), 31.0);
	assert_eq!(kinds("10n 0xFFn 0n"), [BigInt, BigInt, BigInt]);
}

#[test]
fn legacy_octal_numbers() {
	assert_eq!(
		single("017").1,
		Number {
			value: 15.0,
			legacy_octal: true
		}
	);
	assert_eq!(
		single("08.5").1,
		Number {
			value: 8.5,
			legacy_octal: true
		}
	);
	assert_eq!(
		single("0").1,
		Number {
			value: 0.0,
			legacy_octal: false
		}
	);
	assert_eq!(error("017n").message, "Identifier directly after number");
	assert_eq!(
		error("01_7").message,
		"Numeric separator is not allowed in legacy octal numeric literals"
	);
}

#[test]
fn number_errors() {
	assert_eq!(error("1a").message, "Identifier directly after number");
	assert_eq!(
		error("1__0").message,
		"Numeric separator must be exactly one underscore"
	);
	assert_eq!(
		error("1_").message,
		"Numeric separator is not allowed at the last of digits"
	);
	assert_eq!(
		error("0x_1").message,
		"Numeric separator is not allowed at the first of digits"
	);
	assert_eq!(error("0x").message, "Expected number in radix 16");
	assert_eq!(error("1e").message, "Invalid number");
	assert_eq!(error("1.5n").message, "Identifier directly after number");
}

#[test]
fn strings() {
	assert_eq!(string("'hello'").0, "hello");
	assert_eq!(string("\"it's\"").0, "it's");
	assert_eq!(string("'a\\nb\\tc'").0, "a\nb\tc");
	assert_eq!(string("'\\x41\\u0042\\u{43}'").0, "ABC");
	assert_eq!(string("'\\ud83d\\ude00'").0, "😀");
	assert_eq!(string("'a\\\nb'").0, "ab");
	assert_eq!(string("'a\\\r\nb'").0, "ab");
	assert_eq!(string("'\\0'"), ("\0".to_owned(), false));
	assert_eq!(string("'\\q'").0, "q");
	assert_eq!(string("'\u{2028}'").0, "\u{2028}");
	assert_eq!(error("'abc").message, "Unterminated string constant");
	assert_eq!(error("'a\nb'").message, "Unterminated string constant");
	assert_eq!(error("'\\x4'").message, "Bad character escape sequence");
	assert_eq!(error("'\\u{110000}'").message, "Bad character escape sequence");
}

#[test]
fn legacy_octal_escapes() {
	assert_eq!(string("'\\101'"), ("A".to_owned(), true));
	assert_eq!(string("'\\08'"), ("\08".to_owned(), true));
	assert_eq!(string("'\\8'"), ("8".to_owned(), true));
	assert_eq!(string("'\\0a'"), ("\0a".to_owned(), false));
}

#[test]
fn templates() {
	let mut lexer = Lexer::new("`a${b}c\\n`");
	assert_eq!(lexer.next_token().unwrap().kind, Backquote);
	let chunk = lexer.read_template().unwrap();
	assert_eq!((chunk.start, chunk.end), (1, 2));
	let Template { cooked, raw, tail } = chunk.kind else {
		panic!()
	};
	assert_eq!(lexer.strings.get(cooked.unwrap()), "a");
	assert_eq!(lexer.strings.get(raw), "a");
	assert!(!tail);
	assert!(is_ident(&lexer.next_token().unwrap().kind));
	assert_eq!(lexer.next_token().unwrap().kind, BraceR);
	let chunk = lexer.read_template().unwrap();
	assert_eq!((chunk.start, chunk.end), (6, 9));
	let Template { cooked, raw, tail } = chunk.kind else {
		panic!()
	};
	assert_eq!(lexer.strings.get(cooked.unwrap()), "c\n");
	assert_eq!(lexer.strings.get(raw), "c\\n");
	assert!(tail);
	assert_eq!(lexer.next_token().unwrap().kind, Eof);
}

#[test]
fn template_invalid_escapes_cook_to_nothing() {
	for src in ["`\\unicode`", "`\\1`", "`\\x`"] {
		let mut lexer = Lexer::new(src);
		lexer.next_token().unwrap();
		let Template { cooked, tail, .. } = lexer.read_template().unwrap().kind else {
			panic!()
		};
		assert_eq!(cooked, None, "{src}");
		assert!(tail);
	}
}

#[test]
fn template_newlines_normalise() {
	let mut lexer = Lexer::new("`a\r\nb\rc`");
	lexer.next_token().unwrap();
	let Template { cooked, raw, .. } = lexer.read_template().unwrap().kind else {
		panic!()
	};
	assert_eq!(lexer.strings.get(cooked.unwrap()), "a\nb\nc");
	assert_eq!(lexer.strings.get(raw), "a\nb\nc");
	let mut lexer = Lexer::new("`abc");
	lexer.next_token().unwrap();
	assert_eq!(lexer.read_template().unwrap_err().message, "Unterminated template");
}

#[test]
fn regex() {
	let mut lexer = Lexer::new("/a[/]b\\/c/gi;");
	let slash = lexer.next_token().unwrap();
	assert_eq!(slash.kind, Slash);
	let re = lexer.read_regex(slash).unwrap();
	let RegExp { pattern, flags } = re.kind else { panic!() };
	assert_eq!(lexer.strings.get(pattern), "a[/]b\\/c");
	assert_eq!(lexer.strings.get(flags), "gi");
	assert_eq!((re.start, re.end), (0, 12));
	assert_eq!(lexer.next_token().unwrap().kind, Semi);

	let mut lexer = Lexer::new("/=x/");
	let t = lexer.next_token().unwrap();
	assert_eq!(t.kind, SlashEq);
	assert_eq!(lexer.read_regex(t).unwrap().end, 4);

	let mut lexer = Lexer::new("/abc\n/");
	let t = lexer.next_token().unwrap();
	assert_eq!(
		lexer.read_regex(t).unwrap_err().message,
		"Unterminated regular expression"
	);
}

#[test]
fn comments_are_collected_and_skipped() {
	let mut lexer = Lexer::new("a // one\n/* two\n */ b");
	assert!(is_ident(&lexer.next_token().unwrap().kind));
	let b = lexer.next_token().unwrap();
	assert!(is_ident(&b.kind));
	assert!(b.newline_before);
	assert_eq!(
		lexer.comments,
		[
			Comment {
				block: false,
				start: 2,
				end: 8
			},
			Comment {
				block: true,
				start: 9,
				end: 19
			}
		]
	);
	assert_eq!(error("/* x").message, "Unterminated comment");
}

#[test]
fn newline_before() {
	let flags: Vec<bool> = tokens("a\nb c\u{2028}d").iter().map(|t| t.newline_before).collect();
	assert_eq!(flags, [false, true, false, true]);
}

#[test]
fn unicode_whitespace() {
	assert_eq!(texts("a\u{a0}b\u{feff}c\u{3000}d"), ["a", "b", "c", "d"]);
}

#[test]
fn unexpected_characters() {
	assert_eq!(error("a ¬ b").message, "Unexpected character '¬'");
	assert_eq!(error("\\").message, "Expecting Unicode escape sequence \\uXXXX");
}
