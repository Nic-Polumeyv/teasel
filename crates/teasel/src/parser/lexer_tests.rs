use super::Kind::{self, *};
use std::string::String;
use super::{Arg, ESCAPED, Lexer, NEWLINE_BEFORE, Token};
use crate::ast::{Ast, Comment, CommentKind, NodeId, NodeKind};
use crate::interner::Interner;
use crate::{Entry, Options, SyntaxError};

fn lexer(src: &str) -> Lexer<'_> {
	Lexer::new(src, false, true)
}

/// Every token of a script, a `/` read as the operator.
fn tokens(src: &str) -> Vec<Token> {
	let mut lexer = lexer(src);
	let mut out = Vec::new();
	loop {
		let token = lexer.next(false);
		assert!(lexer.error.is_none(), "an error in {src:?}");
		if token.kind == Eof {
			return out;
		}
		out.push(token);
	}
}

fn kinds(src: &str) -> Vec<Kind> {
	tokens(src).into_iter().map(|t| t.kind).collect()
}

fn texts(src: &str) -> Vec<&str> {
	tokens(src)
		.into_iter()
		.map(|t| &src[t.start as usize..t.end as usize])
		.collect()
}

/// The lexer's first error, described as an error of the tree is, and where it stands.
fn lexed(lexer: &mut Lexer<'_>) -> Option<(String, u32)> {
	let e = lexer.error?;
	let mut strings = Interner::default();
	let error = SyntaxError::new(e.pos, e.code);
	let error = match e.arg {
		Arg::None => error,
		Arg::Char(c) => error.arg(strings.intern(c.encode_utf8(&mut [0; 4]))),
		Arg::Radix(radix) => error.arg(strings.intern(&radix.to_string())),
	};
	Some((error.describe(&strings), e.pos))
}

fn error_in(src: &str, strict: bool) -> (String, u32) {
	let mut lexer = lexer(src);
	lexer.strict = strict;
	loop {
		let token = lexer.next(false);
		if let Some(error) = lexed(&mut lexer) {
			return error;
		}
		assert_ne!(token.kind, Eof, "no error for {src:?}");
	}
}

fn error(src: &str) -> (String, u32) {
	error_in(src, false)
}

fn strict_error(src: &str) -> (String, u32) {
	error_in(src, true)
}

/// The one expression of `src` as the parser reads it.
fn expression(src: &str) -> (Ast, NodeId) {
	let (ast, parsed) = crate::parse_at(src, 0, Entry::Expression, Options::default(), "");
	let (list, _) = parsed.unwrap_or_else(|e| panic!("{src:?}: {}", e.describe(&ast.strings)));
	let id = ast.list(list)[0].unwrap();
	(ast, id)
}

/// The error of the expression `src`, described, where it starts and ends.
fn expression_error(src: &str) -> Result<(), (String, u32, u32)> {
	let (ast, parsed) = crate::parse_at(src, 0, Entry::Expression, Options::default(), "");
	parsed.map(|_| ()).map_err(|e| (e.describe(&ast.strings), e.pos, e.end))
}

fn number(src: &str) -> f64 {
	let (ast, id) = expression(src);
	match ast.node(id).kind {
		NodeKind::NumberLiteral { value } => ast.numbers[value as usize],
		kind => panic!("{kind:?}"),
	}
}

fn string_of(src: &str) -> (Ast, crate::StrId) {
	let (ast, id) = expression(src);
	match ast.node(id).kind {
		NodeKind::StringLiteral { value } => (ast, value),
		kind => panic!("{kind:?}"),
	}
}

fn string(src: &str) -> String {
	let (ast, value) = string_of(src);
	ast.strings.get(value).to_owned()
}

fn marked(src: &str) -> Vec<(u32, u16)> {
	let (ast, value) = string_of(src);
	ast.strings.marks_of(value).iter().map(|m| (m.at, m.unit as u16)).collect()
}

/// A name and whether it was written with an escape.
fn ident(src: &str) -> (String, bool) {
	let token = tokens(src)[0];
	assert_eq!(token.kind, Ident);
	let (ast, id) = expression(src);
	let NodeKind::Identifier { name } = ast.node(id).kind else {
		panic!("{:?}", ast.node(id).kind)
	};
	(ast.strings.get(name).to_owned(), token.flags & ESCAPED != 0)
}

