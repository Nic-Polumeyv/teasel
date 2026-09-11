//! A document for each rule of the grammars under `hosts/`, its answer pinned beside it as
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
		let grammar = fs::read_to_string(root.join("hosts").join(format!("{name}.grammar"))).unwrap();
		for file in common::inputs(&dir) {
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

// cargo test --release --test hosts host_phases -- --ignored --nocapture; TEASEL_HOST_BENCH=file adds a document of its own
#[test]
#[ignore]
fn host_phases() {
	use teasel::json::Prepared;
	let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = fs::read_to_string(root.join("hosts/svelte.grammar")).unwrap();
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
