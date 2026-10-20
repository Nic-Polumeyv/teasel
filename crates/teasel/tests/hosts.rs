//! A document for each rule of the host grammars beside them, its answer pinned beside it as
//! `NAME.json`: the tree with comments and scopes, the error when it has one. A name says what
//! else is on: `locations`, `erase`, or `recover` for `errorRecovery`. `UPDATE=1` rewrites the
//! pins once a change is meant.

mod common;

use std::fs;
use std::path::Path;
use teasel::Code;
use teasel::Entry;
use teasel::Options;
use teasel::host::grammar::definition::{self, Bind, Host, Node, Source};
use teasel::host::grammar::{
	Arena, Construct, DirectiveValue, DocField, Grammar, Item, Match, Place, Record, RootField, Unique,
};
use teasel::json::parse_document;

/// A list of a definition, kept for the process.
macro_rules! list {
	($($item:expr),* $(,)?) => {{
		let items: &'static [_] = Vec::leak(vec![$($item),*]);
		items
	}};
}

/// A batch job is four numbers and its source; a fifth word, an entry that is none or a document
/// without a grammar stops the batch after the jobs before it answered.
#[test]
fn batch_headers() {
	use std::io::Write;
	use std::process::{Command, Stdio};
	let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hosts/svelte/host.json");
	let batch = |input: &str, grammar: bool| {
		let mut command = Command::new(env!("CARGO_BIN_EXE_teasel"));
		command.arg("--batch");
		if grammar {
			command.arg("--host").arg(&host);
		}
		let mut child = command
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
		let output = child.wait_with_output().unwrap();
		let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
		(
			output.status.success(),
			text(output.stdout).lines().count(),
			text(output.stderr),
		)
	};
	let (ok, answers, _) = batch("0 0 1 2\na;7 0 1 7\n<a></a>", true);
	assert!(ok && answers == 2);
	for (input, grammar, error) in [
		("1 0 1 1\na0 0 1 2 5\na;", false, "malformed header"),
		("1 0 1 1\na8 0 1 2\na;", true, "entry 8 needs --host or is no entry"),
		("1 0 1 1\na7 0 1 2\na;", false, "entry 7 needs --host or is no entry"),
	] {
		let (ok, answers, stderr) = batch(input, grammar);
		assert!(!ok && answers == 1 && stderr.contains(error), "{input}: {stderr}");
	}
}

#[test]
fn documents() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut wrong = Vec::new();
	let mut grammars: Vec<_> = fs::read_dir(root.join("tests/hosts"))
		.unwrap()
		.map(|e| e.unwrap().path())
		.filter(|p| p.is_dir())
		.collect();
	grammars.sort();
	for dir in grammars {
		let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
		let grammar = fs::read_to_string(root.join("tests/hosts").join(&name).join("host.json")).unwrap();
		for file in common::inputs(&dir) {
			let stem = file.file_stem().unwrap().to_str().unwrap();
			let source = fs::read_to_string(&file).unwrap();
			let mut flags = Options::MODULE | Options::COMMENTS | Options::SCOPES;
			for (word, bit) in [
				("locations", Options::LOCATIONS),
				("erase", Options::ERASE),
				("recover", Options::ERROR_RECOVERY),
			] {
				if stem.contains(word) {
					flags |= bit;
				}
			}
			let answer = common::pretty(&parse_document(&source, &grammar, Options(flags)));
			if !common::pinned(&file.with_extension("json"), &answer) {
				wrong.push(format!("{name}/{stem}"));
			}
		}
	}
	assert!(
		wrong.is_empty(),
		"answers changed for:\n{}\nrun with UPDATE=1 once the change is meant",
		wrong.join("\n")
	);
}

/// Every prefix of every host document, strict and recovering: unfinished input is an answer or
/// an error, never a panic.
#[test]
fn every_prefix_answers() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	for name in ["svelte", "vue"] {
		let grammar = fs::read_to_string(root.join("tests/hosts").join(name).join("host.json")).unwrap();
		for file in common::inputs(&root.join("tests/hosts").join(name)) {
			let source = fs::read_to_string(&file).unwrap();
			for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
				for recover in [false, true] {
					let recovery = if recover { Options::ERROR_RECOVERY } else { 0 };
					let options = Options(Options::MODULE | Options::COMMENTS | Options::SCOPES | recovery);
					parse_document(&source[..end], &grammar, options);
				}
			}
		}
	}
}

/// The unfinished and odd inputs the audit found panicking, and the fallback from one expression
/// to statements, which must not settle for what recovery makes of the expression.
#[test]
fn unfinished_input() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let svelte = fs::read_to_string(root.join("tests/hosts/svelte/host.json")).unwrap();
	let vue = fs::read_to_string(root.join("tests/hosts/vue/host.json")).unwrap();
	let parse = |source: &str, grammar: &str, recover: bool| {
		let recovery = if recover { Options::ERROR_RECOVERY } else { 0 };
		parse_document(source, grammar, Options(Options::MODULE | Options::COMMENTS | recovery))
	};
	assert!(parse("<a x=\"", &svelte, true).contains("\"type\":\"Root\""));
	let comment = parse("<a /*xx", &svelte, true);
	assert!(comment.contains("\"value\":\"xx\""), "{comment}");
	assert!(parse("<script>\"</script>", &svelte, true).contains("\"type\":\"Root\""));
	assert!(parse("<a @x=\"@\"/>", &vue, false).contains("\"node\":null"));
	let elements = format!("{}x{}", "<a>".repeat(40_000), "</a>".repeat(40_000));
	let branches = format!("{{#if a}}{}{{/if}}", "{:else if a}".repeat(40_000));
	for deep in [elements, branches] {
		assert!(parse(&deep, &svelte, false).contains(&format!("\"code\":{},", Code::NestingDepth as u8)));
	}
	let program = parse("<button @click=\"let x = 1\"/>", &vue, true);
	assert!(
		program.contains("\"type\":\"VariableDeclaration\"") && !program.contains("\"errors\":[{"),
		"{program}"
	);
	let program = parse("<button @click=\"if (ok) foo()\"/>", &vue, true);
	assert!(
		program.contains("\"type\":\"IfStatement\"") && !program.contains("\"errors\":[{"),
		"{program}"
	);
}