/// The chunks of the template `src`, cooked and raw.
fn chunks(src: &str) -> Vec<(Option<String>, String)> {
	let (ast, id) = expression(src);
	let quasi = match ast.node(id).kind {
		NodeKind::TaggedTemplateExpression { quasi, .. } => quasi,
		_ => id,
	};
	let NodeKind::TemplateLiteral { quasis, .. } = ast.node(quasi).kind else {
		panic!("{:?}", ast.node(quasi).kind)
	};
	ast.list(quasis)
		.iter()
		.map(|q| match ast.node(q.unwrap()).kind {
			NodeKind::TemplateElement { cooked, raw, .. } => {
				(cooked.map(|c| ast.strings.get(c).to_owned()), ast.strings.get(raw).to_owned())
			}
			kind => panic!("{kind:?}"),
		})
		.collect()
}

#[test]
fn punctuators() {
	assert_eq!(
		kinds("{ } ( ) [ ] ; , : ~"),
		[BraceL, BraceR, ParenL, ParenR, BracketL, BracketR, Semi, Comma, Colon, Tilde]
	);
	assert_eq!(
		kinds(". ... ?. ?? ??= ? :"),
		[Dot, Ellipsis, QuestionDot, QuestionQuestion, QuestionQuestionEq, Question, Colon]
	);
	assert_eq!(kinds("= == === => != !== !"), [Eq, EqEq, EqEqEq, Arrow, BangEq, BangEqEq, Bang]);
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
		[Amp, AmpAmp, AmpEq, AmpAmpEq, Pipe, PipePipe, PipeEq, PipePipeEq, Caret, CaretEq]
	);
	assert_eq!(error("x@y"), ("unexpected_character '@'".into(), 1));
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
	assert_eq!(kinds("let x = await y"), [Ident, Ident, Eq, Ident, Ident]);
	assert_eq!(kinds("if else while function class"), [If, Else, While, Function, Class]);
	assert!(kinds("$foo _bar a1 ünïcödé 変数").iter().all(|k| *k == Ident));
	assert_eq!(texts("a\u{200d}b"), ["a\u{200d}b"]);
	assert_eq!(ident("hello"), ("hello".to_owned(), false));
	assert!(kinds("breaks constant iffy").iter().all(|k| *k == Ident));
}

#[test]
fn names_are_interned_once() {
	let (ast, _) = expression("a + b + a");
	assert_eq!(ast.strings.len(), 2);
}

#[test]
fn escaped_identifiers() {
	assert_eq!(ident("\\u0061bc"), ("abc".to_owned(), true));
	assert_eq!(ident("\\u{62}c"), ("bc".to_owned(), true));
	let token = tokens("\\u0069f")[0];
	assert_eq!((token.kind, token.flags & ESCAPED != 0), (If, true));
	assert_eq!(error("\\u0031a"), ("invalid_unicode_escape".into(), 0));
	assert_eq!(error("\\ud801\\udc00"), ("invalid_unicode_escape".into(), 0));
	assert_eq!(error("a\\x"), ("expected_unicode_escape".into(), 2));
}

#[test]
fn private_names() {
	assert_eq!(kinds("#foo #class"), [PrivateName, PrivateName]);
	assert_eq!(texts("#foo"), ["#foo"]);
	assert_eq!(error("# a"), ("unexpected_character ' '".into(), 1));
}

#[test]
fn hashbang() {
	assert_eq!(texts("#!/usr/bin/env node\nfoo"), ["foo"]);
	let mut lexer = lexer("#!/x\ny");
	lexer.next(false);
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
	assert_eq!(kinds("0x1na"), [BigInt, Ident]);
	assert_eq!(kinds("1\\u0061"), [Number, Ident]);
}

