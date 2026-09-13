//! A document for each rule of the host grammars beside them, its answer pinned beside it as
//! `NAME.json`: the tree with comments and scopes, the error when it has one. A name says what
//! else is on: `locations`, `erase`, or `recover` for `errorRecovery`. `UPDATE=1` rewrites the
//! pins once a change is meant.

mod common;

use std::fs;
use std::path::Path;
use teasel::Entry;
use teasel::json::{Request, parse_document};

#[test]
fn documents() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut wrong = Vec::new();
	let mut grammars: Vec<_> = fs::read_dir(root.join("tests/hosts"))
		.unwrap()
		.map(|e| e.unwrap().path())
		.collect();
	grammars.sort();
	for dir in grammars {
		let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
		let grammar = fs::read_to_string(root.join("tests/hosts").join(&name).join("host.grammar")).unwrap();
		for file in sources(&dir) {
			let stem = file.file_stem().unwrap().to_str().unwrap();
			let source = fs::read_to_string(&file).unwrap();
			let mut request = Request::new(Entry::Program, 0);
			request.set("comments");
			request.set("scopes");
			for (word, flag) in [
				("locations", "locations"),
				("erase", "erase"),
				("recover", "errorRecovery"),
			] {
				if stem.contains(word) {
					request.set(flag);
				}
			}
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
		let grammar = fs::read_to_string(root.join("tests/hosts").join(name).join("host.grammar")).unwrap();
		for file in sources(&root.join("tests/hosts").join(name)) {
			let source = fs::read_to_string(&file).unwrap();
			for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
				for recover in [false, true] {
					let mut request = Request::new(Entry::Program, 0);
					request.set("comments");
					request.set("scopes");
					if recover {
						request.set("errorRecovery");
					}
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
	let svelte = fs::read_to_string(root.join("tests/hosts/svelte/host.grammar")).unwrap();
	let vue = fs::read_to_string(root.join("tests/hosts/vue/host.grammar")).unwrap();
	let parse = |source: &str, grammar: &str, recover: bool| {
		let mut request = Request::new(Entry::Program, 0);
		request.set("comments");
		if recover {
			request.set("errorRecovery");
		}
		parse_document(source, grammar, &request)
	};
	assert!(parse("<a x=\"", &svelte, true).contains("\"type\":\"Root\""));
	let comment = parse("<a /*xx", &svelte, true);
	assert!(comment.contains("\"value\":\"xx\""), "{comment}");
	assert!(parse("<script>\"</script>", &svelte, true).contains("\"type\":\"Root\""));
	assert!(parse("<a @x=\"@\"/>", &vue, false).contains("\"error\""));
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

// cargo test --release --test hosts host_phases -- --ignored --nocapture; TEASEL_HOST_BENCH=file adds a document of its own
#[test]
#[ignore]
fn host_phases() {
	use teasel::json::Prepared;
	let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = fs::read_to_string(root.join("tests/hosts/svelte/host.grammar")).unwrap();
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
		for flags in ["module", "module scopes comments locations"] {
			let prepared = Prepared::borrowed(source, Request::from_names(flags))
				.host(&grammar)
				.unwrap();
			let mut best = f64::MAX;
			for _ in 0..300 {
				let t = std::time::Instant::now();
				prepared.binary(Entry::Program, 0.0, None, "").unwrap();
				best = best.min(t.elapsed().as_secs_f64() * 1e6);
			}
			eprintln!("{best:9.2} µs  {name} {flags}");
		}
	}
}

/// The documents of a host directory: not its grammar, its plan or the pins.
fn sources(dir: &Path) -> Vec<std::path::PathBuf> {
	common::inputs(dir)
		.into_iter()
		.filter(|f| f.file_stem().is_some_and(|s| s != "plan") && f.extension().is_some_and(|e| e != "grammar"))
		.collect()
}