#[test]
fn tags_fill_unread() {
	let mut host = minimal();
	host.definition.constructs = Some(Record(list![(
		"T",
		tag(
			&["{", "@t"],
			list![
				definition::Item::Opt(list![definition::Item::Fields(Record(list![(
					"a",
					source("js", "expression", false),
				)]))]),
				definition::Item::Opt(list![
					definition::Item::Word(","),
					definition::Item::Fields(Record(list![("b", source("js", "expression", true))])),
				]),
				definition::Item::Word("}"),
			],
		),
	)]));
	let answer = parse_document("{@t }", &host.json(), Options(Options::MODULE));
	assert!(answer.contains("\"a\":null") && !answer.contains("\"b\""), "{answer}");
}

// cargo test --release --test hosts host_phases -- --ignored --nocapture; TEASEL_HOST_BENCH=file adds a document of its own
#[test]
#[ignore]
fn host_phases() {
	use teasel::json::Prepared;
	let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar =
		teasel::json::grammar(&fs::read_to_string(root.join("tests/hosts/svelte/host.json")).unwrap()).unwrap();
	let mut documents = vec![(
		"200 each blocks".to_string(),
		format!(
			"<script>let items = [1,2,3];</script>\n{}",
			"{#each items as item}<p class=\"row\">{item + 1}</p>{/each}\n".repeat(200)
		),
	)];
	if let Ok(path) = std::env::var("TEASEL_HOST_BENCH") {
		documents.push((path.clone(), fs::read_to_string(path).unwrap()));
	}
	for (name, source) in &documents {
		for (flags, label) in [
			(Options::MODULE, "module"),
			(
				Options::MODULE | Options::SCOPES | Options::COMMENTS | Options::LOCATIONS,
				"module scopes comments locations",
			),
		] {
			let prepared = Prepared::borrowed(source, Options(flags));
			let mut best = f64::MAX;
			for _ in 0..300 {
				let t = std::time::Instant::now();
				prepared
					.in_place(Entry::Program, 0.0, None, "", Some(&grammar))
					.unwrap();
				best = best.min(t.elapsed().as_secs_f64() * 1e6);
			}
			eprintln!("{best:9.2} µs  {name} {label}");
		}
	}
}

fn source(from: &'static str, read: &'static str, optional: bool) -> Source {
	Source {
		from,
		read,
		optional,
		bind: Bind::No,
		or_arg: false,
		literal: None,
	}
}

fn node(ty: &'static str, items: &'static [definition::Item<'static>]) -> Node<'static> {
	Node {
		node: ty,
		form: Some(items),
	}
}

fn piece(marker: &[&'static str], items: &'static [definition::Item<'static>]) -> definition::Piece<'static> {
	definition::Piece {
		marker: marker.to_vec().leak(),
		space: Some(true),
		form: items,
	}
}

fn tag(marker: &[&'static str], items: &'static [definition::Item<'static>]) -> definition::Construct<'static> {
	definition::Construct {
		r#in: None,
		open: piece(marker, items),
		branches: None,
		close: None,
	}
}

/// A block written as Svelte writes one: `{#name …}`, `{:word …}`, `{/name}`.
fn block(
	name: &'static str,
	items: &'static [definition::Item<'static>],
	branches: &'static [definition::Branch<'static>],
) -> definition::Construct<'static> {
	definition::Construct {
		r#in: None,
		open: piece(&["{", keep(format!("#{name}"))], items),
		branches: Some(branches),
		close: Some(definition::Piece {
			marker: list!["{", keep(format!("/{name}"))],
			space: None,
			form: list![definition::Item::Word("}")],
		}),
	}
}

fn branch(
	marker: &[&'static str],
	items: &'static [definition::Item<'static>],
	reopen: Option<(&'static str, &'static str)>,
) -> definition::Branch<'static> {
	definition::Branch {
		marker: marker.to_vec().leak(),
		space: None,
		form: items,
		reopen,
	}
}

fn keep(s: String) -> &'static str {
	Box::leak(s.into_boxed_str())
}

fn construct<'g>(grammar: &'g Grammar, ty: &str) -> &'g Construct {
	grammar.constructs.iter().find(|c| c.ty == ty).unwrap()
}

fn element(ty: &'static str) -> definition::Element<'static> {
	definition::Element {
		node: ty,
		form: None,
		root: None,
		once: None,
		inside: None,
		outside: None,
		content: None,
	}
}

/// `{expression}` in content and in attribute values.
fn expression(field: &'static str) -> definition::Construct<'static> {
	definition::Construct {
		r#in: Some(list![Place::Content, Place::Value]),
		open: definition::Piece {
			marker: list!["{"],
			space: None,
			form: list![
				definition::Item::Fields(Record(list![(field, source("js", "expression", false))])),
				definition::Item::Word("}"),
			],
		},
		branches: None,
		close: None,
	}
}

/// A definition with nothing but what every grammar needs.
fn minimal() -> Host<'static> {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	Host {
		name: "t",
		definition: definition::Definition {
			document: node(
				"Document",
				list![fields(list![("children", source("content", "fragment", false))])],
			),
			text: node("Text", list![fields(list![("data", source("text", "data", false))])]),
			comment: node("Comment", list![fields(list![("data", source("text", "data", false))])]),
			fragment: None,
			attributes: None,
			autoclose: None,
			trim: None,
			void: None,
			verbatim: None,
			elements: definition::Elements {
				fields: Record(list![
					("name", source("element", "name", false)),
					("attributes", source("element", "attributes", false)),
					("children", source("content", "fragment", false)),
				]),
				rules: None,
				component: None,
				other: None,
			},
			script: None,
			style: None,
			directives: None,
			constructs: None,
		},
	}
}

#[test]
fn a_body_declares_only_what_the_form_reads() {
	let each = |context: &'static str| {
		let mut host = minimal();
		let bound = Source {
			bind: Bind::Inside,
			..source("js", "pattern", false)
		};
		host.definition.constructs = Some(Record(list![(
			"EachBlock",
			block(
				"each",
				list![
					definition::Item::Fields(Record(list![("expression", source("js", "expression", false))])),
					definition::Item::Word("as"),
					definition::Item::Fields(Record(list![(context, bound)])),
					definition::Item::Word("}"),
					definition::Item::Fields(Record(list![("body", source("content", "fragment", false))])),
				],
				&[],
			),
		)]));
		keep(host.json())
	};
	let grammar = Grammar::read(each("context")).unwrap();
	assert_eq!(
		construct(&grammar, "EachBlock")
			.open
			.form
			.body
			.as_ref()
			.unwrap()
			.declares[0]
			.field,
		"context"
	);
	assert!(Grammar::read("").is_err() && Grammar::read(&each("context")[..40]).is_err());
	let mut host = minimal();
	let mut form = host.definition.document.form.unwrap().to_vec();
	form.push(definition::Item::Word("x"));
	host.definition.document.form = Some(form.leak());
	assert!(Grammar::read(keep(host.json())).unwrap_err().contains("document"));
}