#[test]
fn legacy_octal_numbers() {
	assert_eq!(number("017"), 15.0);
	assert_eq!(number("08.5"), 8.5);
	assert_eq!(number("0"), 0.0);
	assert_eq!(number("0374547736741752762421"), 4552292526566663700.0);
	assert_eq!(error("017n").0, "identifier_after_number");
	assert_eq!(error("01_7").0, "numeric_separator_legacy_octal");
	assert_eq!(error("0_1"), ("numeric_separator_legacy_octal".into(), 1));
	assert_eq!(strict_error("017"), ("invalid_number".into(), 0));
	assert_eq!(strict_error("06B"), ("invalid_number".into(), 0));
	assert_eq!(strict_error("x = 08"), ("invalid_number".into(), 4));
}

#[test]
fn number_errors() {
	assert_eq!(error("1a"), ("identifier_after_number".into(), 1));
	assert_eq!(error("1__0").0, "numeric_separator_double");
	assert_eq!(error("1_").0, "numeric_separator_last");
	assert_eq!(error("0x_1").0, "numeric_separator_first");
	assert_eq!(error("0x"), ("expected_number_in_radix '16'".into(), 2));
	assert_eq!(error("1e").0, "invalid_number");
	assert_eq!(error("1.5n").0, "identifier_after_number");
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
	assert_eq!(marked("'\\ud83dx'"), vec![(0, 0xd83d)]);
	assert_eq!(marked("'\\ude00'"), vec![(0, 0xde00)]);
	assert_eq!(marked("'\\u{de00}'"), vec![(0, 0xde00)]);
	assert_eq!(marked("'😀\\ud83d\\ud83d\\ude00\\udbff'"), vec![(2, 0xd83d), (5, 0xdbff)]);
	assert_eq!(marked("'\\ud83d\\ude00'"), vec![]);
	assert_eq!(marked("'\\ufffd'"), vec![]);
	{
		let (ast, id) = expression("['\\ud83d', '\\ufffd', '\\udbff']");
		let NodeKind::ArrayExpression { elements } = ast.node(id).kind else { panic!() };
		let ids: Vec<_> = ast
			.list(elements)
			.iter()
			.map(|e| match ast.node(e.unwrap()).kind {
				NodeKind::StringLiteral { value } => value,
				kind => panic!("{kind:?}"),
			})
			.collect();
		assert_ne!(ids[0], ids[1]);
		assert_ne!(ids[0], ids[2]);
	}
	assert_eq!(string("'a\\\nb'"), "ab");
	assert_eq!(string("'a\\\r\nb'"), "ab");
	assert_eq!(string("'\\0'"), "\0");
	assert_eq!(string("'\\q'"), "q");
	assert_eq!(string("'\u{2028}'"), "\u{2028}");
	assert_eq!(error("'abc").0, "unterminated_string");
	assert_eq!(error("'a\nb'").0, "unterminated_string");
	assert_eq!(error("'\\"), ("unterminated_string".into(), 0));
	assert_eq!(error("'\\x4'"), ("bad_character_escape".into(), 3));
	assert_eq!(error("'\\u{}'"), ("bad_character_escape".into(), 4));
	assert_eq!(error("'\\u{110000}'"), ("code_point_out_of_bounds".into(), 4));
}

#[test]
fn legacy_octal_escapes() {
	assert_eq!(string("'\\101'"), "A");
	assert_eq!(string("'\\08'"), "\08");
	assert_eq!(string("'\\8'"), "8");
	assert_eq!(string("'\\0a'"), "\0a");
	assert_eq!(strict_error("x = '\\101'"), ("strict_octal".into(), 5));
	assert_eq!(strict_error("'\\8'"), ("strict_escape".into(), 2));
	assert_eq!(strict_error("'\\08'").0, "strict_octal");
	let mut lexer = lexer("'\\0'");
	lexer.strict = true;
	assert_eq!(lexer.next(false).kind, Kind::String);
	assert!(lexer.error.is_none());
}

#[test]
fn templates() {
	let src = "`a${b}c\\n`";
	let mut lexer = lexer(src);
	let head = lexer.next(false);
	assert_eq!((head.kind, head.start, head.end), (TemplateHead, 0, 4));
	assert_eq!(lexer.next(false).kind, Ident);
	let brace = lexer.next(false);
	assert_eq!(brace.kind, BraceR);
	let tail = lexer.template_continue(brace);
	assert_eq!((tail.kind, tail.start, tail.end), (TemplateTail, 5, 10));
	assert_eq!(lexer.next(false).kind, Eof);
	assert_eq!(
		chunks(src),
		[(Some("a".into()), "a".into()), (Some("c\n".into()), "c\\n".into())]
	);
}

