//! A document for each rule of the host grammars beside them, its answer pinned beside it as
//! `NAME.json`: the tree with comments and scopes, the error when it has one. A name says what
//! else is on: `locations`, `erase`, or `recover` for `errorRecovery`. `UPDATE=1` rewrites the
//! pins once a change is meant.

mod common;

use std::fs;
use std::path::Path;
use teasel::Entry;
use teasel::Options;
use teasel::host::grammar::definition::{self, Bind, Host, Node, Source};
use teasel::host::grammar::{DirectiveValue, DocField, Grammar, Item, Match, Record, RootField, Unique};
use teasel::json::{Request, parse_document};

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
		let grammar = fs::read(root.join("tests/hosts").join(&name).join("host.wire")).unwrap();
		for file in sources(&dir) {
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
			let request = Request::from_flags(flags);
			let answer = common::pretty(&parse_document(&source, &grammar, &request));
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
		let grammar = fs::read(root.join("tests/hosts").join(name).join("host.wire")).unwrap();
		for file in sources(&root.join("tests/hosts").join(name)) {
			let source = fs::read_to_string(&file).unwrap();
			for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
				for recover in [false, true] {
					let recovery = if recover { Options::ERROR_RECOVERY } else { 0 };
					let request = Request::from_flags(Options::MODULE | Options::COMMENTS | Options::SCOPES | recovery);
					parse_document(&source[..end], &grammar, &request);
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
	let svelte = fs::read(root.join("tests/hosts/svelte/host.wire")).unwrap();
	let vue = fs::read(root.join("tests/hosts/vue/host.wire")).unwrap();
	let parse = |source: &str, grammar: &[u8], recover: bool| {
		let recovery = if recover { Options::ERROR_RECOVERY } else { 0 };
		let request = Request::from_flags(Options::MODULE | Options::COMMENTS | recovery);
		parse_document(source, grammar, &request)
	};
	assert!(parse("<a x=\"", &svelte, true).contains("\"type\":\"Root\""));
	let comment = parse("<a /*xx", &svelte, true);
	assert!(comment.contains("\"value\":\"xx\""), "{comment}");
	assert!(parse("<script>\"</script>", &svelte, true).contains("\"type\":\"Root\""));
	assert!(parse("<a @x=\"@\"/>", &vue, false).contains("\"error\""));
	let elements = format!("{}x{}", "<a>".repeat(40_000), "</a>".repeat(40_000));
	let branches = format!("{{#if a}}{}{{/if}}", "{:else if a}".repeat(40_000));
	for deep in [elements, branches] {
		assert!(parse(&deep, &svelte, false).contains("\"code\":\"nesting_depth\""));
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
	host.definition.sigils.as_mut().unwrap().tags = Some(Record(vec![(
		"t",
		definition::Tag {
			among: definition::Among::Content,
			node: Node {
				r#type: "T",
				items: vec![
					definition::Item::Opt(vec![definition::Item::Fields(Record(vec![(
						"a",
						source("js", "expression", false),
					)]))]),
					definition::Item::Opt(vec![
						definition::Item::Word(","),
						definition::Item::Fields(Record(vec![("b", source("js", "expression", true))])),
					]),
				],
			},
		},
	)]));
	let answer = parse_document("{@t }", &host.wire(), &Request::from_flags(Options::MODULE));
	assert!(answer.contains("\"a\":null") && !answer.contains("\"b\""), "{answer}");
}

// cargo test --release --test hosts host_phases -- --ignored --nocapture; TEASEL_HOST_BENCH=file adds a document of its own
#[test]
#[ignore]
fn host_phases() {
	use teasel::json::Prepared;
	let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = teasel::json::grammar(&fs::read(root.join("tests/hosts/svelte/host.wire")).unwrap()).unwrap();
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
			let prepared = Prepared::borrowed(source, Request::from_flags(flags));
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

/// The documents of a host directory: not its grammar or the pins.
fn sources(dir: &Path) -> Vec<std::path::PathBuf> {
	common::inputs(dir)
		.into_iter()
		.filter(|f| f.extension().is_some_and(|e| e != "wire"))
		.collect()
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

fn node(ty: &'static str, items: Vec<definition::Item>) -> Node {
	Node { r#type: ty, items }
}

/// A definition with nothing but what every grammar needs.
fn minimal() -> Host {
	let fields = |list: Vec<(&'static str, Source)>| definition::Item::Fields(Record(list));
	Host {
		name: "t",
		definition: definition::Definition {
			document: node(
				"Document",
				vec![fields(vec![("children", source("content", "fragment", false))])],
			),
			text: node("Text", vec![fields(vec![("data", source("text", "data", false))])]),
			comment: node("Comment", vec![fields(vec![("data", source("text", "data", false))])]),
			fragment: None,
			delimiters: ("{", "}"),
			attributes: None,
			autoclose: None,
			trim: None,
			void: None,
			verbatim: None,
			elements: definition::Elements {
				fields: Record(vec![
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
			spread: None,
			sigils: Some(definition::Sigils {
				open: "#",
				branch: ":",
				close: "/",
				tag: "@",
				blocks: None,
				tags: None,
			}),
			declaration: None,
			expression: None,
		},
	}
}

#[test]
fn a_body_declares_only_what_the_form_reads() {
	let block = |context: &'static str| {
		let mut host = minimal();
		let bound = Source {
			bind: Bind::Inside,
			..source("js", "pattern", false)
		};
		host.definition.sigils.as_mut().unwrap().blocks = Some(Record(vec![(
			"each",
			definition::Block {
				branches: Record(Vec::new()),
				node: node(
					"EachBlock",
					vec![
						definition::Item::Fields(Record(vec![("expression", source("js", "expression", false))])),
						definition::Item::Word("as"),
						definition::Item::Fields(Record(vec![(context, bound)])),
						definition::Item::Fields(Record(vec![("body", source("content", "fragment", false))])),
					],
				),
			},
		)]));
		host.wire()
	};
	let grammar = Grammar::read(&block("context")).unwrap();
	assert_eq!(
		grammar.block("each").unwrap().open.body.as_ref().unwrap().declares[0].field,
		"context"
	);
	assert!(Grammar::read(&[]).is_err() && Grammar::read(&block("context")[..40]).is_err());
	let mut host = minimal();
	host.definition.document.items.push(definition::Item::Word("x"));
	assert!(Grammar::read(&host.wire()).unwrap_err().contains("document"));
}

// the wire the Rust side writes is the one it reads, so both ends stay one format
#[test]
fn the_wire_round_trips() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let bytes = fs::read(root.join("tests/hosts/svelte/host.wire")).unwrap();
	let host = Host::read(&bytes).unwrap();
	assert_eq!(host.wire(), bytes);
	assert_eq!(format!("{:?}", Host::read(&host.wire()).unwrap()), format!("{host:?}"));
}

#[test]
fn reads_the_svelte_grammar() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = Grammar::read(&fs::read(root.join("tests/hosts/svelte/host.wire")).unwrap()).unwrap();
	assert_eq!(grammar.name, "svelte");
	assert_eq!(grammar.document.ty, "Root");
	assert!(
		matches!(&grammar.document.fields[5], DocField::Scope(inner) if matches!(inner[0], DocField::Field { field: "instance", holds: RootField::Script { module: false }, omit: true }))
	);
	assert_eq!(grammar.directive("let").unwrap().declares, Some(vec![]));
	assert_eq!(grammar.fragment, Some(("Fragment", "nodes")));
	assert!(grammar.fragment_scope);
	assert!(grammar.attribute_expressions && grammar.attribute_shorthand && grammar.autoclose && grammar.trim);
	assert_eq!(grammar.sigils.as_ref().unwrap().tag, "@");
	assert!(grammar.tag("attach").unwrap().attribute && !grammar.tag("html").unwrap().attribute);
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
	let each = grammar.block("each").unwrap();
	assert_eq!(each.ty, "EachBlock");
	let body = each.open.body.as_ref().unwrap();
	assert_eq!(body.field, "body");
	assert_eq!(body.declares.len(), 2);
	assert!(
		matches!(each.open.items[1], Item::Group { ref alternatives, required: false, .. } if alternatives.len() == 1)
	);
	let await_ = grammar.block("await").unwrap();
	let Item::Group { alternatives, .. } = &await_.open.items[1] else {
		panic!()
	};
	assert_eq!(alternatives.len(), 3);
	assert_eq!(alternatives[0].body.as_ref().unwrap().field, "then");
	assert_eq!(await_.branches[1].words, ["catch"]);
	let if_ = grammar.block("if").unwrap();
	assert_eq!(if_.chain_flag, Some("elseif"));
	assert_eq!(if_.branches[0].form.body.as_ref().unwrap().chain, Some("consequent"));
	assert_eq!(grammar.script.as_ref().unwrap().typescript, [("lang", Some("ts"))]);
	assert_eq!(grammar.element("Foo.Bar").unwrap().ty, "Component");
	assert_eq!(grammar.element("div").unwrap().ty, "RegularElement");
	assert_eq!(grammar.element("svelte:head").unwrap().ty, "SvelteHead");
}

#[test]
fn reads_the_vue_grammar() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = Grammar::read(&fs::read(root.join("tests/hosts/vue/host.wire")).unwrap()).unwrap();
	assert_eq!(grammar.delimiters, ("{{", "}}"));
	assert!(!grammar.attribute_expressions && !grammar.autoclose && grammar.fragment.is_none());
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

// the types the JavaScript side writes a grammar as, pinned beside its writer
#[test]
fn wire_types() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	assert!(
		common::pinned(&root.join("../../npm/src/wire.ts"), &Grammar::wire_types()),
		"npm/src/wire.ts changed; run with UPDATE=1 once the change is meant"
	);
}

// cargo test --release --test hosts wire_cost -- --ignored --nocapture
#[test]
#[ignore]
fn wire_cost() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let bytes = fs::read(root.join("tests/hosts/svelte/host.wire")).unwrap();
	let t = std::time::Instant::now();
	let first = Grammar::read(&bytes).unwrap();
	println!("first Grammar::read in this process: {:?}", t.elapsed());
	let t = std::time::Instant::now();
	for _ in 0..1000 {
		std::hint::black_box(Grammar::read(&bytes).unwrap());
	}
	println!("warm Grammar::read: {:?}", t.elapsed() / 1000);
	drop(first);
}
