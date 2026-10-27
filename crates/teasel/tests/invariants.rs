//! What must hold between the parser's own answers, over every file of test262-parser-tests that
//! parses: recovery changes nothing about a file that parses, an entry reads the same statement
//! the program did, and a file cut anywhere is read without a panic. Over every file that does not
//! parse, a cut is read without a panic too. The erase answer lists kept nodes in source order,
//! and no child ends past its parent.

use std::fs;
use std::path::{Path, PathBuf};
use teasel::Entry;
use teasel::Options;
use teasel::ast::{Ast, List, Walk};
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

fn request(entry: Entry, offset: u32, module: bool, flags: u32) -> Request {
	let module = if module { Options::MODULE } else { 0 };
	Request {
		entry,
		offset,
		..Request::from_flags(module | flags)
	}
}

#[test]
fn recovery_changes_nothing_that_parses() {
	let Some(root) = suite() else { return };
	let mut differ = Vec::new();
	for (path, source, module) in parsing(&root) {
		let flags = Options::COMMENTS | Options::SCOPES | Options::LOCATIONS;
		let strict = parse(&source, &request(Entry::Program, 0, module, flags));
		let mut recovered = parse(
			&source,
			&request(Entry::Program, 0, module, Options::ERROR_RECOVERY | flags),
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

/// The `node` of an answer, `{"node":...,"end":N}`; none for a failed parse.
fn node_of(answer: &str) -> Option<&str> {
	let rest = answer.strip_prefix("{\"node\":{")?;
	let end = rest.rfind(",\"end\":")?;
	Some(&answer[8..9 + end])
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
		let program = parse(&source, &request(Entry::Program, 0, module, Options::LOCATIONS));
		let mut at = 0u32;
		while (at as usize) < source.len() {
			let answer = parse(&source, &request(Entry::Statement, at, module, Options::LOCATIONS));
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
			for flags in [
				0,
				Options::ERROR_RECOVERY | Options::COMMENTS | Options::SCOPES | Options::LOCATIONS,
			] {
				let read = std::panic::catch_unwind(|| parse(cut, &request(Entry::Program, 0, module, flags)));
				if read.is_err() {
					panicked.push(format!("{path} cut at {i} with {flags:?}"));
				}
			}
		}
	}
	assert!(panicked.is_empty(), "{}", panicked.join("\n"));
}

fn kept(json: &str) -> Vec<(u32, u32)> {
	let list = json.split("\"typescript\":[").nth(1).expect("no typescript list");
	let list = &list[..list.find(']').unwrap()];
	let numbers = |key: &str| -> Vec<u32> {
		list.match_indices(key)
			.map(|(at, _)| {
				list[at + key.len()..]
					.split(|c: char| !c.is_ascii_digit())
					.next()
					.unwrap()
					.parse()
					.unwrap()
			})
			.collect()
	};
	numbers("\"start\":").into_iter().zip(numbers("\"end\":")).collect()
}

// a node comes before what it contains: a field precedes the decorators that share its start
#[test]
fn kept_nodes_are_in_source_order() {
	let request = Request::from_flags(Options::MODULE | Options::TYPESCRIPT | Options::ERASE);
	let answer = parse("class C { @a @b accessor x = 1; }", &request);
	assert_eq!(kept(&answer), [(10, 31), (10, 12), (13, 15)]);
}

/// Every node reached from `roots` that ends past its parent.
fn escaped<X: Walk>(path: &str, ast: &Ast<X>, roots: List, out: &mut Vec<String>) {
	let mut stack: Vec<_> = ast.list(roots).iter().flatten().copied().collect();
	let mut children = Vec::new();
	while let Some(id) = stack.pop() {
		let parent = ast.node(id);
		children.clear();
		ast.children(id, &mut children);
		for &child in &children {
			let node = ast.node(child);
			if node.end > parent.end {
				out.push(format!(
					"{path}: {}..{} inside {}..{}",
					node.start, node.end, parent.start, parent.end
				));
			}
		}
		stack.extend_from_slice(&children);
	}
}

// comment attachment hands a node's trailing comments to its last child by this
#[test]
fn no_child_ends_past_its_parent() {
	let Some(root) = suite() else { return };
	let mut wrong = Vec::new();
	for (path, source, module) in parsing(&root) {
		let options = Options(if module { Options::MODULE } else { 0 });
		if let (ast, Ok((roots, _))) = teasel::parse_at(&source, 0, None, Entry::Program, options, "") {
			escaped(&path, &ast, roots, &mut wrong);
		}
	}
	let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ts");
	for entry in fs::read_dir(fixtures).unwrap() {
		let file = entry.unwrap().path();
		let name = file.file_name().unwrap().to_str().unwrap();
		if !name.ends_with(".ts") {
			continue;
		}
		let source = fs::read_to_string(&file).unwrap();
		let module = if name.contains(".script.") { 0 } else { Options::MODULE };
		let options = Options(module | Options::TYPESCRIPT);
		if let (ast, Ok((roots, _))) = teasel::typescript::parse_at(&source, 0, None, Entry::Program, options, "") {
			escaped(name, &ast, roots, &mut wrong);
		}
	}
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