// what a definition may not say is an error naming its field, never a panic or a silent drop
#[test]
fn a_node_type_of_javascript_is_refused() {
	let mut host = minimal();
	host.definition.text.node = "Identifier";
	let error = Grammar::read(keep(host.json())).unwrap_err();
	assert!(error.contains("Identifier") && error.contains("JavaScript"), "{error}");
	let mut host = minimal();
	host.definition.text.node = "TSAnyKeyword";
	assert!(Grammar::read(keep(host.json())).is_err());
}

#[test]
fn a_definition_is_refused_by_name() {
	let with_if = |items: &'static [definition::Item<'static>], branches: &'static [definition::Branch<'static>]| {
		let mut host = minimal();
		host.definition.constructs = Some(Record(list![("IfBlock", block("if", items, branches))]));
		keep(host.json())
	};
	let bound = Source {
		bind: Bind::Inside,
		..source("js", "pattern", false)
	};
	let test = definition::Item::Fields(Record(list![("test", bound)]));
	let consequent = definition::Item::Fields(Record(list![("consequent", source("content", "fragment", false))]));
	let close = definition::Item::Word("}");
	let error = Grammar::read(with_if(
		list![test, close, consequent],
		list![branch(
			&["{", ":else", "if"],
			list![test, close],
			Some(("alternate", "elseif")),
		)],
	))
	.unwrap_err();
	assert!(error.contains("reopens the block"), "{error}");
	let error = Grammar::read(with_if(list![test, close], &[])).unwrap_err();
	assert!(error.contains("IfBlock ends in no body"), "{error}");
	let grammar = Grammar::read(with_if(
		list![test, close, consequent],
		list![branch(
			&["{", ":else", "if"],
			list![test, close, consequent],
			Some(("alternate", "elseif")),
		)],
	))
	.unwrap();
	assert_eq!(
		construct(&grammar, "IfBlock").branches[0]
			.form
			.body
			.as_ref()
			.unwrap()
			.declares[0]
			.field,
		"test"
	);
	let mut host = minimal();
	host.definition.directives = Some(definition::Directives {
		prefix: None,
		arg: None,
		modifier: None,
		dynamic: None,
		unique: None,
		fields: Record(&[]),
		shorthands: None,
		rules: Some(Record(list![(
			"on",
			definition::Directive {
				node: "OnDirective",
				form: Some(list![definition::Item::Fields(Record(list![(
					"modifiers",
					Source {
						literal: Some(definition::Literal::List),
						..source("literal", "literal", false)
					},
				)]))]),
				unique: None,
			},
		)])),
		other: None,
	});
	let error = Grammar::read(keep(host.json())).unwrap_err();
	assert!(error.contains("OnDirective: modifiers is a flag"), "{error}");
	let mut host = minimal();
	host.definition.document.form = Some(list![definition::Item::Fields(Record(list![(
		"js",
		Source {
			literal: Some(definition::Literal::True),
			..source("literal", "literal", false)
		},
	)]))]);
	let error = Grammar::read(keep(host.json())).unwrap_err();
	assert!(error.contains("js on the document"), "{error}");
	let whole = keep(minimal().json());
	assert!(
		Grammar::read(&whole[..whole.len() - 1])
			.unwrap_err()
			.contains("ends early")
	);
}

