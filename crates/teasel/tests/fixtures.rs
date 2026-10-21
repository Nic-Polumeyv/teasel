//! A file for each feature of the language under `tests/fixtures/js` and `tests/fixtures/ts`, a
//! stylesheet under `tests/fixtures/css`, its answer pinned beside it as `NAME.json`: the tree
//! with comments and scopes, or the error.
//! A file is a module unless its name ends in `.script.js` or `.script.ts`; a name says what else is on:
//! `locations`, `erase`, `recover` for `errorRecovery`, or `legacy` and `proposal` for decorators.
//! `UPDATE=1` rewrites the pins once a change is meant.

mod common;

use std::fs;
use std::path::Path;
use teasel::json::{Request, parse};
use teasel::{Entry, Options};

// a misspelled or new type, key or enum text is a line of this pin's diff, as `c!` once failed the build on one
#[test]
fn spellings() {
	let layout = teasel::json::layout_json();
	assert!(!layout.contains('\\'), "a string the split below would cut");
	let mut names: Vec<&str> = layout.split('"').skip(1).step_by(2).collect();
	names.sort_unstable();
	names.dedup();
	let pin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/spellings.txt");
	assert!(
		common::pinned(&pin, &(names.join("\n") + "\n")),
		"spellings changed: run with UPDATE=1 once the change is meant"
	);
}

#[test]
fn files() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
	let mut wrong = Vec::new();
	for language in ["js", "ts", "css"] {
		for file in common::inputs(&root.join(language)) {
			let name = file.file_name().unwrap().to_str().unwrap();
			let stem = name.split('.').next().unwrap();
			let source = fs::read_to_string(&file).unwrap();
			let mut flags = Options::COMMENTS | Options::SCOPES;
			if !name.contains(".script.") {
				flags |= Options::MODULE;
			}
			if language == "ts" {
				flags |= Options::TYPESCRIPT;
			}
			for (word, bit) in [
				("locations", Options::LOCATIONS),
				("erase", Options::ERASE),
				("recover", Options::ERROR_RECOVERY),
			] {
				if stem.contains(word) {
					flags |= bit;
				}
			}
			let request = Request {
				entry: if language == "css" {
					Entry::StyleSheet
				} else {
					Entry::Program
				},
				..Request::from_flags(flags)
			};
			let answer = common::pretty(&parse(&source, &request));
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
