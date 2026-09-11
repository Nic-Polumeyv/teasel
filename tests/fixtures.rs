//! A file for each feature of the language under `tests/fixtures/js` and `tests/fixtures/ts`,
//! its answer pinned beside it as `NAME.json`: the tree with comments and scopes, or the error.
//! A file is a module unless its name ends in `.script.js` or `.script.ts`; a name says what else is on:
//! `locations`, `erase`, `recover` for `errorRecovery`, or `legacy` and `proposal` for decorators.
//! `UPDATE=1` rewrites the pins once a change is meant.

mod common;

use std::fs;
use std::path::Path;
use teasel::Entry;
use teasel::json::{Request, parse};

#[test]
fn files() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
	let mut wrong = Vec::new();
	for language in ["js", "ts"] {
		for file in common::inputs(&root.join(language)) {
			let name = file.file_name().unwrap().to_str().unwrap();
			let stem = name.split('.').next().unwrap();
			let source = fs::read_to_string(&file).unwrap();
			let mut request = Request::new(Entry::Program, 0);
			request.options.module = !name.contains(".script.");
			request.set("comments");
			request.set("scopes");
			if language == "ts" {
				request.set("typescript");
			}
			for (word, flag) in [
				("locations", "locations"),
				("erase", "erase"),
				("recover", "errorRecovery"),
				("legacy", "legacyDecorators"),
				("proposal", "proposalDecorators"),
			] {
				if stem.contains(word) {
					request.set(flag);
				}
			}
			let answer = common::pretty(&parse(&source, &request, ""));
			if !common::pinned(&file.with_extension("json"), &answer) {
				wrong.push(format!("{language}/{name}"));
			}
		}
	}
	assert!(
		wrong.is_empty(),
		"answers changed for:\n{}\nrun with UPDATE=1 once the change is meant",
		wrong.join("\n")
	);
}
