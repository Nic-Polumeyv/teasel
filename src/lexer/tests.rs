use super::Lexer;
use super::token::{
	Keyword, Token,
	TokenKind::{self, *},
};
use crate::ast::{Comment, CommentKind};
use crate::error::SyntaxError;
use crate::interner::StrId;

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

fn error_in(src: &str, strict: bool) -> SyntaxError {
	let mut lexer = Lexer::new(src);
	lexer.strict = strict;
	loop {
		match lexer.next_token() {
			Ok(t) if t.kind == Eof => panic!("no error for {src:?}"),
			Ok(_) => {}
			Err(e) => return e,
		}
	}
}

fn error(src: &str) -> (std::string::String, u32) {
	let e = error_in(src, false);
	(e.message, e.pos)
}

fn strict_error(src: &str) -> (std::string::String, u32) {
	let e = error_in(src, true);
	(e.message, e.pos)
}

fn single(src: &str) -> (Lexer<'_>, Token) {
	let mut lexer = Lexer::new(src);
	let token = lexer.next_token().unwrap();
	(lexer, token)
}

fn number(src: &str) -> f64 {
	match single(src).1.kind {
		Number(value) => value,
		kind => panic!("{kind:?}"),
	}
}

fn string(src: &str) -> std::string::String {
	let (lexer, token) = single(src);
	match token.kind {
		String(value) => lexer.strings.get(value).to_owned(),
		kind => panic!("{kind:?}"),
	}
}

fn ident(src: &str) -> (std::string::String, bool) {
	let (lexer, token) = single(src);
	match token.kind {
		Ident(name) => (lexer.strings.get(name).to_owned(), token.escaped),
		kind => panic!("{kind:?}"),
	}
}

fn is_ident(kind: &TokenKind) -> bool {
	matches!(kind, Ident(_))
}