// how a definition groups its fields into records never changes what it reads
#[test]
fn records_group_freely() {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	let parse = |host: &Host, text: &str| parse_document(text, &host.json(), Options(Options::MODULE));

	let mut host = minimal();
	let open = list![
		fields(list![("test", source("js", "expression", false))]),
		definition::Item::Word("}"),
		fields(list![("consequent", source("content", "fragment", false))]),
	];
	host.definition.constructs = Some(Record(list![(
		"IfBlock",
		block(
			"if",
			open,
			list![branch(&["{", ":else", "if"], open, Some(("alternate", "elseif")))],
		),
	)]));
	let answer = parse(&host, "{#if a}x{:else if b}y{/if}");
	assert!(
		!answer.contains("\"error\"") && answer.contains("\"elseif\":true") && answer.contains("\"name\":\"b\""),
		"{answer}"
	);

	let mut host = minimal();
	host.definition.directives = Some(definition::Directives {
		prefix: None,
		arg: None,
		modifier: None,
		dynamic: None,
		unique: None,
		fields: Record(list![("name", source("directive", "arg", false))]),
		shorthands: None,
		rules: Some(Record(list![(
			"transition",
			definition::Directive {
				node: "TransitionDirective",
				form: Some(list![fields(list![
					("expression", source("value", "expression", false)),
					(
						"intro",
						Source {
							literal: Some(definition::Literal::True),
							..source("literal", "literal", false)
						},
					),
				])]),
				unique: None,
			},
		)])),
		other: None,
	});
	host.definition.elements.other = Some(element("RegularElement"));
	host.definition.constructs = Some(Record(list![("ExpressionTag", expression("expression"))]));
	let answer = parse(&host, "<a transition:fade={params}/>");
	assert!(
		!answer.contains("\"error\"") && answer.contains("\"name\":\"params\"") && answer.contains("\"intro\":true"),
		"{answer}"
	);
}

// a branch of another block in a block that has none names the block it stands in
#[test]
fn a_branch_names_a_block_without_branches() {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	let mut host = minimal();
	host.definition.constructs = Some(Record(list![
		(
			"RepeatBlock",
			block(
				"repeat",
				list![
					fields(list![("list", source("js", "expression", false))]),
					definition::Item::Word("}"),
					fields(list![("body", source("content", "fragment", false))]),
				],
				&[],
			),
		),
		(
			"IfBlock",
			block(
				"if",
				list![
					fields(list![("test", source("js", "expression", false))]),
					definition::Item::Word("}"),
					fields(list![("consequent", source("content", "fragment", false))]),
				],
				list![branch(
					&["{", ":else"],
					list![
						definition::Item::Word("}"),
						fields(list![("alternate", source("content", "fragment", false))]),
					],
					None,
				)],
			),
		),
	]));
	let answer = parse_document(
		"{#repeat items}x{:else}y{/repeat}",
		&host.json(),
		Options(Options::MODULE),
	);
	assert!(
		answer.contains(&format!(
			r#""code":{},"args":["A branch in {{#repeat}}"]"#,
			Code::Placement as u8
		)),
		"{answer}"
	);
}

