//! What must hold between the parser's own answers, over every file of test262-parser-tests that
//! parses: recovery changes nothing about a file that parses, an entry reads the same statement
//! the program did, and a file cut anywhere is read without a panic.

use std::fs;
use std::path::{Path, PathBuf};
use teasel::Entry;
use teasel::json::{Request, parse};

fn suite() -> Option<PathBuf> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test262-parser-tests");
	if root.exists() {
		Some(root)
	} else {
		eprintln!("skipped: no tests/test262-parser-tests");
		None
	}
}

/// Every file that parses, sorted, each with its source and whether it is a module.
fn parsing(root: &Path) -> Vec<(String, String, bool)> {
	let mut out = Vec::new();
	for dir in ["pass", "pass-explicit"] {
		let mut names: Vec<String> = fs::read_dir(root.join(dir))
			.unwrap()
			.map(|entry| entry.unwrap().file_name().into_string().unwrap())
			.collect();
		names.sort();
		for name in names {
			let source = String::from_utf8_lossy(&fs::read(root.join(dir).join(&name)).unwrap()).into_owned();
			out.push((format!("{dir}/{name}"), source, name.contains(".module.")));
		}
	}
	out
}

fn request(entry: Entry, offset: u32, module: bool, flags: &[&str]) -> Request {
	let mut request = Request::new(entry, offset);
	request.options.module = module;
	for flag in flags {
		request.set(flag);
	}
	request
}

#[test]
fn recovery_changes_nothing_that_parses() {
	let Some(root) = suite() else { return };
	let mut differ = Vec::new();
	for (path, source, module) in parsing(&root) {
		let flags = ["comments", "scopes", "locations"];
		let strict = parse(&source, &request(Entry::Program, 0, module, &flags), "");
		let mut recovered = parse(
			&source,
			&request(
				Entry::Program,
				0,
				module,
				&["errorRecovery", "comments", "scopes", "locations"],
			),
			"",
		);
		if let Some(at) = recovered.find(",\"errors\":[]") {
			recovered.replace_range(at..at + ",\"errors\":[]".len(), "");
		}
		if strict != recovered {
			differ.push(path);
		}
	}
	assert!(differ.is_empty(), "{}", differ.join("\n"));
}

/// The `node` of an entry's answer, `{"node":...,"end":N}`.
fn node_of(answer: &str) -> Option<&str> {
	let rest = answer.strip_prefix("{\"node\":")?;
	let end = rest.rfind(",\"end\":")?;
	Some(&rest[..end])
}

/// The byte offset of a UTF-16 offset, which is how the answers count.
fn byte_at(source: &str, utf16: u32) -> u32 {
	let mut units = 0u32;
	for (i, c) in source.char_indices() {
		if units >= utf16 {
			return i as u32;
		}
		units += c.len_utf16() as u32;
	}
	source.len() as u32
}

#[test]
fn an_entry_reads_what_the_program_read() {
	let Some(root) = suite() else { return };
	let mut differ = Vec::new();
	for (path, source, module) in parsing(&root) {
		let program = parse(&source, &request(Entry::Program, 0, module, &["locations"]), "");
		let mut at = 0u32;
		while (at as usize) < source.len() {
			let answer = parse(&source, &request(Entry::Statement, at, module, &["locations"]), "");
			let Some(node) = node_of(&answer) else { break };
			let end: u32 = answer[answer.rfind(",\"end\":").unwrap() + 7..answer.len() - 1]
				.parse()
				.unwrap();
			// a directive is what a statement is in the prologue: the entry cannot know that
			let prologue = &node[..node.len() - 1];
			if !program.contains(node) && !program.contains(&format!("{prologue},\"directive\":")) {
				differ.push(format!("{path} at {at}: {}", &node[..node.len().min(120)]));
			}
			let next = byte_at(&source, end);
			if next <= at {
				break;
			}
			at = next;
		}
	}
	assert!(
		differ.is_empty(),
		"{} statements read differently:\n{}",
		differ.len(),
		differ.join("\n")
	);
}

#[test]
fn a_file_cut_anywhere_is_read_without_a_panic() {
	let Some(root) = suite() else { return };
	let mut panicked = Vec::new();
	for (path, source, module) in parsing(&root) {
		for (i, byte) in source.bytes().enumerate() {
			if byte != b'\n' {
				continue;
			}
			let cut = &source[..i];
			for flags in [&[][..], &["errorRecovery", "comments", "scopes", "locations"][..]] {
				let read = std::panic::catch_unwind(|| parse(cut, &request(Entry::Program, 0, module, flags), ""));
				if read.is_err() {
					panicked.push(format!("{path} cut at {i} with {flags:?}"));
				}
			}
		}
	}
	assert!(panicked.is_empty(), "{}", panicked.join("\n"));
}
