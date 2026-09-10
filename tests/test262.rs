//! tc39/test262-parser-tests on its own terms: everything under pass and pass-explicit parses,
//! everything under fail and early does not. The suite is cloned next to this file with
//! `git clone --depth 1 https://github.com/tc39/test262-parser-tests tests/test262-parser-tests`;
//! without it the test is skipped.

use std::fs;
use std::path::Path;
use teasel::{Entry, Options, parse_at};

/// Where the suite predates the language: `\8` and `\9` in sloppy strings (Annex B, ES2021),
/// class fields (ES2022), `for (var x = 1 in o)` (Annex B.3.5) and a function declared twice in
/// a sloppy block (Annex B.3.3.4).
const SUPERSEDED: &[&str] = &[
	"fail/0d5e450f1da8a92a.js",
	"fail/748656edbfb2d0bb.js",
	"fail/79f882da06f88c9f.js",
	"fail/92b6af54adef3624.js",
	"fail/98204d734f8c72b3.js",
	"fail/ef81b93cf9bdb4ec.js",
	"fail/e3fbcf63d7e43ead.js",
	"early/12a74c60f52a60de.js",
	"early/1aff49273f3e3a98.js",
	"early/be7329119eaa3d47.js",
	"early/ec31fa5e521c5df4.js",
];

#[test]
fn test262_parser_tests() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test262-parser-tests");
	if !root.exists() {
		eprintln!("skipped: no tests/test262-parser-tests");
		return;
	}
	let mut wrong = Vec::new();
	for dir in ["pass", "pass-explicit", "fail", "early"] {
		let mut names: Vec<String> = fs::read_dir(root.join(dir))
			.unwrap()
			.map(|entry| entry.unwrap().file_name().into_string().unwrap())
			.collect();
		names.sort();
		for name in names {
			let source = String::from_utf8_lossy(&fs::read(root.join(dir).join(&name)).unwrap()).into_owned();
			let options = Options {
				module: name.contains(".module."),
				..Options::default()
			};
			let parses = parse_at(&source, 0, None, Entry::Program, options, "").is_ok();
			let path = format!("{dir}/{name}");
			let superseded = SUPERSEDED.contains(&path.as_str());
			if parses != dir.starts_with("pass") && !superseded {
				wrong.push(path);
			} else if parses == dir.starts_with("pass") && superseded {
				wrong.push(format!("{path} is no longer superseded"));
			}
		}
	}
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
