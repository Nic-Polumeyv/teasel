//! Svelte's tests. A component for each construct under `tests/hosts/svelte`, its answer pinned
//! beside it as `NAME.json`: the tree with comments and scopes, the error when it has one; a name
//! says what else is on: `erase`, or `recover` for `errorRecovery`. Svelte's own tests, every
//! `.svelte` file under `packages/svelte/tests` of the repository cloned next to this file as in
//! ci.yml, the whole answer of each under five option sets, errors included, pinned as a hash in
//! `svelte.hashes`; without the clone that test is skipped. Then the rules a component is read by,
//! one construct at a time. `UPDATE=1` rewrites the pins once a change is meant.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use teasel::Code;
use teasel::Entry;
use teasel::Options;

/// A component as JSON.
fn svelte(source: &str, flags: u32) -> String {
	teasel::estree::parse(source, Entry::Svelte, 0, Options(flags))
}

// ── the pins

#[test]
fn components() {
	let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hosts/svelte");
	let mut wrong = Vec::new();
	for file in common::inputs(&dir) {
		let stem = file.file_stem().unwrap().to_str().unwrap();
		let source = fs::read_to_string(&file).unwrap();
		let mut flags = Options::MODULE | Options::COMMENTS | Options::SCOPES;
		for (word, bit) in [("erase", Options::ERASE), ("recover", Options::ERROR_RECOVERY)] {
			if stem.contains(word) {
				flags |= bit;
			}
		}
		let answer = common::pretty(&svelte(&source, flags));
		if !common::pinned(&file.with_extension("json"), &answer) {
			wrong.push(format!("svelte/{stem}"));
		}
	}
	assert!(
		wrong.is_empty(),
		"answers changed for:\n{}\nrun with UPDATE=1 once the change is meant",
		wrong.join("\n")
	);
}

/// Every prefix of every component, strict and recovering: unfinished input is an answer or an
/// error, never a panic.
#[test]
fn every_prefix_answers() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	for file in common::inputs(&root.join("tests/hosts/svelte")) {
		let source = fs::read_to_string(&file).unwrap();
		for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
			for recover in [false, true] {
				let recovery = if recover { Options::ERROR_RECOVERY } else { 0 };
				svelte(&source[..end], Options::MODULE | Options::COMMENTS | Options::SCOPES | recovery);
			}
		}
	}
}

/// The files under `dir`, by their path from it, sorted.
fn files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
	let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
	entries.sort();
	for path in entries {
		if path.is_dir() {
			files(root, &path, out);
		} else if path.extension().is_some_and(|e| e == "svelte") {
			let name = path.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/");
			out.push((name, path));
		}
	}
}

/// FNV-1a over the answers' bytes: no dependency, and a change anywhere in a tree shows.
fn hash(texts: &[String]) -> u64 {
	texts.iter().flat_map(|text| text.bytes().chain([0])).fold(0xcbf29ce484222325u64, |h, b| {
		(h ^ u64::from(b)).wrapping_mul(0x100000001b3)
	})
}

#[test]
fn svelte_tests() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/svelte/packages/svelte/tests");
	if !root.exists() {
		eprintln!("skipped: no tests/svelte");
		return;
	}
	let sets = [
		0,
		Options::COMMENTS | Options::SCOPES,
		Options::ERROR_RECOVERY | Options::COMMENTS | Options::SCOPES,
		Options::ERASE | Options::COMMENTS | Options::SCOPES,
		Options::PARENTHESIZED | Options::SCOPES,
	];
	let mut list = Vec::new();
	files(&root, &root, &mut list);
	let mut lines = Vec::new();
	for (name, path) in list {
		let source = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
		let answers: Vec<String> = sets.iter().map(|&bits| svelte(&source, bits)).collect();
		lines.push(format!("{name} {:016x}", hash(&answers)));
	}
	let pins = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/svelte.hashes");
	let now = lines.join("\n") + "\n";
	if !common::pinned(&pins, &now) {
		let pinned = fs::read_to_string(&pins).unwrap_or_default();
		let changed: Vec<&str> = now
			.lines()
			.zip(pinned.lines().chain(std::iter::repeat("")))
			.filter(|(a, b)| a != b)
			.map(|(a, _)| a.split(' ').next().unwrap())
			.collect();
		panic!(
			"answers changed for:\n{}\nrun with UPDATE=1 once the change is meant",
			changed.join("\n")
		);
	}
}

