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

// what a definition may not say is an error naming its field, never a panic or a silent drop
#[test]
fn a_definition_is_refused_by_name() {
	let block = |items: Vec<definition::Item>, branches: Vec<(&'static str, definition::Branch)>| {
		let mut host = minimal();
		host.definition.sigils.as_mut().unwrap().blocks = Some(Record(vec![(
			"if",
			definition::Block {
				node: node("IfBlock", items),
				branches: Record(branches),
			},
		)]));
		host.wire()
	};
	let reopen = definition::Branch::Reopen(definition::Reopen {
		reopen: "alternate",
		flag: "elseif",
	});
	let error = Grammar::read(&block(Vec::new(), vec![("else if", reopen.clone())])).unwrap_err();
	assert!(error.contains("ends in its body"), "{error}");
	let bound = Source {
		bind: Bind::Inside,
		..source("js", "pattern", false)
	};
	let error = Grammar::read(&block(
		vec![definition::Item::Fields(Record(vec![("test", bound.clone())]))],
		Vec::new(),
	))
	.unwrap_err();
	assert!(error.contains("if binds test"), "{error}");
	let grammar = Grammar::read(&block(
		vec![
			definition::Item::Fields(Record(vec![("test", bound)])),
			definition::Item::Fields(Record(vec![("consequent", source("content", "fragment", false))])),
		],
		vec![("else if", reopen)],
	))
	.unwrap();
	assert_eq!(
		grammar.block("if").unwrap().branches[0]
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
		fields: Record(Vec::new()),
		shorthands: None,
		rules: Some(Record(vec![(
			"on",
			definition::Directive {
				unique: definition::Uniqueness::No,
				node: node(
					"OnDirective",
					vec![definition::Item::Fields(Record(vec![(
						"modifiers",
						Source {
							literal: Some(definition::Literal::List),
							..source("literal", "literal", false)
						},
					)]))],
				),
			},
		)])),
		other: None,
	});
	let error = Grammar::read(&host.wire()).unwrap_err();
	assert!(error.contains("modifiers on a directive is a flag"), "{error}");
	let mut host = minimal();
	host.definition.document.items = vec![definition::Item::Fields(Record(vec![(
		"js",
		Source {
			literal: Some(definition::Literal::True),
			..source("literal", "literal", false)
		},
	)]))];
	let error = Grammar::read(&host.wire()).unwrap_err();
	assert!(error.contains("js on the document"), "{error}");
	let mut short = minimal().wire();
	short[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
	assert!(Grammar::read(&short).unwrap_err().contains("ends early"));
}

