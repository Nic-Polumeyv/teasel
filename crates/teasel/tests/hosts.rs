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
	assert!(error.contains("IfBlock ends in no body"), "{error}");
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
	assert!(error.contains("OnDirective: modifiers is a flag"), "{error}");
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

// a branch in a block that has none names the block, rather than an empty list of branches
#[test]
fn a_branch_names_a_block_without_branches() {
	let mut host = minimal();
	host.definition.sigils.as_mut().unwrap().blocks = Some(Record(vec![(
		"repeat",
		definition::Block {
			node: node(
				"RepeatBlock",
				vec![
					definition::Item::Fields(Record(vec![("list", source("js", "expression", false))])),
					definition::Item::Fields(Record(vec![("body", source("content", "fragment", false))])),
				],
			),
			branches: Record(Vec::new()),
		},
	)]));
	let answer = parse_document(
		"{#repeat items}x{:empty}y{/repeat}",
		&host.wire(),
		&Request::from_flags(Options::MODULE),
	);
	assert!(answer.contains("A branch in repeat is not allowed here"), "{answer}");
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

// a definition the engine could never parse with is refused when it is read, naming the rule
#[test]
fn a_grammar_that_cannot_work_is_refused() {
	let fields = |list: Vec<(&'static str, Source)>| definition::Item::Fields(Record(list));
	let content = |field: &'static str| fields(vec![(field, source("content", "fragment", false))]);
	let js = |field: &'static str, read: &'static str| fields(vec![(field, source("js", read, false))]);
	let bound = |field: &'static str| {
		fields(vec![(
			field,
			Source {
				bind: Bind::Inside,
				..source("js", "pattern", false)
			},
		)])
	};
	let refused = |host: Host, says: &str| {
		let error = Grammar::read(&host.wire()).unwrap_err();
		assert!(error.contains(says), "expected an error naming {says:?}, got {error:?}");
	};
	let with_block = |items: Vec<definition::Item>, branches: Vec<(&'static str, definition::Branch)>| {
		let mut host = minimal();
		host.definition.sigils.as_mut().unwrap().blocks = Some(Record(vec![(
			"x",
			definition::Block {
				node: node("X", items),
				branches: Record(branches),
			},
		)]));
		host
	};
	let reopen = |flag: &'static str| {
		definition::Branch::Reopen(definition::Reopen {
			reopen: "alternate",
			flag,
		})
	};

	let mut host = minimal();
	host.definition.delimiters = ("", "}");
	refused(host, "the open delimiter is empty");
	let mut host = minimal();
	host.definition.sigils.as_mut().unwrap().tag = "";
	refused(host, "the tag sigil is empty");
	let mut host = minimal();
	host.definition.sigils.as_mut().unwrap().close = "#";
	refused(host, "the open and close sigils are both #");

	refused(with_block(vec![js("e", "expression")], Vec::new()), "X ends in no body");
	refused(
		with_block(
			vec![
				js("e", "expression"),
				definition::Item::Opt(vec![definition::Item::Word("then"), content("then")]),
			],
			Vec::new(),
		),
		"a group ending in bodies is the last item",
	);
	refused(
		with_block(
			vec![
				js("e", "expression"),
				definition::Item::OneOf(vec![
					vec![definition::Item::Word("then"), content("then")],
					vec![js("f", "expression")],
				]),
			],
			Vec::new(),
		),
		"every alternative ending in a body",
	);
	refused(
		with_block(
			vec![
				js("e", "expression"),
				definition::Item::Opt(Vec::new()),
				content("body"),
			],
			Vec::new(),
		),
		"X: `opt` needs items",
	);
	refused(
		with_block(
			vec![
				js("e", "expression"),
				definition::Item::OneOf(Vec::new()),
				content("body"),
			],
			Vec::new(),
		),
		"X: `oneOf` needs alternatives",
	);
	refused(
		with_block(
			vec![definition::Item::OneOf(vec![Vec::new()]), content("body")],
			Vec::new(),
		),
		"X: an alternative needs items",
	);
	refused(
		with_block(
			vec![
				js("e", "expression"),
				definition::Item::Opt(vec![definition::Item::Word("as"), js("e", "pattern")]),
				content("body"),
			],
			Vec::new(),
		),
		"X reads e twice",
	);
	refused(
		with_block(
			vec![js("e", "expression"), content("body")],
			vec![("", definition::Branch::Form(vec![content("other")]))],
		),
		"X: a branch needs words",
	);
	refused(
		with_block(
			vec![js("e", "expression"), content("body")],
			vec![("else", definition::Branch::Form(vec![js("f", "expression")]))],
		),
		"X: the else branch ends in no body",
	);
	// each branch that reopens the block has its own flag, true on the block it opened
	let host = with_block(
		vec![js("e", "expression"), content("body")],
		vec![("else if", reopen("elseif")), ("else when", reopen("elsewhen"))],
	);
	let answer = parse_document(
		"{#x a}1{:else when b}2{:else if c}3{/x}",
		&host.wire(),
		&Request::from_flags(Options::MODULE),
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
		vec![
			js("e", "expression"),
			definition::Item::Word("as"),
			bound("alias"),
			definition::Item::OneOf(vec![
				vec![
					definition::Item::Word("then"),
					definition::Item::Opt(vec![bound("v")]),
					content("then"),
				],
				vec![
					definition::Item::Word("catch"),
					definition::Item::Opt(vec![bound("v")]),
					content("catch"),
				],
				vec![content("pending")],
			]),
		],
		Vec::new(),
	);
	let grammar = Grammar::read(&host.wire()).unwrap();
	let Item::Group { alternatives, .. } = grammar.block("x").unwrap().open.items.last().unwrap() else {
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
	host.definition
		.elements
		.fields
		.0
		.push(("more", source("element", "name", false)));
	refused(host, "an element reads name twice");
	let mut host = minimal();
	host.definition.elements.fields.0[1].1.optional = true;
	refused(host, "attributes on an element is never left out");
	let mut host = minimal();
	host.definition.document.items = vec![fields(vec![("comments", source("doc", "comments", false))])];
	refused(host, "Document holds no content");
	let mut host = minimal();
	host.definition
		.document
		.items
		.push(fields(vec![("again", source("content", "fragment", false))]));
	refused(host, "the document reads fragment twice");
}

// the walker reads what the definition says, not a syntax of its own
#[test]
fn a_host_has_its_own_syntax() {
	let fields = |list: Vec<(&'static str, Source)>| definition::Item::Fields(Record(list));
	let content = |field: &'static str| fields(vec![(field, source("content", "fragment", false))]);
	let js = |field: &'static str, read: &'static str| fields(vec![(field, source("js", read, false))]);
	let parse = |host: &Host, text: &str| parse_document(text, &host.wire(), &Request::from_flags(Options::MODULE));
	let element = |ty: &'static str, outside: Option<&'static str>| definition::Element {
		node: node(ty, Vec::new()),
		root: false,
		once: false,
		inside: None,
		outside,
		content: None,
	};
	let mut host = minimal();
	host.definition.expression = Some(node("Expr", vec![js("value", "expression")]));
	host.definition.declaration = Some(node("Decl", vec![js("statement", "statement")]));
	host.definition.attributes = Some(definition::Attributes {
		expressions: Some(true),
		shorthand: Some(true),
	});
	host.definition.spread = Some("Spread");
	host.definition.elements.other = Some(element("Element", None));
	host.definition.elements.rules = Some(Record(vec![("slot", element("Slot", Some("data-x")))]));
	let sigils = host.definition.sigils.as_mut().unwrap();
	sigils.blocks = Some(Record(vec![
		(
			"for",
			definition::Block {
				node: node("For", vec![js("e", "expression"), content("body")]),
				branches: Record(Vec::new()),
			},
		),
		(
			"for-each",
			definition::Block {
				node: node("ForEach", vec![js("e", "expression"), content("body")]),
				branches: Record(Vec::new()),
			},
		),
	]));
	sigils.tags = Some(Record(vec![(
		"myTag",
		definition::Tag {
			node: node("MyTag", vec![js("e", "expression")]),
			among: definition::Among::Content,
		},
	)]));

	// the delimiters can end in an operator, be non-ASCII, and stand among attributes
	for (open, close) in [("<%", "%>"), ("«", "»"), ("{{", "}}"), ("{|", "|}")] {
		host.definition.delimiters = (open, close);
		let answer = parse(&host, &format!("a {open} x %  2 {close} b"));
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
	host.definition.delimiters = ("{{", "}}");
	let answer = parse(&host, "{{#fore a}}x{{/fore}}");
	assert!(answer.contains("Expected whitespace"), "{answer}");

	// `outside` names the attribute
	let answer = parse(&host, "<div data-x><slot/></div><div shadowrootmode><slot/></div>");
	assert_eq!(answer.matches("\"type\":\"Slot\"").count(), 1, "{answer}");
	assert!(
		answer.find("data-x").unwrap() < answer.find("\"name\":\"slot\"").unwrap(),
		"{answer}"
	);
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