// a marker read halfway is another word, unless it read a part whole or the punctuation a part
// starts with: `{.5}` and `{email}` are expressions, `{#eac a}` a misspelled block
#[test]
fn a_marker_read_halfway_is_another_word() {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	let js = |field: &'static str| fields(list![(field, source("js", "expression", false))]);
	let content = |field: &'static str| fields(list![(field, source("content", "fragment", false))]);
	let close = || definition::Item::Word("}");
	let mut spread = tag(&["{", "..."], list![js("e"), close()]);
	spread.r#in = Some(list![Place::Attributes]);
	let mut host = minimal();
	host.definition.elements.other = Some(element("Element"));
	host.definition.constructs = Some(Record(list![
		(
			"EachBlock",
			block(
				"each",
				list![js("list"), close(), content("body")],
				list![branch(&["{", "else"], list![close(), content("fallback")], None)],
			),
		),
		("Spread", spread),
		("Expr", expression("e")),
	]));
	let parse = |text: &str| parse_document(text, &host.json(), Options(Options::MODULE));
	for text in [
		"{.5}",
		"<a b={.5}></a>",
		"{email}",
		"{elsewhere}",
		"{#each a}{email}{else}{e}{/each}",
	] {
		let answer = parse(text);
		assert!(!answer.contains("error"), "{text}: {answer}");
	}
	let answer = parse("{#eac a}");
	assert!(
		answer.contains(&format!(r#""code":{},"args":["{{#each}}"]"#, Code::Expected as u8)),
		"{answer}"
	);
	let answer = parse("{#each a}{/eac}");
	assert!(
		answer.contains(&format!(r#""code":{},"args":["{{/each}}"]"#, Code::Expected as u8)),
		"{answer}"
	);
	let answer = parse("{/eachx}");
	assert!(
		answer.contains(&format!(r#""code":{},"args":["eachx"]"#, Code::UnexpectedClose as u8)),
		"{answer}"
	);
}

// blocks may share a close, which closes the innermost of them, in either written order
#[test]
fn a_shared_close_closes_the_innermost_block() {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	let close = || definition::Item::Word("}");
	let with_end = |name: &'static str| definition::Construct {
		close: Some(definition::Piece {
			marker: list!["{", "end"],
			space: None,
			form: list![close()],
		}),
		..block(
			name,
			list![
				fields(list![("e", source("js", "expression", false))]),
				close(),
				fields(list![("body", source("content", "fragment", false))]),
			],
			&[],
		)
	};
	for order in [["range", "if"], ["if", "range"]] {
		let mut host = minimal();
		host.definition.constructs = Some(Record(
			order
				.iter()
				.map(|name| (*name, with_end(name)))
				.collect::<Vec<_>>()
				.leak(),
		));
		for text in ["{#range a}{#if b}x{end}y{end}", "{#if b}{#range a}x{end}y{end}"] {
			let answer = parse_document(text, &host.json(), Options(Options::MODULE));
			assert!(
				!answer.contains("error") && answer.contains("\"data\":\"y\""),
				"{order:?} {text}: {answer}"
			);
		}
	}
}

// a marker that starts with a letter starts a word, and rcdata where no construct stands is text
#[test]
fn markers_stand_where_they_may() {
	let js = |field: &'static str| definition::Item::Fields(Record(list![(field, source("js", "expression", false))]));
	let mut host = minimal();
	host.definition.elements.other = Some(element("Element"));
	host.definition.elements.rules = Some(Record(list![(
		"textarea",
		definition::Element {
			content: Some(definition::Content::Rcdata),
			..element("Element")
		},
	)]));
	let mut expr = expression("e");
	expr.r#in = Some(list![Place::Content]);
	host.definition.constructs = Some(Record(list![
		("Do", tag(&["do"], list![js("e"), definition::Item::Word("end")])),
		("Expr", expr),
	]));
	let parse = |host: &Host, text: &str| parse_document(text, &host.json(), Options(Options::MODULE));
	let answer = parse(&host, "enddo");
	assert!(!answer.contains("error") && !answer.contains("\"Do\""), "{answer}");
	let answer = parse(&host, "x do a end");
	assert!(answer.contains("\"type\":\"Do\""), "{answer}");
	let answer = parse(&host, "<textarea>{x}</textarea>");
	assert!(!answer.contains("error") && !answer.contains("\"Expr\""), "{answer}");
	let mut constructs = host.definition.constructs.unwrap().0.to_vec();
	constructs[1].1.r#in = Some(list![Place::Content, Place::Rcdata]);
	host.definition.constructs = Some(Record(constructs.leak()));
	let answer = parse(&host, "<textarea>{x}</textarea>");
	assert!(answer.contains("\"type\":\"Expr\""), "{answer}");
}

// what a definition says that the engine would not read is refused, naming the field
#[test]
fn a_definition_says_only_what_is_read() {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	let refused = |host: Host, says: &str| {
		let error = Grammar::read(keep(host.json())).unwrap_err();
		assert!(error.contains(says), "expected an error naming {says:?}, got {error:?}");
	};
	let with_block = |items: &'static [definition::Item<'static>]| {
		let mut host = minimal();
		host.definition.constructs = Some(Record(list![("EachBlock", block("each", items, &[]))]));
		host
	};
	refused(
		with_block(list![
			fields(list![("expression", source("js", "expression", false))]),
			definition::Item::Word("}"),
			fields(list![
				("body", source("content", "fragment", false)),
				("fallback", source("content", "fragment", false)),
			]),
		]),
		"fallback is a second body after body",
	);
	refused(
		with_block(list![
			fields(list![("expression", source("value", "expression", false))]),
			definition::Item::Word("}"),
			fields(list![("body", source("content", "fragment", false))]),
		]),
		"expression reads value expression, which a form cannot",
	);

	let mut host = minimal();
	host.definition.constructs = Some(Record(list![(
		"ConstTag",
		tag(
			&["{", "@const"],
			list![
				fields(list![(
					"declaration",
					Source {
						bind: Bind::Inside,
						..source("js", "pattern", false)
					},
				)]),
				definition::Item::Word("}"),
			],
		),
	)]));
	refused(host, "declaration binds, but a tag opens no scope");

	let directive = |items: &'static [definition::Item<'static>]| {
		let mut host = minimal();
		host.definition.directives = Some(definition::Directives {
			prefix: None,
			arg: None,
			modifier: None,
			dynamic: None,
			unique: None,
			fields: Record(&[]),
			shorthands: None,
			rules: Some(Record(list![(
				"on",
				definition::Directive {
					node: "OnDirective",
					form: Some(items),
					unique: None,
				},
			)])),
			other: None,
		});
		host
	};
	refused(
		directive(list![fields(list![(
			"expression",
			source("value", "expression", true)
		)])]),
		"expression is a directive's value",
	);
	refused(
		directive(list![definition::Item::Opt(list![fields(list![
			("expression", source("value", "expression", false)),
			(
				"intro",
				Source {
					literal: Some(definition::Literal::True),
					..source("literal", "literal", false)
				},
			),
		])])]),
		"a flag stands outside groups",
	);

	let mut host = minimal();
	host.definition.elements.rules = Some(Record(list![(
		"svelte:element",
		definition::Element {
			form: Some(list![fields(list![
				("tag", source("element", "this:text", false)),
				("other", source("element", "this", false)),
			])]),
			..element("SvelteElement")
		},
	)]));
	refused(host, "SvelteElement reads one `this` field at most");

	let mut host = minimal();
	host.definition.elements.rules = Some(Record(list![
		("head", element("Head")),
		(
			"title",
			definition::Element {
				inside: Some("haed"),
				..element("Title")
			},
		),
	]));
	refused(host, "Title: inside names haed, which no element rule is");

	let mut host = minimal();
	host.definition.comment = node(
		"Comment",
		list![fields(list![
			("data", source("text", "data", false)),
			("raw", source("text", "raw", false)),
		])],
	);
	refused(host, "Comment holds data and nothing else");

	let mut host = minimal();
	host.definition.fragment = Some(node(
		"Fragment",
		list![fields(list![
			("nodes", source("nodes", "nodes", false)),
			("more", source("nodes", "nodes", false)),
		])],
	));
	refused(host, "Fragment holds one field, its nodes");
}

// a definition the engine could never parse with is refused when it is read, naming the rule
#[test]
fn a_grammar_that_cannot_work_is_refused() {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	let content = |field: &'static str| fields(list![(field, source("content", "fragment", false))]);
	let js = |field: &'static str, read: &'static str| fields(list![(field, source("js", read, false))]);
	let close = || definition::Item::Word("}");
	let bound = |field: &'static str| {
		fields(list![(
			field,
			Source {
				bind: Bind::Inside,
				..source("js", "pattern", false)
			},
		)])
	};
	let refused = |host: Host, says: &str| {
		let error = Grammar::read(keep(host.json())).unwrap_err();
		assert!(error.contains(says), "expected an error naming {says:?}, got {error:?}");
	};
	let with_block = |items: &'static [definition::Item<'static>], branches: &'static [definition::Branch<'static>]| {
		let mut host = minimal();
		host.definition.constructs = Some(Record(list![("X", block("x", items, branches))]));
		host
	};

	let mut host = minimal();
	host.definition.constructs = Some(Record(list![("T", tag(&[], list![close()]))]));
	refused(host, "T: a marker needs parts, none of them empty");
	let mut host = minimal();
	host.definition.constructs = Some(Record(list![("T", tag(&["{", ""], list![close()]))]));
	refused(host, "T: a marker needs parts, none of them empty");
	let mut host = minimal();
	host.definition.constructs = Some(Record(list![("T", tag(&["{"], list![js("e", "expression")]))]));
	refused(host, "T: a marker's form ends its tag with a word");

	refused(
		with_block(list![js("e", "expression"), close()], &[]),
		"X ends in no body",
	);
	refused(
		with_block(
			list![
				js("e", "expression"),
				definition::Item::Opt(list![definition::Item::Word("then"), close(), content("then")]),
			],
			&[],
		),
		"a group ending in bodies is the last item",
	);
	refused(
		with_block(
			list![
				js("e", "expression"),
				definition::Item::OneOf(list![
					list![definition::Item::Word("then"), close(), content("then")],
					list![js("f", "expression"), close()],
				]),
			],
			&[],
		),
		"every alternative ending in a body",
	);
	refused(
		with_block(
			list![
				js("e", "expression"),
				definition::Item::Opt(&[]),
				close(),
				content("body"),
			],
			&[],
		),
		"X: `opt` needs items",
	);
	refused(
		with_block(
			list![
				js("e", "expression"),
				definition::Item::OneOf(&[]),
				close(),
				content("body"),
			],
			&[],
		),
		"X: `oneOf` needs alternatives",
	);
	refused(
		with_block(list![definition::Item::OneOf(&[&[]]), close(), content("body")], &[]),
		"X: an alternative needs items",
	);
	refused(
		with_block(
			list![
				js("e", "expression"),
				definition::Item::Opt(list![definition::Item::Word("as"), js("e", "pattern")]),
				close(),
				content("body"),
			],
			&[],
		),
		"X reads e twice",
	);
	refused(
		with_block(
			list![js("e", "expression"), close(), content("body")],
			list![branch(&[], list![close(), content("other")], None)],
		),
		"X: a marker needs parts",
	);
	refused(
		with_block(
			list![js("e", "expression"), close(), content("body")],
			list![branch(&["{", ":else"], list![js("f", "expression"), close()], None)],
		),
		"X: the {:else} branch ends in no body",
	);
	// each branch that reopens the block has its own flag, true on the block it opened
	let open = list![js("e", "expression"), close(), content("body")];
	let host = with_block(
		open,
		list![
			branch(&["{", ":else", "if"], open, Some(("alternate", "elseif"))),
			branch(&["{", ":else", "when"], open, Some(("alternate", "elsewhen"))),
		],
	);
	let answer = parse_document(
		"{#x a}1{:else when b}2{:else if c}3{/x}",
		&host.json(),
		Options(Options::MODULE),
	);
	let flags: Vec<&str> = answer
		.match_indices("\"elseif\":")
		.chain(answer.match_indices("\"elsewhen\":"))
		.map(|(at, _)| &answer[at..at + 18])
		.collect();
	assert_eq!(flags.len(), 6, "{answer}");
	assert!(
		answer.contains("\"elseif\":false,\"elsewhen\":false")
			&& answer.contains("\"elseif\":false,\"elsewhen\":true")
			&& answer.contains("\"elseif\":true,\"elsewhen\":false"),
		"{answer}"
	);

	// sibling alternatives may read one field, and an alternative's body declares what was bound before the group
	let host = with_block(
		list![
			js("e", "expression"),
			definition::Item::Word("as"),
			bound("alias"),
			definition::Item::OneOf(list![
				list![
					definition::Item::Word("then"),
					definition::Item::Opt(list![bound("v")]),
					close(),
					content("then"),
				],
				list![
					definition::Item::Word("catch"),
					definition::Item::Opt(list![bound("v")]),
					close(),
					content("catch"),
				],
				list![close(), content("pending")],
			]),
		],
		&[],
	);
	let grammar = Grammar::read(keep(host.json())).unwrap();
	let Item::Group { alternatives, .. } = construct(&grammar, "X").open.form.items.last().unwrap() else {
		panic!("the group is the last item");
	};
	let declares = |i: usize| -> Vec<&str> {
		alternatives[i]
			.body
			.as_ref()
			.unwrap()
			.declares
			.iter()
			.map(|d| d.field)
			.collect()
	};
	assert_eq!(declares(0), ["alias", "v"]);
	assert_eq!(declares(2), ["alias"]);

	let mut host = minimal();
	host.definition.constructs = Some(Record(list![("T", tag(&["{", "# x"], list![close()]))]));
	refused(
		host,
		"T: a marker needs parts, none of them empty or holding whitespace",
	);
	let mut host = with_block(list![js("e", "expression"), close(), content("body")], &[]);
	let mut constructs = host.definition.constructs.unwrap().0.to_vec();
	constructs[0].1.r#in = Some(list![Place::Content, Place::Value]);
	host.definition.constructs = Some(Record(constructs.leak()));
	refused(host, "X closes, so it stands in content only");
	let mut pair = tag(&["{"], list![js("a", "identifier"), js("b", "expression"), close()]);
	pair.r#in = Some(list![Place::Rcdata]);
	let mut host = minimal();
	host.definition.constructs = Some(Record(list![("T", pair)]));
	refused(
		host,
		"T: in a value or rcdata a tag reads one expression, then its end word",
	);
	refused(
		with_block(
			list![js("e", "expression"), close(), content("body")],
			list![branch(&["{", "/x"], list![close(), content("other")], None)],
		),
		"X: two of its pieces share the marker {/x",
	);
	let mut host = minimal();
	host.definition.attributes = Some(definition::Attributes {
		shorthand: Some(("{", "}")),
	});
	refused(
		host,
		"the attribute shorthand needs a tag in value that reads one expression",
	);

	let mut host = minimal();
	let mut fields_read = host.definition.elements.fields.0.to_vec();
	fields_read.push(("more", source("element", "name", false)));
	host.definition.elements.fields = Record(fields_read.leak());
	refused(host, "an element reads name twice");
	let mut host = minimal();
	let mut fields_read = host.definition.elements.fields.0.to_vec();
	fields_read[1].1.optional = true;
	host.definition.elements.fields = Record(fields_read.leak());
	refused(host, "attributes on an element is never left out");
	let mut host = minimal();
	host.definition.document.form = Some(list![fields(list![("comments", source("doc", "comments", false))])]);
	refused(host, "Document holds no content");
	let mut host = minimal();
	let mut form = host.definition.document.form.unwrap().to_vec();
	form.push(fields(list![("again", source("content", "fragment", false))]));
	host.definition.document.form = Some(form.leak());
	refused(host, "the document reads fragment twice");
}

