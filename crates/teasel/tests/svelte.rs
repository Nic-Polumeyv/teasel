//! Svelte's own tests, every `.svelte` file under `packages/svelte/tests`, read as components.
//! The repository is cloned next to this file as in ci.yml; without it the test is skipped.
//!
//! `svelte.hashes` pins the whole answer of every file under five option sets, errors included,
//! so any change to a tree fails here naming the file; `UPDATE=1` rewrites the pins once the
//! change is meant.

#[allow(dead_code)]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use teasel::Entry;
use teasel::Options;
use teasel::estree::parse;

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
fn pinned_answers() {
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
		Options::MODULE | Options::PARENTHESIZED | Options::SCOPES,
	];
	let mut list = Vec::new();
	files(&root, &root, &mut list);
	let mut lines = Vec::new();
	for (name, path) in list {
		let source = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
		let answers: Vec<String> = sets.iter().map(|&bits| parse(&source, Entry::Svelte, 0, Options(bits))).collect();
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