#[test]
fn punctuators() {
	assert_eq!(
		kinds("{ } ( ) [ ] ; , : ~"),
		[
			BraceL, BraceR, ParenL, ParenR, BracketL, BracketR, Semi, Comma, Colon, Tilde
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
	assert_eq!(error("x@y"), ("Unexpected character '@'".into(), 1));
}

#[test]
fn longest_match_without_spaces() {
	assert_eq!(texts("a>>>=b"), ["a", ">>>=", "b"]);
	assert_eq!(texts("a**=b"), ["a", "**=", "b"]);
	assert_eq!(texts("a?.b"), ["a", "?.", "b"]);
	assert_eq!(texts("a?.5:b"), ["a", "?", ".5", ":", "b"]);
	assert_eq!(texts("a?."), ["a", "?", "."]);
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
	assert!(super::tests::kinds("breaks constant iffy").iter().all(is_ident));
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
	let token = single("\\u0069f").1;
	assert_eq!((token.kind, token.escaped), (Keyword(Keyword::If), true));
	assert_eq!(error("\\u0031a"), ("Invalid Unicode escape".into(), 0));
	assert_eq!(error("\\ud801\\udc00"), ("Invalid Unicode escape".into(), 0));
	assert_eq!(error("a\\x"), ("Expecting Unicode escape sequence \\uXXXX".into(), 2));
}

#[test]
fn private_names() {
	let (lexer, token) = single("#foo");
	let PrivateName(name) = token.kind else {
		panic!("{:?}", token.kind)
	};
	assert_eq!(lexer.strings.get(name), "foo");
	assert!(matches!(single("#class").1.kind, PrivateName(_)));
	assert_eq!(error("# a"), ("Unexpected character ' '".into(), 1));
}

#[test]
fn hashbang() {
	assert_eq!(texts("#!/usr/bin/env node\nfoo"), ["foo"]);
	let mut lexer = Lexer::new("#!/x\ny");
	lexer.next_token().unwrap();
	assert_eq!(
		lexer.comments,
		[Comment {
			kind: CommentKind::Hashbang,
			start: 0,
			end: 4
		}]
	);
	assert_eq!(&"#!/x\ny"[lexer.comments[0].text_range()], "/x");
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
	assert_eq!(kinds("0x1na"), [BigInt, Ident(StrId(0))]);
	assert_eq!(kinds("1\\u0061"), [Number(1.0), Ident(StrId(0))]);
}

#[test]
fn legacy_octal_numbers() {
	assert_eq!(number("017"), 15.0);
	assert_eq!(number("08.5"), 8.5);
	assert_eq!(number("0"), 0.0);
	assert_eq!(number("0374547736741752762421"), 4552292526566663700.0);
	assert_eq!(error("017n").0, "Identifier directly after number");
	assert_eq!(
		error("01_7").0,
		"Numeric separator is not allowed in legacy octal numeric literals"
	);
	assert_eq!(
		error("0_1"),
		(
			"Numeric separator is not allowed in legacy octal numeric literals".into(),
			1
		)
	);
	assert_eq!(strict_error("017"), ("Invalid number".into(), 0));
	assert_eq!(strict_error("06B"), ("Invalid number".into(), 0));
	assert_eq!(strict_error("x = 08"), ("Invalid number".into(), 4));
}

#[test]
fn number_errors() {
	assert_eq!(error("1a"), ("Identifier directly after number".into(), 1));
	assert_eq!(error("1__0").0, "Numeric separator must be exactly one underscore");
	assert_eq!(error("1_").0, "Numeric separator is not allowed at the last of digits");
	assert_eq!(
		error("0x_1").0,
		"Numeric separator is not allowed at the first of digits"
	);
	assert_eq!(error("0x"), ("Expected number in radix 16".into(), 2));
	assert_eq!(error("1e").0, "Invalid number");
	assert_eq!(error("1.5n").0, "Identifier directly after number");
}

#[test]
fn strings() {
	assert_eq!(string("'hello'"), "hello");
	assert_eq!(string("\"it's\""), "it's");
	assert_eq!(string("'a\\nb\\tc'"), "a\nb\tc");
	assert_eq!(string("'\\x41\\u0042\\u{43}'"), "ABC");
	assert_eq!(string("'\\ud83d\\ude00'"), "😀");
	assert_eq!(string("'\\u{d83d}\\u{de00}'"), "😀");
	assert_eq!(string("'\\ud83d\\u{de00}'"), "😀");
	assert_eq!(string("'\\ud83dx'"), "\u{fffd}x");
	assert_eq!(string("'a\\\nb'"), "ab");
	assert_eq!(string("'a\\\r\nb'"), "ab");
	assert_eq!(string("'\\0'"), "\0");
	assert_eq!(string("'\\q'"), "q");
	assert_eq!(string("'\u{2028}'"), "\u{2028}");
	assert_eq!(error("'abc").0, "Unterminated string constant");
	assert_eq!(error("'a\nb'").0, "Unterminated string constant");
	assert_eq!(error("'\\"), ("Unterminated string constant".into(), 0));
	assert_eq!(error("'\\x4'"), ("Bad character escape sequence".into(), 3));
	assert_eq!(error("'\\u{}'"), ("Bad character escape sequence".into(), 4));
	assert_eq!(error("'\\u{110000}'"), ("Code point out of bounds".into(), 4));
}

#[test]
fn legacy_octal_escapes() {
	assert_eq!(string("'\\101'"), "A");
	assert_eq!(string("'\\08'"), "\08");
	assert_eq!(string("'\\8'"), "8");
	assert_eq!(string("'\\0a'"), "\0a");
	assert_eq!(strict_error("x = '\\101'"), ("Octal literal in strict mode".into(), 5));
	assert_eq!(strict_error("'\\8'"), ("Invalid escape sequence".into(), 2));
	assert_eq!(strict_error("'\\08'").0, "Octal literal in strict mode");
	let mut lexer = Lexer::new("'\\0'");
	lexer.strict = true;
	assert!(matches!(lexer.next_token().unwrap().kind, String(_)));
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
	assert_eq!(lexer.strings.get(raw), "a");
	assert_eq!(cooked, Some(raw));
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
	for src in ["`\\unicode`", "`\\1`", "`\\x`", "`\\8`"] {
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
	let e = lexer.read_template().unwrap_err();
	assert_eq!((e.message.as_str(), e.pos), ("Unterminated template", 1));
	let mut lexer = Lexer::new("`");
	lexer.next_token().unwrap();
	let e = lexer.read_template().unwrap_err();
	assert_eq!((e.message.as_str(), e.pos), ("Unterminated template literal", 1));
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
	let e = lexer.read_regex(t).unwrap_err();
	assert_eq!((e.message.as_str(), e.pos), ("Unterminated regular expression", 1));

	let mut lexer = Lexer::new("/a/\\u0067");
	let t = lexer.next_token().unwrap();
	let e = lexer.read_regex(t).unwrap_err();
	assert_eq!((e.message.as_str(), e.pos), ("Unexpected token", 3));
	let mut lexer = Lexer::new("/a/\\u{30}");
	let t = lexer.next_token().unwrap();
	let e = lexer.read_regex(t).unwrap_err();
	assert_eq!((e.message.as_str(), e.pos), ("Invalid Unicode escape", 3));
	let mut lexer = Lexer::new("/a/\\ux");
	let t = lexer.next_token().unwrap();
	let e = lexer.read_regex(t).unwrap_err();
	assert_eq!((e.message.as_str(), e.pos), ("Bad character escape sequence", 5));
}

#[test]
fn regex_validation() {
	let regex = |src: &str| {
		let mut lexer = Lexer::new(src);
		let t = lexer.next_token().unwrap();
		lexer.read_regex(t).map(|_| ()).map_err(|e| (e.message, e.pos))
	};
	assert!(regex("/(?<a>x)|(?<a>y)/").is_ok());
	assert!(regex("/[\\p{L}--[a-z]]/v").is_ok());
	assert!(regex("/(?i:a)b/").is_ok());
	assert!(regex("/\\1(a)/").is_ok());
	assert_eq!(
		regex("/(/"),
		Err(("Invalid regular expression: /(/: Unterminated group".into(), 1))
	);
	assert_eq!(regex("/a/gg"), Err(("Duplicate regular expression flag".into(), 1)));
	assert_eq!(regex("/a/uv"), Err(("Invalid regular expression flag".into(), 1)));
	assert_eq!(
		regex("/\\1/u"),
		Err(("Invalid regular expression: /\\1/: Invalid escape".into(), 1))
	);
	assert_eq!(
		regex("/[b-a]/"),
		Err((
			"Invalid regular expression: /[b-a]/: Range out of order in character class".into(),
			1
		))
	);
	assert_eq!(
		regex("/(?<a>x)(?<a>y)/"),
		Err((
			"Invalid regular expression: /(?<a>x)(?<a>y)/: Duplicate capture group name".into(),
			1
		))
	);
	assert_eq!(
		regex("/\\p{Nope}/u"),
		Err((
			"Invalid regular expression: /\\p{Nope}/: Invalid property name".into(),
			1
		))
	);
	assert!(regex("/{*/").is_ok());
	assert_eq!(
		regex("/{*/u"),
		Err(("Invalid regular expression: /{*/: Lone quantifier brackets".into(), 1))
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
				kind: CommentKind::Line,
				start: 2,
				end: 8
			},
			Comment {
				kind: CommentKind::Block,
				start: 9,
				end: 19
			}
		]
	);
	assert_eq!(&"a // one\n/* two\n */ b"[lexer.comments[1].text_range()], " two\n ");
	assert_eq!(error("/* x").0, "Unterminated comment");
}

#[test]
fn html_comments_outside_modules() {
	assert_eq!(texts("a <!-- b\n+c"), ["a", "+", "c"]);
	assert_eq!(texts("--> x\ny"), ["y"]);
	assert_eq!(texts("a = b-->1;\n --> nothing"), ["a", "=", "b", "--", ">", "1", ";"]);
	assert_eq!(texts("/* c */--> x\ny"), ["y"]);
	let mut lexer = Lexer::new("<!-- a\nb");
	lexer.next_token().unwrap();
	assert_eq!(lexer.comments[0].kind, CommentKind::HtmlOpen);
	assert_eq!(&"<!-- a\nb"[lexer.comments[0].text_range()], " a");
	let mut lexer = Lexer::new("a <!-- b");
	lexer.module = true;
	lexer.next_token().unwrap();
	assert_eq!(lexer.next_token().unwrap().kind, Lt);
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
	assert_eq!(error("a ¬ b").0, "Unexpected character '¬'");
	assert_eq!(error("\\").0, "Expecting Unicode escape sequence \\uXXXX");
}