// the walker reads what the definition says, not a syntax of its own
#[test]
fn a_host_has_its_own_syntax() {
	let fields = |list: &'static [(&'static str, Source)]| definition::Item::Fields(Record(list));
	let content = |field: &'static str| fields(list![(field, source("content", "fragment", false))]);
	let js = |field: &'static str, read: &'static str| fields(list![(field, source("js", read, false))]);
	let parse = |host: &Host, text: &str| parse_document(text, &host.json(), Options(Options::MODULE));
	let mut host = minimal();
	host.definition.elements.other = Some(element("Element"));
	host.definition.elements.rules = Some(Record(list![(
		"slot",
		definition::Element {
			outside: Some("data-x"),
			..element("Slot")
		},
	)]));
	let constructs = |open: &'static str, close: &'static str| {
		let word = definition::Item::Word(close);
		let mark = |w: &'static str| -> &'static [&'static str] { list![open, w] };
		let construct = |places, marker, space: bool, items| definition::Construct {
			r#in: Some(places),
			open: definition::Piece {
				marker,
				space: space.then_some(true),
				form: items,
			},
			branches: None,
			close: None,
		};
		let for_ = |name: &'static str| definition::Construct {
			r#in: None,
			open: definition::Piece {
				marker: mark(keep(format!("#{name}"))),
				space: Some(true),
				form: list![js("e", "expression"), word, content("body")],
			},
			branches: None,
			close: Some(definition::Piece {
				marker: mark(keep(format!("/{name}"))),
				space: None,
				form: list![word],
			}),
		};
		Record(list![
			("For", for_("for")),
			("ForEach", for_("for-each")),
			(
				"MyTag",
				construct(
					list![Place::Content],
					mark("@myTag"),
					true,
					list![js("e", "expression"), word],
				),
			),
			(
				"Spread",
				construct(
					list![Place::Attributes],
					mark("..."),
					false,
					list![js("expression", "expression"), word],
				),
			),
			(
				"Decl",
				construct(
					list![Place::Content],
					list![open],
					false,
					list![js("statement", "statement"), word],
				),
			),
			(
				"Expr",
				construct(
					list![Place::Content, Place::Value],
					list![open],
					false,
					list![js("value", "expression"), word],
				),
			),
		])
	};

	// the markers can end in an operator, be non-ASCII, and stand among attributes
	for (open, close) in [("<%", "%>"), ("«", "»"), ("{{", "}}"), ("{|", "|}")] {
		host.definition.constructs = Some(constructs(open, close));
		host.definition.attributes = Some(definition::Attributes {
			shorthand: Some((open, close)),
		});
		let answer = parse(&host, &format!("a {open} x %  2 {close} b"));
		assert!(
			answer.contains("\"operator\":\"%\"") && !answer.contains("error"),
			"{open} {close}: {answer}"
		);
		let answer = parse(&host, &format!("<a b=\"{open} x %  2 {close}\"/>"));
		assert!(
			answer.contains("\"operator\":\"%\"") && !answer.contains("error"),
			"{open} {close}: {answer}"
		);
		let answer = parse(&host, &format!("<a {open}name{close} {open}...rest{close}/>"));
		assert!(
			answer.contains("\"name\":\"name\"") && answer.contains("\"type\":\"Spread\""),
			"{open} {close}: {answer}"
		);
		let answer = parse(
			&host,
			&format!(
				"{open}#for-each a{close}x{open}/for-each{close}{open}#for b{close}y{open}/for{close}{open}@myTag c{close}"
			),
		);
		assert!(
			answer.contains("\"type\":\"ForEach\"")
				&& answer.contains("\"type\":\"For\"")
				&& answer.contains("\"type\":\"MyTag\""),
			"{open} {close}: {answer}"
		);
		let answer = parse(&host, &format!("{open}const a = 1 {close}"));
		assert!(
			answer.contains("\"statement\":{\"type\":\"VariableDeclaration\""),
			"{open} {close}: {answer}"
		);
	}
	host.definition.constructs = Some(constructs("{{", "}}"));
	let answer = parse(&host, "{{#fore a}}x{{/fore}}");
	assert!(
		answer.contains(&format!(r#""code":{},"args":["whitespace"]"#, Code::Expected as u8)),
		"{answer}"
	);

	// `outside` names the attribute
	let answer = parse(&host, "<div data-x><slot/></div><div shadowrootmode><slot/></div>");
	assert_eq!(answer.matches("\"type\":\"Slot\"").count(), 1, "{answer}");
	assert!(
		answer.find("data-x").unwrap() < answer.find("\"name\":\"slot\"").unwrap(),
		"{answer}"
	);
}

// what the Rust side writes reads back as the host it wrote, so a test's definition crosses as the package's does
#[test]
fn the_json_round_trips() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let arena = Arena::default();
	let host = Host::read(
		keep(fs::read_to_string(root.join("tests/hosts/svelte/host.json")).unwrap()),
		&arena,
	)
	.unwrap();
	assert_eq!(
		format!("{:?}", Host::read(keep(host.json()), &arena).unwrap()),
		format!("{host:?}")
	);
}