#[test]
fn template_invalid_escapes_cook_to_nothing() {
	for src in ["x`\\unicode`", "x`\\1`", "x`\\x`", "x`\\8`"] {
		assert_eq!(chunks(src)[0].0, None, "{src}");
	}
}

#[test]
fn template_newlines_normalise() {
	assert_eq!(chunks("`a\r\nb\rc`"), [(Some("a\nb\nc".into()), "a\nb\nc".into())]);
	assert_eq!(chunks("`a\\\r\nb\\\rc`"), [(Some("abc".into()), "a\\\nb\\\nc".into())]);
	assert_eq!(expression_error("`abc"), Err(("unterminated_template".into(), 1, 1)));
	assert_eq!(expression_error("`"), Err(("unterminated_template".into(), 1, 1)));
}

#[test]
fn regex() {
	let mut lexer = lexer("/a[/]b\\/c/gi;");
	let re = lexer.next(true);
	assert_eq!((re.kind, re.start, re.end), (RegExp, 0, 12));
	assert_eq!(lexer.next(false).kind, Semi);
	let (ast, id) = expression("/a[/]b\\/c/gi");
	let NodeKind::RegExpLiteral { pattern, flags } = ast.node(id).kind else { panic!() };
	assert_eq!((ast.strings.get(pattern), ast.strings.get(flags)), ("a[/]b\\/c", "gi"));

	let mut lexer = self::lexer("/=x/");
	assert_eq!(lexer.next(false).kind, SlashEq);
	let mut lexer = self::lexer("/=x/");
	let t = lexer.next(true);
	assert_eq!((t.kind, t.end), (RegExp, 4));

	for (src, error, at) in [
		("/abc\n/", "unterminated_regexp", 1),
		("/a/\\u0067", "unexpected_token", 3),
		("/a/\\u{30}", "invalid_unicode_escape", 3),
		("/a/\\ux", "bad_character_escape", 5),
	] {
		// read as a regular expression where the grammar asks for one: read early, one that does not
		// lex is the `/` operator until then
		let mut lexer = self::lexer(src);
		let slash = lexer.next(false);
		lexer.regex(slash);
		assert_eq!(lexed(&mut lexer), Some((error.into(), at)), "{src:?}");
	}
}

