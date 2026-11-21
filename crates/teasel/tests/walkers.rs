//! The walker written for Svelte against the Svelte grammar's: every fixture of the grammar and
//! every prefix of each, under several option sets, answer the same JSON, errors and all.
//! `npm/scripts/walkers.ts` runs the same comparison over Svelte's own tests.

#[allow(dead_code)]
mod common;

use std::fs;
use std::path::Path;
use teasel::Entry;
use teasel::Options;
use teasel::estree::{parse, parse_document};
use teasel::host::Grammar;

fn grammar() -> String {
	fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hosts/svelte/host.json")).unwrap()
}

/// The first line where two answers part, pretty printed, for a message that reads.
fn first_difference(old: &str, new: &str) -> String {
	let (old, new) = (common::pretty(old), common::pretty(new));
	let line = old.lines().zip(new.lines()).position(|(a, b)| a != b).unwrap_or(0);
	format!(
		"line {line}: {:?} vs {:?}",
		old.lines().nth(line).unwrap_or(""),
		new.lines().nth(line).unwrap_or("")
	)
}

#[test]
fn the_names_and_children_are_the_grammars() {
	let grammar = Grammar::read(Box::leak(grammar().into_boxed_str())).unwrap();
	assert_eq!(grammar.names, teasel::host::svelte::NAMES);
	assert_eq!(grammar.children(), teasel::host::svelte::children());
}

#[test]
fn every_fixture_answers_as_the_grammar() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = grammar();
	let sets = [
		0,
		Options::COMMENTS | Options::SCOPES,
		Options::ERROR_RECOVERY | Options::COMMENTS | Options::SCOPES,
		Options::ERASE | Options::COMMENTS | Options::SCOPES,
		Options::MODULE | Options::PARENTHESIZED | Options::SCOPES,
	];
	let mut wrong = Vec::new();
	for file in common::inputs(&root.join("tests/hosts/svelte")) {
		let source = fs::read_to_string(&file).unwrap();
		for bits in sets {
			let old = parse_document(&source, &grammar, Options(bits));
			let new = parse(&source, Entry::Svelte, 0, Options(bits));
			if old != new {
				wrong.push(format!("{} with {bits:#x}: {}", file.display(), first_difference(&old, &new)));
			}
		}
	}
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_prefix_answers_as_the_grammar() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let grammar = grammar();
	let mut wrong = Vec::new();
	for file in common::inputs(&root.join("tests/hosts/svelte")) {
		let source = fs::read_to_string(&file).unwrap();
		for bits in [
			Options::COMMENTS | Options::SCOPES,
			Options::ERROR_RECOVERY | Options::COMMENTS | Options::SCOPES,
		] {
			for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
				let cut = &source[..end];
				let old = parse_document(cut, &grammar, Options(bits));
				let new = parse(cut, Entry::Svelte, 0, Options(bits));
				if old != new {
					wrong.push(format!("{} cut at {end} with {bits:#x}: {}", file.display(), first_difference(&old, &new)));
					break;
				}
			}
		}
	}
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