#[test]
fn reads_the_svelte_grammar() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = Grammar::read(keep(
		fs::read_to_string(root.join("tests/hosts/svelte/host.json")).unwrap(),
	))
	.unwrap();
	assert_eq!(grammar.name, "svelte");
	assert_eq!(grammar.document.ty, "Root");
	assert!(
		matches!(&grammar.document.fields[5], DocField::Scope(inner) if matches!(inner[0], DocField::Field { field: "instance", holds: RootField::Script { module: false }, omit: true }))
	);
	assert_eq!(grammar.directive("let").unwrap().declares, Some(vec![]));
	assert_eq!(grammar.fragment, Some(("Fragment", "nodes")));
	assert!(grammar.fragment_scope);
	assert!(grammar.shorthand == Some(("{", "}")) && grammar.autoclose && grammar.trim);
	assert_eq!(construct(&grammar, "HtmlTag").open.marker, ["{", "@html"]);
	assert!(
		construct(&grammar, "AttachTag").stands(Place::Attributes)
			&& !construct(&grammar, "HtmlTag").stands(Place::Attributes)
	);
	assert!(grammar.is_void("br") && grammar.is_void("!DOCTYPE") && !grammar.is_void("div"));
	assert_eq!(grammar.elements.len(), 17);
	assert!(grammar.element("textarea").unwrap().rcdata);
	assert_eq!(grammar.directives.len(), 10);
	let syntax = grammar.directive_syntax.as_ref().unwrap();
	assert_eq!(
		(syntax.arg, syntax.modifier, syntax.arg_field),
		(":", "|", Some("name"))
	);
	assert_eq!(grammar.directive("bind").unwrap().unique, Unique::Attribute);
	assert_eq!(
		grammar.directive("in").unwrap().flags,
		[("intro", true), ("outro", false)]
	);
	let each = construct(&grammar, "EachBlock");
	assert_eq!(each.ty, "EachBlock");
	let body = each.open.form.body.as_ref().unwrap();
	assert_eq!(body.field, "body");
	assert_eq!(body.declares.len(), 2);
	assert!(
		matches!(each.open.form.items[1], Item::Group { ref alternatives, required: false, .. } if alternatives.len() == 1)
	);
	let await_ = construct(&grammar, "AwaitBlock");
	let Item::Group { alternatives, .. } = &await_.open.form.items[1] else {
		panic!()
	};
	assert_eq!(alternatives.len(), 3);
	assert_eq!(alternatives[0].body.as_ref().unwrap().field, "then");
	assert_eq!(await_.branches[1].display, "{:catch}");
	let if_ = construct(&grammar, "IfBlock");
	assert_eq!(if_.chain_flags, ["elseif"]);
	assert_eq!(
		if_.branches[0].form.body.as_ref().unwrap().chain,
		Some(("consequent", "elseif"))
	);
	assert_eq!(grammar.script.as_ref().unwrap().typescript, [("lang", Some("ts"))]);
	assert_eq!(grammar.element("Foo.Bar").unwrap().ty, "Component");
	assert_eq!(grammar.element("div").unwrap().ty, "RegularElement");
	assert_eq!(grammar.element("svelte:head").unwrap().ty, "SvelteHead");
}