#[test]
fn regex_validation() {
	let regex = expression_error;
	let nested = |open: &str, close: &str, flags: &str| format!("/{}{}/{flags}", open.repeat(5000), close.repeat(5000));
	for valid in [
		r"/(?<a>x)|(?<a>y)/",
		r"/(?:(?<a>x)|(?<a>y))\k<a>/",
		r"/(?<a>x)|(?:y|(?<a>z))/",
		r"/(?<𝒜>.)\k<𝒜>(?<\u{1d49e}>.)(?<\ud835\udc9f>.)/",
		r"/(?i:a)(?i-m:b)(?-s:c)(?s-:d)/",
		r"/\1(a)\k<b>[\k]\c[\c][\c1-\c_]\8\p{L}/",
		r"/(a)\1\0/u",
		r"/{*a{b{1c{,2}]}x{1}{/",
		r"/(?=a)*(?!a){2}/",
		r"/a{2,2}b{0002,2}c{1,}?/",
		r"/[a-😀][\d-a][a-\d][---][\b-\t]/",
		r"/[😀-😁][\ud83d\ude00-\ud83d\ude01][\--\/]/u",
		r"/\p{Script=Greek}\p{scx=Grek}\P{Lu}\p{gc=L}\p{Any}/u",
		r"/[\p{L}--[a-z]][\p{L}&&\p{Lu}&&[^a]][\q{ab|c}a-z\d][^\q{a|b}][\p{RGI_Emoji}--\q{x}][^\q{ab}&&a][]/v",
		"/a/gimsyd",
		"/a/dv",
		&nested("(", ")", ""),
		&nested("(?:a|", ")", "u"),
		&nested("[", "]", "v"),
	] {
		assert_eq!(regex(valid), Ok(()), "{}", &valid[..valid.len().min(80)]);
	}
	for (invalid, error, at, end) in [
		(r"/(/", "unterminated_regexp_group", 1, 2),
		(r"/a)/", "unopened_regexp_group", 2, 3),
		(r"/a**/", "lone_regexp_quantifier", 3, 4),
		(r"/^*/", "lone_regexp_quantifier", 2, 3),
		(r"/a|*/", "lone_regexp_quantifier", 3, 4),
		(r"/(?<=a)*/", "lone_regexp_quantifier", 7, 8),
		(r"/(?=a)*/u", "lone_regexp_quantifier", 6, 7),
		(r"/{1}/", "lone_regexp_quantifier", 1, 4),
		(r"/a{2,1}/", "regexp_quantifier_order", 2, 7),
		(
			r"/a{9007199254740993,9007199254740992}/",
			"regexp_quantifier_order",
			2,
			37,
		),
		(r"/{*/u", "lone_regexp_bracket", 1, 2),
		(r"/a}/u", "lone_regexp_bracket", 2, 3),
		(r"/a{1/u", "lone_regexp_bracket", 2, 3),
		(r"/\1/u", "undefined_regexp_group", 2, 3),
		(r"/(a)\2/u", "undefined_regexp_group", 5, 6),
		(r"/a\00/u", "invalid_regexp_escape", 2, 4),
		(r"/a\-/u", "invalid_regexp_escape", 2, 4),
		(r"/a\x4/u", "invalid_regexp_escape", 2, 4),
		(r"/a[\1]/u", "invalid_regexp_escape", 3, 5),
		(r"/a\c/u", "invalid_regexp_escape", 2, 4),
		(r"/a\u{110000}/u", "invalid_regexp_escape", 2, 11),
		(r"/[b-a]/", "regexp_range_order", 2, 5),
		(r"/[😀-😁]/", "regexp_range_order", 2, 11),
		(r"/[\ud83d\ude00-\ud83d\ude01]/", "regexp_range_order", 8, 21),
		(r"/x[a-\d]/u", "invalid_regexp_class", 3, 7),
		(r"/(?<a>x)(?<a>y)/", "duplicate_regexp_group_name", 11, 12),
		(r"/(?<a>x)(?<\u0061>y)/", "duplicate_regexp_group_name", 11, 17),
		(r"/(?:(?<a>x))(?<a>z)/", "duplicate_regexp_group_name", 15, 16),
		(r"/(?<a>(?<a>x))/", "duplicate_regexp_group_name", 9, 10),
		(
			r"/(?<a>.)|(?<b>.)(?:(?<c>.)|(?<b>.))/",
			"duplicate_regexp_group_name",
			30,
			31,
		),
		(r"/(?<a>x)[\k]/", "invalid_regexp_escape", 9, 11),
		(r"/\k<a>(?<b>x)/", "undefined_regexp_group", 4, 5),
		(r"/a\k<a>/u", "undefined_regexp_group", 5, 6),
		(r"/(?<a>x)\k/", "invalid_regexp_group_name", 8, 10),
		(r"/(?<1>a)/", "invalid_regexp_group_name", 1, 5),
		(r"/(?<a/", "invalid_regexp_group_name", 1, 5),
		(r"/a(?x:a)/", "invalid_regexp_group", 2, 4),
		(r"/(?ii:a)/", "duplicate_regexp_modifier", 4, 5),
		(r"/(?i-i:a)/", "duplicate_regexp_modifier", 5, 6),
		(r"/(?-:a)/", "invalid_regexp_group", 1, 5),
		(r"/\p{Nope}/u", "invalid_regexp_property", 1, 8),
		(r"/a\P{RGI_Emoji}/v", "invalid_regexp_property", 2, 15),
		(r"/\p{sc=Nope}/u", "invalid_regexp_property", 1, 11),
		(r"/[^\q{ab}]/v", "invalid_regexp_class", 1, 10),
		(r"/a[^[\q{}]]/v", "invalid_regexp_class", 2, 11),
		(r"/[a&&&b]/v", "invalid_regexp_class", 3, 5),
		(r"/[a-z&&b]/v", "invalid_regexp_class", 5, 7),
		(r"/[a--b&&c]/v", "invalid_regexp_class", 6, 8),
		(r"/[a&&]/v", "invalid_regexp_class", 5, 6),
		(r"/[a-]/v", "invalid_regexp_class", 3, 4),
		(r"/[a!!]/v", "invalid_regexp_class", 3, 4),
		(r"/[a(]/v", "invalid_regexp_class", 3, 4),
		(r"/[z-a]/v", "regexp_range_order", 2, 5),
		(r"/[a\q]/v", "invalid_regexp_escape", 3, 5),
	] {
		assert_eq!(regex(invalid), Err((error.into(), at, end)), "{invalid}");
	}
	assert_eq!(regex("/a/gg"), Err(("duplicate_regexp_flag".into(), 4, 5)));
	assert_eq!(regex("/a/guv"), Err(("invalid_regexp_flag".into(), 5, 6)));
	assert_eq!(regex("/a/gx"), Err(("invalid_regexp_flag".into(), 4, 5)));
}

