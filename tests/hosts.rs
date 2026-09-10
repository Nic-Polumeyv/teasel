//! A document for each rule of the grammars under `hosts/`, its answer pinned beside it as
//! `NAME.json`: the tree with comments and scopes, the error when it has one. A name says what
//! else is on: `locations`, `erase`, or `recover` for `errorRecovery`. `UPDATE=1` rewrites the
//! pins once a change is meant.

use std::fs;
use std::path::Path;
use teasel::Entry;
use teasel::json::{Request, parse_document};

/// The compact answer laid out one value per line, so a pin reads and diffs.
fn pretty(json: &str) -> String {
	let mut out = String::with_capacity(json.len() * 2);
	let mut depth = 0usize;
	let mut chars = json.chars().peekable();
	let newline = |out: &mut String, depth: usize| {
		out.push('\n');
		out.extend(std::iter::repeat_n('\t', depth));
	};
	while let Some(c) = chars.next() {
		match c {
			'"' => {
				out.push('"');
				while let Some(c) = chars.next() {
					out.push(c);
					match c {
						'\\' => out.push(chars.next().unwrap()),
						'"' => break,
						_ => {}
					}
				}
			}
			'{' | '[' => {
				out.push(c);
				let closer = if c == '{' { '}' } else { ']' };
				if chars.peek() == Some(&closer) {
					out.push(chars.next().unwrap());
				} else {
					depth += 1;
					newline(&mut out, depth);
				}
			}
			'}' | ']' => {
				depth -= 1;
				newline(&mut out, depth);
				out.push(c);
			}
			',' => {
				out.push(c);
				newline(&mut out, depth);
			}
			':' => out.push_str(": "),
			_ => out.push(c),
		}
	}
	out.push('\n');
	out
}

#[test]
fn documents() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let update = std::env::var_os("UPDATE").is_some();
	let mut wrong = Vec::new();
	let mut grammars: Vec<_> = fs::read_dir(root.join("tests/hosts"))
		.unwrap()
		.map(|e| e.unwrap().path())
		.collect();
	grammars.sort();
	for dir in grammars {
		let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
		let grammar = fs::read_to_string(root.join("hosts").join(format!("{name}.grammar"))).unwrap();
		let mut files: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
		files.sort();
		for file in files.into_iter().filter(|f| f.extension().is_some_and(|e| e != "json")) {
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
			let answer = pretty(&parse_document(&source, &grammar, &request));
			let pin = file.with_extension("json");
			if update {
				fs::write(&pin, &answer).unwrap();
			} else if fs::read_to_string(&pin).ok().as_deref() != Some(answer.as_str()) {
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