#[test]
fn reads_the_vue_grammar() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = Grammar::read(keep(
		fs::read_to_string(root.join("tests/hosts/vue/host.json")).unwrap(),
	))
	.unwrap();
	assert_eq!(construct(&grammar, "Interpolation").open.marker, ["{{"]);
	assert!(
		!grammar.constructs.iter().any(|c| c.stands(Place::Value)) && !grammar.autoclose && grammar.fragment.is_none()
	);
	assert_eq!(grammar.element_fields.children, "children");
	let syntax = grammar.directive_syntax.as_ref().unwrap();
	assert_eq!(
		(syntax.prefix, syntax.modifier, syntax.dynamic),
		(Some("v-"), ".", Some(("[", "]")))
	);
	assert_eq!(grammar.shorthands.len(), 4);
	assert_eq!(grammar.shorthands[3].modifiers, ["prop"]);
	let for_ = grammar.directive("for").unwrap();
	let DirectiveValue::Form(form) = &for_.value else {
		panic!()
	};
	assert!(matches!(form.items[1], Item::Group { required: true, .. }));
	assert_eq!(grammar.directive("anything").unwrap().name, Match::Any);
	assert_eq!(for_.declares, Some(vec!["value", "key", "index"]));
	assert_eq!(grammar.verbatim, Some("v-pre"));
}