#[test]
fn comments_are_collected_and_skipped() {
	let src = "a // one\n/* two\n */ b";
	let mut lexer = lexer(src);
	assert_eq!(lexer.next(false).kind, Ident);
	let b = lexer.next(false);
	assert_eq!(b.kind, Ident);
	assert!(b.flags & NEWLINE_BEFORE != 0);
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
	assert_eq!(&src[lexer.comments[1].text_range()], " two\n ");
	assert_eq!(error("/* x").0, "unterminated_comment");
}

#[test]
fn html_comments_outside_modules() {
	assert_eq!(texts("a <!-- b\n+c"), ["a", "+", "c"]);
	assert_eq!(texts("--> x\ny"), ["y"]);
	assert_eq!(texts("a = b-->1;\n --> nothing"), ["a", "=", "b", "--", ">", "1", ";"]);
	assert_eq!(texts("/* c */--> x\ny"), ["y"]);
	let mut lexer = lexer("<!-- a\nb");
	lexer.next(false);
	assert_eq!(lexer.comments[0].kind, CommentKind::HtmlOpen);
	assert_eq!(&"<!-- a\nb"[lexer.comments[0].text_range()], " a");
	let mut lexer = Lexer::new("a <!-- b", true, true);
	lexer.next(false);
	assert_eq!(lexer.next(false).kind, Lt);
}

#[test]
fn newline_before() {
	let flags: Vec<bool> = tokens("a\nb c\u{2028}d").iter().map(|t| t.flags & NEWLINE_BEFORE != 0).collect();
	assert_eq!(flags, [false, true, false, true]);
}

#[test]
fn unicode_whitespace() {
	assert_eq!(texts("a\u{a0}b\u{feff}c\u{3000}d"), ["a", "b", "c", "d"]);
}

#[test]
fn unexpected_characters() {
	assert_eq!(error("a ¬ b").0, "unexpected_character '¬'");
	assert_eq!(error("\\").0, "expected_unicode_escape");
}

#[test]
fn every_keyword_reads_back() {
	let words = [
		"break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "export",
		"extends", "false", "finally", "for", "function", "if", "import", "in", "instanceof", "new", "null", "return",
		"super", "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while", "with",
	];
	for word in words {
		let kind = super::keyword_spelled(word.as_bytes());
		assert!(kind.is_keyword(), "{word}");
		assert_eq!(kinds(word), [kind], "{word}");
		assert_ne!(super::word::flags(word) & super::word::KEYWORD, 0, "{word}");
	}
	assert_eq!(super::keyword_spelled(b"brea"), Ident);
	assert_eq!(super::keyword_spelled(b"breaks"), Ident);
}