// how a definition groups its fields into records never changes what it reads
#[test]
fn records_group_freely() {
	let fields = |list: Vec<(&'static str, Source)>| definition::Item::Fields(Record(list));
	let parse = |host: &Host, text: &str| parse_document(text, &host.wire(), &Request::from_flags(Options::MODULE));

	let mut host = minimal();
	host.definition.sigils.as_mut().unwrap().blocks = Some(Record(vec![(
		"if",
		definition::Block {
			node: node(
				"IfBlock",
				vec![fields(vec![
					("test", source("js", "expression", false)),
					("consequent", source("content", "fragment", false)),
				])],
			),
			branches: Record(vec![(
				"else if",
				definition::Branch::Reopen(definition::Reopen {
					reopen: "alternate",
					flag: "elseif",
				}),
			)]),
		},
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
		fields: Record(vec![("name", source("directive", "arg", false))]),
		shorthands: None,
		rules: Some(Record(vec![(
			"transition",
			definition::Directive {
				unique: definition::Uniqueness::No,
				node: node(
					"TransitionDirective",
					vec![fields(vec![
						("expression", source("value", "expression", false)),
						(
							"intro",
							Source {
								literal: Some(definition::Literal::True),
								..source("literal", "literal", false)
							},
						),
					])],
				),
			},
		)])),
		other: None,
	});
	host.definition.attributes = Some(definition::Attributes {
		expressions: Some(true),
		shorthand: None,
	});
	host.definition.elements.other = Some(definition::Element {
		node: node("RegularElement", Vec::new()),
		root: false,
		once: false,
		inside: None,
		outside: None,
		content: None,
	});
	host.definition.expression = Some(node(
		"ExpressionTag",
		vec![fields(vec![("expression", source("js", "expression", false))])],
	));
	let answer = parse(&host, "<a transition:fade={params}/>");
	assert!(
		!answer.contains("\"error\"") && answer.contains("\"name\":\"params\"") && answer.contains("\"intro\":true"),
		"{answer}"
	);
}

// what a definition says that the engine would not read is refused, naming the field
#[test]
fn a_definition_says_only_what_is_read() {
	let fields = |list: Vec<(&'static str, Source)>| definition::Item::Fields(Record(list));
	let refused = |host: Host, says: &str| {
		let error = Grammar::read(&host.wire()).unwrap_err();
		assert!(error.contains(says), "expected an error naming {says:?}, got {error:?}");
	};
	let with_block = |items: Vec<definition::Item>| {
		let mut host = minimal();
		host.definition.sigils.as_mut().unwrap().blocks = Some(Record(vec![(
			"each",
			definition::Block {
				node: node("EachBlock", items),
				branches: Record(Vec::new()),
			},
		)]));
		host
	};
	refused(
		with_block(vec![
			fields(vec![("expression", source("js", "expression", false))]),
			fields(vec![
				("body", source("content", "fragment", false)),
				("fallback", source("content", "fragment", false)),
			]),
		]),
		"fallback is a second body after body",
	);
	refused(
		with_block(vec![
			fields(vec![("expression", source("value", "expression", false))]),
			fields(vec![("body", source("content", "fragment", false))]),
		]),
		"expression reads value expression, which a form cannot",
	);

	let mut host = minimal();
	host.definition.sigils.as_mut().unwrap().tags = Some(Record(vec![(
		"const",
		definition::Tag {
			among: definition::Among::Content,
			node: node(
				"ConstTag",
				vec![fields(vec![(
					"declaration",
					Source {
						bind: Bind::Inside,
						..source("js", "pattern", false)
					},
				)])],
			),
		},
	)]));
	refused(host, "declaration binds, but a tag opens no scope");

	let directive = |items: Vec<definition::Item>| {
		let mut host = minimal();
		host.definition.directives = Some(definition::Directives {
			prefix: None,
			arg: None,
			modifier: None,
			dynamic: None,
			unique: None,
			fields: Record(Vec::new()),
			shorthands: None,
			rules: Some(Record(vec![(
				"on",
				definition::Directive {
					unique: definition::Uniqueness::No,
					node: node("OnDirective", items),
				},
			)])),
			other: None,
		});
		host
	};
	refused(
		directive(vec![fields(vec![("expression", source("value", "expression", true))])]),
		"expression is a directive's value",
	);
	refused(
		directive(vec![definition::Item::Opt(vec![fields(vec![
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
	host.definition.elements.rules = Some(Record(vec![(
		"svelte:element",
		definition::Element {
			node: node(
				"SvelteElement",
				vec![fields(vec![
					("tag", source("element", "this:text", false)),
					("other", source("element", "this", false)),
				])],
			),
			root: false,
			once: false,
			inside: None,
			outside: None,
			content: None,
		},
	)]));
	refused(host, "SvelteElement reads one `this` field at most");

	let mut host = minimal();
	host.definition.comment = node(
		"Comment",
		vec![fields(vec![
			("data", source("text", "data", false)),
			("raw", source("text", "raw", false)),
		])],
	);
	refused(host, "Comment holds data and nothing else");

	let mut host = minimal();
	host.definition.fragment = Some(node(
		"Fragment",
		vec![fields(vec![
			("nodes", source("nodes", "nodes", false)),
			("more", source("nodes", "nodes", false)),
		])],
	));
	refused(host, "Fragment holds one field, its nodes");
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
