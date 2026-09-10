//! tc39/test262-parser-tests on its own terms: everything under pass and pass-explicit parses,
//! everything under fail and early does not. The suite is cloned next to this file with
//! `git clone --depth 1 https://github.com/tc39/test262-parser-tests tests/test262-parser-tests`;
//! without it both tests are skipped.
//!
//! `test262.hashes` pins the whole answer for every file that parses, comments, scopes and
//! locations included, so any change to a tree fails here naming the file; `UPDATE=1` rewrites
//! the pins once the change is meant.

use std::fs;
use std::path::{Path, PathBuf};
use teasel::json::{Request, parse};
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

fn suite() -> Option<PathBuf> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test262-parser-tests");
	if root.exists() {
		Some(root)
	} else {
		eprintln!("skipped: no tests/test262-parser-tests");
		None
	}
}

/// The files of a directory of the suite, sorted, each with its source.
fn files(root: &Path, dir: &str) -> Vec<(String, String)> {
	let mut names: Vec<String> = fs::read_dir(root.join(dir))
		.unwrap()
		.map(|entry| entry.unwrap().file_name().into_string().unwrap())
		.collect();
	names.sort();
	names
		.into_iter()
		.map(|name| {
			let source = String::from_utf8_lossy(&fs::read(root.join(dir).join(&name)).unwrap()).into_owned();
			(format!("{dir}/{name}"), source)
		})
		.collect()
}

fn is_module(path: &str) -> bool {
	path.contains(".module.")
}

#[test]
fn verdicts() {
	let Some(root) = suite() else { return };
	let mut wrong = Vec::new();
	for dir in ["pass", "pass-explicit", "fail", "early"] {
		for (path, source) in files(&root, dir) {
			let options = Options {
				module: is_module(&path),
				..Options::default()
			};
			let parses = parse_at(&source, 0, None, Entry::Program, options, "").is_ok();
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

/// FNV-1a over the answer's bytes: no dependency, and a change anywhere in the tree shows.
fn hash(text: &str) -> u64 {
	text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
		(h ^ u64::from(b)).wrapping_mul(0x100000001b3)
	})
}

#[test]
fn pinned_answers() {
	let Some(root) = suite() else { return };
	let pins = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test262.hashes");
	let mut lines = Vec::new();
	for dir in ["pass", "pass-explicit"] {
		for (path, source) in files(&root, dir) {
			let mut request = Request::new(Entry::Program, 0);
			request.options.module = is_module(&path);
			for flag in ["comments", "scopes", "locations"] {
				request.set(flag);
			}
			lines.push(format!("{path} {:016x}", hash(&parse(&source, &request, ""))));
		}
	}
	let now = lines.join("\n") + "\n";
	if std::env::var_os("UPDATE").is_some() {
		fs::write(&pins, &now).unwrap();
		return;
	}
	let pinned = fs::read_to_string(&pins).unwrap_or_else(|_| panic!("no {}: run with UPDATE=1", pins.display()));
	let changed: Vec<&str> = now
		.lines()
		.zip(pinned.lines().chain(std::iter::repeat("")))
		.filter(|(a, b)| a != b)
		.map(|(a, _)| a.split(' ').next().unwrap())
		.collect();
	assert!(
		changed.is_empty(),
		"answers changed for:\n{}\nrun with UPDATE=1 once the change is meant",
		changed.join("\n")
	);
}