/// A list in parentheses read as an arrow's parameters or as an expression, as its tokens say.
#[test]
fn a_list_is_read_as_its_tokens_decide() {
	let read_as = |src: &str, options: Options| {
		let (ast, parsed) = crate::parse_at(src, 0, Entry::Program, options, "");
		let Ok((list, _)) = parsed else { return "error" };
		let NodeKind::Program { body, .. } = ast.node(ast.list(list)[0].unwrap()).kind else { panic!() };
		let NodeKind::ExpressionStatement { expression, .. } = ast.node(ast.list(body)[0].unwrap()).kind else {
			panic!()
		};
		match ast.node(expression).kind {
			NodeKind::ArrowFunctionExpression { .. } => "arrow",
			_ => "expression",
		}
	};
	let read = |src: &str, typescript: bool| {
		read_as(src, Options(Options::MODULE | if typescript { Options::TYPESCRIPT } else { 0 }))
	};
	let js = |src: &str| read(src, false);
	assert_eq!(js("(a) => a"), "arrow");
	assert_eq!(js("(a) + 1"), "expression");
	assert_eq!(js("(a)\n=> a"), "error");
	assert_eq!(js("(...a) + 1"), "error");
	assert_eq!(js("(a, b)\n=> 1"), "error");
	assert_eq!(js("(1, b) => 1"), "error");
	assert_eq!(read("(a, b): T => 1", true), "arrow");
	assert_eq!(read("(a: T, b) => 1", true), "arrow");
	// strings, comments, regular expressions and templates hold brackets that close nothing
	assert_eq!(js("(a = \")\", b = '(') => 1"), "arrow");
	assert_eq!(js("(a /* ) */, b // )\n) => 1"), "arrow");
	assert_eq!(js("(a = /[)]/g, b = c / d / e) => 1"), "arrow");
	assert_eq!(js("(a = `${(b) => `${c})`}`, d) => 1"), "arrow");
	assert_eq!(js("(a = `${{ b: ')' }.b}`, c) + 1"), "expression");
	// a `/` after a statement head's `)` starts a regular expression
	assert_eq!(js("(a = function () { if (x) /[)]/.test(y) }, b) => 1"), "arrow");
	assert_eq!(js("(a = b.if (c) / d, e) => 1"), "arrow");
	assert_eq!(js("(a = async function () { for await (x of y) /[)]/.test(z) }, b) => 1"), "arrow");
	// after any brace's `}` only the parser knows: a block, a body or an object
	assert_eq!(js("(a = function () {} / 1, b) => 1"), "arrow");
	assert_eq!(js("(a = function () { {} /[)]/.test(y) }, b) => 1"), "arrow");
	assert_eq!(js("(a = class extends {} {} / (b, \")\" / c)) => 1"), "arrow");
	assert_eq!(js("(a = of / 2, b) => 1"), "arrow");
	// a keyword after `.` or `?.` is a property name, so the `/` after it divides
	assert_eq!(js("(a = b.default / 2, c = \"/\") => 1"), "arrow");
	assert_eq!(js("(a = b?.delete / 2, c = \"/\") => 1"), "arrow");
	assert_eq!(js("(a = void /[)]/.b, c) => 1"), "arrow");
	assert_eq!(read("(a = b.default! / 2, c = \"/\") => 1", true), "arrow");
	// `void`, `const` and `>` end a type or start an operand
	assert_eq!(read("(a = b as void / 2, c) => 1", true), "arrow");
	assert_eq!(read("(a = [1] as const / 2, c) => 1", true), "arrow");
	assert_eq!(read("(a = b as A<B> / 2, c) => 1", true), "arrow");
	assert_eq!(read("(a = b as A<B<C>> / 2, c) => 1", true), "arrow");
	// a postfix `++` ends an operand, a prefix one starts one
	assert_eq!(js("(a = x++ / 2, b) => 1"), "arrow");
	assert_eq!(js("(a = ++/[)]/.b, c) => 1"), "arrow");
	assert_eq!(read("(a = x! / 2, b) => 1", true), "arrow");
	assert_eq!(js("(x, (a, b) => 1, [(c, d)]) + 1"), "expression");
	// a script's HTML comments
	assert_eq!(read_as("(a, b <!-- )\n) => 1", Options::default()), "arrow");
	assert_eq!(read_as("(a, b\n--> )\n) => 1", Options::default()), "arrow");
}