// ── the command line

#[test]
fn batch_headers() {
	use std::io::Write;
	use std::process::{Command, Stdio};
	let batch = |input: &str| {
		let mut child = Command::new(env!("CARGO_BIN_EXE_teasel"))
			.arg("--batch")
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
	let (ok, answers, _) = batch("0 1 2\na;1 0 5\na { }2 1 7\n<a></a>");
	assert!(ok && answers == 3);
	for (input, error) in [
		("0 1 2\na;0 1 2 5\na;", "malformed header"),
		("0 1 2\na;3 1 2\na;", "3 is no language"),
	] {
		let (ok, answers, stderr) = batch(input);
		assert!(!ok && answers == 1 && stderr.contains(error), "{input}: {stderr}");
	}
}

// ── what a component is read by

/// The unfinished and odd inputs the audit found panicking.
#[test]
fn unfinished_input() {
	let parse = |source: &str, recover: bool| {
		let recovery = if recover { Options::ERROR_RECOVERY } else { 0 };
		svelte(source, Options::MODULE | Options::COMMENTS | recovery)
	};
	assert!(parse("<a x=\"", true).contains("\"type\":\"Root\""));
	let comment = parse("<a /*xx", true);
	assert!(comment.contains("\"value\":\"xx\""), "{comment}");
	assert!(parse("<script>\"</script>", true).contains("\"type\":\"Root\""));
	for source in ["<script / ></script>", "<script / é></script>"] {
		assert!(parse(source, true).contains("\"type\":\"Root\""));
	}
	assert!(parse("<a on:x={@}/>", false).contains("\"node\":null"));
	let elements = format!("{}x{}", "<a>".repeat(40_000), "</a>".repeat(40_000));
	let blocks = format!("{}{}", "{#if a}".repeat(40_000), "{/if}".repeat(40_000));
	for deep in [elements, blocks] {
		assert!(parse(&deep, false).contains(&format!("\"code\":{},", Code::NestingDepth as u8)));
	}
}

/// A script, a style sheet and raw text end at the first closing tag, read by one rule, wherever
/// the scan's eight bytes fall.
#[test]
fn raw_text_ends_at_its_closing_tag() {
	let parse = |source: &str| svelte(source, Options::MODULE);
	let unclosed = format!(r#""code":{},"args":["script"]"#, Code::Unclosed as u8);
	for pad in 0..9 {
		let pad = " ".repeat(pad);
		for body in [
			"a<b",
			"1<<2",
			"\"é\"<1",
			"\"</div>\"",
			"\"</scripts>\"",
			"\"</script\"",
			"a</1/",
		] {
			for closer in ["</script>", "</script \t\n>", "</SCRIPT>", "</script x>"] {
				let answer = parse(&format!("<script>{pad}{body}{closer}"));
				let end = 8 + pad.len() + body.encode_utf16().count();
				assert!(
					answer.contains(&format!(r#""content":{{"type":"Program","start":8,"end":{end},"#)),
					"{pad}{body}{closer}: {answer}"
				);
			}
		}
		for body in [
			"a<b",
			"a <",
			"a</",
			"a</scrip",
			"a</script",
			"a</script x",
			"a</scripts>",
		] {
			let answer = parse(&format!("<script>{pad}{body}"));
			assert!(answer.contains(&unclosed), "{pad}{body}: {answer}");
		}
		for closer in ["</style>", "</style \t\n>", "</STYLE>", "</style x>"] {
			let answer = parse(&format!("<style>{pad}a{{}}{closer}"));
			assert!(
				answer.contains(&format!(r#""styles":"{pad}a{{}}","#)),
				"{pad}{closer}: {answer}"
			);
		}
		for closer in ["", "</style", "</style x", "</styles>"] {
			let answer = parse(&format!("<style>{pad}a{{}}{closer}"));
			assert!(answer.contains("\"node\":null"), "{pad}{closer}: {answer}");
		}
		// a `<` that opens no `</`, the closing tag right after it among them
		for body in ["a<b", "<<", "é<</x>", "</scripts>", "</script", "<", "a<<", "</<"] {
			for closer in ["</script>", "</SCRIPT x>"] {
				let source = format!("<div><script>{pad}{body}{closer}</div>");
				let answer = parse(&source);
				assert!(
					answer.contains(&format!(r#""raw":"{pad}{body}","#)),
					"{source}: {answer}"
				);
			}
			let answer = parse(&format!("<div><script>{pad}{body}"));
			assert!(answer.contains(&unclosed), "{pad}{body}: {answer}");
		}
	}
}

// a branch of another block in a block that has none names the block it stands in
#[test]
fn a_branch_names_a_block_without_branches() {
	let answer = svelte("{#key items}x{:else}y{/key}", Options::MODULE);
	assert!(
		answer.contains(&format!(
			r#""code":{},"args":["A branch in {{#key}}"]"#,
			Code::Placement as u8
		)),
		"{answer}"
	);
}

// a marker read halfway is another word, unless it read a part whole or the punctuation a part
// starts with: `{.5}` and `{email}` are expressions, `{#eac a}` a misspelled block
#[test]
fn a_marker_read_halfway_is_another_word() {
	let parse = |text: &str| svelte(text, Options::MODULE);
	for text in [
		"{.5}",
		"<a b={.5}></a>",
		"{email}",
		"{elsewhere}",
		"{#each a as b}{email}{:else}{e}{/each}",
	] {
		let answer = parse(text);
		assert!(!answer.contains("error"), "{text}: {answer}");
	}
	for (text, code, arg) in [
		("{#eac a}", Code::Expected, "{#each}"),
		("{#each a as b}{/eac}", Code::Expected, "{/each}"),
		("{/eachx}", Code::UnexpectedClose, "eachx"),
		("{#eachx a}", Code::Expected, "whitespace"),
	] {
		let answer = parse(text);
		assert!(
			answer.contains(&format!(r#""code":{},"args":["{arg}"]"#, code as u8)),
			"{text}: {answer}"
		);
	}
}

// an expression stands in an element's text, a block does not
#[test]
fn markers_stand_where_they_may() {
	let answer = svelte("<textarea>a {x}</textarea>", Options::MODULE);
	assert!(!answer.contains("error") && answer.contains("\"type\":\"ExpressionTag\""), "{answer}");
	let answer = svelte("<textarea>{#if a}b{/if}</textarea>", Options::MODULE);
	assert!(
		answer.contains(&format!(
			r#""code":{},"args":["A block or tag in <textarea>"]"#,
			Code::Placement as u8
		)),
		"{answer}"
	);
}

// a tag under `svelte:` needs a rule of its own; under any other prefix it reads as any other
#[test]
fn a_reserved_prefix_needs_a_rule() {
	let parse = |text: &str| svelte(text, Options::MODULE);
	let answer = parse("<x:foo></x:foo>");
	assert!(answer.contains("\"type\":\"RegularElement\""), "{answer}");
	let answer = parse("<svelte:head></svelte:head><y:foo></y:foo>");
	assert!(
		answer.contains("\"type\":\"SvelteHead\"") && answer.contains("\"type\":\"RegularElement\""),
		"{answer}"
	);
	let answer = parse("<svelte:foo></svelte:foo>");
	assert!(
		answer.contains(&format!(r#""code":{},"args":["svelte:foo"]"#, Code::InvalidName as u8)),
		"{answer}"
	);
}

// a slot is Svelte's but inside an element with `shadowrootmode`
#[test]
fn a_slot_in_a_shadow_root_is_an_element() {
	let answer = svelte("<div data-x><slot/></div><div shadowrootmode><slot/></div>", Options::MODULE);
	assert_eq!(answer.matches("\"type\":\"SlotElement\"").count(), 1, "{answer}");
	assert!(
		answer.find("\"type\":\"SlotElement\"").unwrap() < answer.find("shadowrootmode").unwrap(),
		"{answer}"
	);
}

// a field a block's head did not read is null, or absent where Svelte leaves it out
#[test]
fn an_unread_field_is_null_or_absent() {
	let answer = svelte("{#each xs}{/each}", Options::MODULE);
	assert!(
		answer.contains("\"context\":null")
			&& !answer.contains("\"index\"")
			&& !answer.contains("\"key\"")
			&& !answer.contains("\"fallback\""),
		"{answer}"
	);
}

// Svelte's own rules: void elements, where a tag may stand, what a directive says, what may not repeat
#[test]
fn svelte_rules() {
	let parse = |text: &str| svelte(text, Options::MODULE | Options::SCOPES);
	let code = |answer: &str, code: Code, arg: &str| answer.contains(&format!(r#""code":{},"args":["{arg}"]"#, code as u8));
	let answer = parse("<br><!DOCTYPE html><div></div>");
	assert!(!answer.contains("\"code\"") && answer.matches("\"type\":\"RegularElement\"").count() == 3, "{answer}");
	assert!(code(&parse("<div {@html x}></div>"), Code::Placement, "{@html} among attributes"));
	assert!(parse("<div {@attach x}></div>").contains("\"type\":\"AttachTag\""));
	let answer = parse("<a in:fade out:fly transition:t/>");
	assert!(
		answer.contains(r#""name":"fade","expression":null,"modifiers":[],"intro":true,"outro":false"#)
			&& answer.contains(r#""name":"fly","expression":null,"modifiers":[],"intro":false,"outro":true"#)
			&& answer.contains(r#""name":"t","expression":null,"modifiers":[],"intro":true,"outro":true"#),
		"{answer}"
	);
	assert!(code(&parse("<a bind:x={a} bind:x={b}/>"), Code::Duplicate, "x"));
	assert!(code(&parse("<a class:x class:x/>"), Code::Duplicate, "x"));
	assert!(!parse("<a on:x on:x/>").contains("\"code\""));
	// `let:` declares its name in the element's scope
	let answer = parse("<C let:x>{x}</C>");
	assert!(answer.contains("\"binding\":0") && !answer.contains("\"binding\":null"), "{answer}");
	let answer = parse("<svelte:head><title>a</title></svelte:head><title>b</title>");
	assert!(
		answer.matches("\"type\":\"TitleElement\"").count() == 1 && answer.contains("\"type\":\"SvelteHead\""),
		"{answer}"
	);
	assert!(code(&parse("<svelte:window/><svelte:window/>"), Code::Duplicate, "svelte:window"));
	assert!(code(&parse("<div><svelte:window/></div>"), Code::Placement, "svelte:window"));
}

// a repeating branch's node ends where the next branch or the close starts; after a branch read
// once it is a duplicate; what it leaves open at the end is its block
#[test]
fn a_repeating_branch_is_listed() {
	let parse = |source: &str, recover: bool| {
		let recovery = if recover { Options::ERROR_RECOVERY } else { 0 };
		svelte(source, Options::MODULE | recovery)
	};
	let answer = parse("{#if a}1{:else if b}2{:else if c}3{:else}4{/if}", false);
	assert!(
		answer.contains(r#""branches":[{"type":"ElseIf","start":8,"end":21,"#)
			&& answer.contains(r#"{"type":"ElseIf","start":21,"end":34,"#)
			&& answer.contains(r#"}],"alternate":{"type":"Fragment""#),
		"{answer}"
	);
	let answer = parse("{#if a}1{:else if b}{#if c}2{:else if d}3{/if}4{:else}5{/if}", false);
	assert!(
		!answer.contains("\"code\"") && answer.matches("\"type\":\"ElseIf\"").count() == 2,
		"{answer}"
	);
	for (source, duplicate) in [
		("{#if a}1{:else}2{:else if b}3{/if}", "{:else if}"),
		("{#if a}1{:else if b}2{:else}3{:else}4{/if}", "{:else}"),
	] {
		let answer = parse(source, false);
		assert!(
			answer.contains(&format!(r#""code":{},"args":["{duplicate}"]"#, Code::Duplicate as u8)),
			"{answer}"
		);
	}
	let answer = parse("{#if a}1{:else if b}2", true);
	assert!(
		answer
			.matches(&format!(r#""code":{},"args":["{{#if}}"]"#, Code::Unclosed as u8))
			.count() == 1
			&& answer.contains(r#""branches":[{"type":"ElseIf","start":8,"end":21,"#),
		"{answer}"
	);
	// a head that cannot be read leaves what follows it in the body still open
	let answer = parse("{#if a}x{:else if(b)}y{/if}", true);
	assert!(
		answer.contains("(b)}y") && answer.contains("\"branches\":[]"),
		"{answer}"
	);
	let answer = parse("{#if a}x{:else if b}y{:else if}z{:else}w{/if}", true);
	assert!(
		answer.contains("}z")
			&& answer.matches("\"type\":\"ElseIf\"").count() == 1
			&& answer.contains("\"data\":\"w\""),
		"{answer}"
	);
	// a branch whose node would stand too deep is refused before the body it follows closes
	let deep = format!(
		"{}x{{:else if b}}KEEP{{:else}}ALT{{/if}}{}",
		"{#if a}".repeat(999),
		"{/if}".repeat(999)
	);
	let answer = parse(&deep, true);
	assert!(
		answer.contains(&format!("\"code\":{}", Code::NestingDepth as u8)) && answer.contains("KEEP"),
		"{answer}"
	);
	// a chain's length is no depth
	let long = format!("{{#if a}}{}{{/if}}", "{:else if b}x".repeat(5000));
	let answer = parse(&long, false);
	assert!(
		!answer.contains("\"code\"") && answer.matches("\"type\":\"ElseIf\"").count() == 5000,
		"{}",
		&answer[..200]
	);
}

// a block's node has one shape whatever its component opened: a field where the first piece reading
// it stands, a body after the fields of its scope, a list where its branch stands
#[test]
fn a_block_has_one_shape() {
	let parse = |source: &str| svelte(source, Options::MODULE | Options::SCOPES);
	let ordered = |answer: &str, keys: &[&str]| {
		let at = |key: &str| {
			answer
				.find(&format!("\"{key}\":"))
				.unwrap_or_else(|| panic!("{key}: {answer}"))
		};
		keys.windows(2).all(|pair| at(pair[0]) < at(pair[1]))
	};
	for source in ["{#if a}1{/if}", "{#if a}1{:else}2{/if}", "{#if a}1{:else if b}2{/if}"] {
		let answer = parse(source);
		assert!(
			ordered(&answer, &["test", "consequent", "branches", "alternate"]),
			"{answer}"
		);
	}
	for source in [
		"{#await p}{/await}",
		"{#await p then v}{/await}",
		"{#await p catch e}{/await}",
		"{#await p}1{:then v}2{/await}",
		"{#await p}1{:then}2{:catch e}3{/await}",
		"{#await p}{:catch}{/await}",
	] {
		let answer = parse(source);
		assert!(
			ordered(&answer, &["expression", "value", "then", "error", "catch", "pending"]),
			"{answer}"
		);
	}
	// what `value` declares is seen in `then`, before which it stands, and not in `pending`
	let answer = parse("{#await p}{v}{:then v}{v}{/await}");
	assert_eq!(answer.matches("\"binding\":0").count(), 1, "{answer}");
	assert_eq!(answer.matches("\"binding\":null").count(), 2, "{answer}");
	for source in ["{#each xs as x}1{/each}", "{#each xs as x, i (k)}1{:else}2{/each}"] {
		let answer = parse(source);
		assert!(ordered(&answer, &["expression", "context", "body"]), "{answer}");
	}
}

// ── speed

// cargo test --release --test svelte component_phases -- --ignored --nocapture; TEASEL_SVELTE_BENCH=file adds a component of its own
#[test]
#[ignore]
fn component_phases() {
	let mut documents = vec![(
		"200 each blocks".to_string(),
		format!(
			"<script>let items = [1,2,3];</script>\n{}",
			"{#each items as item}<p class=\"row\">{item + 1}</p>{/each}\n".repeat(200)
		),
	)];
	if let Ok(path) = std::env::var("TEASEL_SVELTE_BENCH") {
		documents.push((path.clone(), fs::read_to_string(path).unwrap()));
	}
	for (name, source) in &documents {
		for (flags, label) in [
			(Options::MODULE, "module"),
			(
				Options::MODULE | Options::SCOPES | Options::COMMENTS,
				"module scopes comments",
			),
		] {
			let mut best = f64::MAX;
			for _ in 0..300 {
				let t = std::time::Instant::now();
				teasel::engine::parse(source, Options(flags), Entry::Svelte);
				best = best.min(t.elapsed().as_secs_f64() * 1e6);
			}
			eprintln!("{best:9.2} µs  {name} {label}");
		}
	}
}
