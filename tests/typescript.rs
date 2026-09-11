//! TypeScript's own conformance cases on TypeScript's terms: a case with a clean baseline parses,
//! a case whose baseline reports a grammar error (`TS1xxx`, less the ones only the checker can
//! see) does not; what TypeScript reports from its binder, a redeclaration say, the parser may
//! reject early as ECMAScript does. The repository is
//! checked out sparsely next to this file; without it the test is skipped:
//!
//!   git clone --depth 1 --filter=blob:none --sparse https://github.com/microsoft/TypeScript tests/TypeScript
//!   git -C tests/TypeScript sparse-checkout set --no-cone '/tsc/testdata/tests/cases/conformance/**' '/tsc/testdata/baselines/reference/conformance/*.errors.txt'
//!
//! A case with several files (`// @filename`) or a `.d.ts` file is left out.

use std::fs;
use std::path::{Path, PathBuf};
use teasel::Entry;
use teasel::json::{Request, parse};

/// Grammar diagnostics (`TS1xxx`) only the checker can see, or where ECMAScript decides otherwise.
const CHECKER: &[(&str, &str)] = &[
	(
		"1046",
		"top-level declarations of a .d.ts file: the file's kind, not its syntax",
	),
	("1058", "async return types"),
	("1064", "async return types"),
	("1320", "await operand types"),
	("1100", "strict mode set by compiler options, not by the source"),
	("1101", "strict mode set by compiler options, not by the source"),
	("1102", "strict mode set by compiler options, not by the source"),
	("1121", "strict mode set by compiler options, not by the source"),
	("1212", "strict mode set by compiler options, not by the source"),
	("1117", "duplicate object literal properties, allowed since ES2015"),
	("1118", "duplicate object literal accessors, allowed since ES2015"),
	("1166", "computed property names of literal type: a type"),
	("1169", "computed property names of literal type: a type"),
	("1170", "computed property names of literal type: a type"),
	(
		"1189",
		"an initializer in a for-in head: Annex B allows it in sloppy code",
	),
	("1202", "import assignments under an ES module target: an option"),
	("1203", "export assignments under an ES module target: an option"),
	("1207", "decorator signatures"),
	("1238", "decorator signatures"),
	("1239", "resolving a parameter decorator's call signature"),
	("1240", "decorator signatures"),
	("1241", "decorator signatures"),
	("1329", "decorator signatures"),
	("1268", "index signature parameter types"),
	("1345", "truthiness of void"),
	("17009", "super before this"),
	("17011", "super before this"),
	("1270", "decorator signatures"),
	("1360", "satisfies against a type"),
	("1288", "verbatimModuleSyntax: an option"),
	("1319", "a default export under a module kind: an option"),
	("1337", "index signature parameter types"),
	("1489", "a decimal with leading zeros: Annex B allows it in sloppy code"),
];

/// Grammar the parser does not check yet.
const NOT_YET: &[(&str, &str)] = &[
	(
		"1206",
		"decorators on parameters, private names, class expressions, abstract and declare members: an option the runner does not read",
	),
	("1228", "type predicates outside return types"),
	("1338", "infer outside a conditional's extends clause"),
	("1308", "await in an enum initializer"),
	("1163", "yield in an enum initializer"),
	("1355", "what a const assertion may apply to"),
	("1196", "the type a catch variable may have"),
	("1063", "an export assignment in a namespace"),
	("1035", "a quoted module name without declare"),
	("1344", "a label on a declaration"),
	("1213", "strict-mode reserved words as type names in a class"),
	("1242", "abstract on what is not a class"),
];

/// Cases decided by name: what the parser rejects on ECMAScript's rules, which TypeScript does
/// not apply, and what it does not read yet.
const BY_NAME: &[(&str, &str)] = &[
	(
		"asyncWithVarShadowing_es6.ts",
		"a var may not redeclare a destructured catch parameter",
	),
	("topLevelVarHoistingCommonJS.ts", "with in a module"),
	(
		"esDecorators-decoratorExpression.2.ts",
		"a decorator expression is what the proposal allows",
	),
	("topLevelAwait.2.ts", "await as a name in a module"),
	(
		"functionWithUseStrictAndSimpleParameterList.ts",
		"'use strict' in a function with non-simple parameters",
	),
	(
		"exportNonInitializedVariablesSystem.ts",
		"no baseline for the module option",
	),
	(
		"exportNonInitializedVariablesUMD.ts",
		"no baseline for the module option",
	),
	(
		"parservoidInQualifiedName1.ts",
		"void in a qualified type name is not read yet",
	),
];

/// A file TypeScript treats as a module: one with an import or export of its own, or `import.meta`.
fn is_module(source: &str) -> bool {
	source.contains("import.meta")
		|| source.lines().any(|line| {
			let line = line.trim_start();
			["import", "export"].iter().any(|word| {
				line.strip_prefix(word)
					.is_some_and(|after| !after.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$'))
			})
		})
}

fn has_several_files(source: &str) -> bool {
	source.lines().any(|line| {
		line.trim_start()
			.strip_prefix("//")
			.is_some_and(|after| after.trim_start().to_ascii_lowercase().starts_with("@filename"))
	})
}

fn cases(root: &Path, out: &mut Vec<PathBuf>) {
	for entry in fs::read_dir(root).unwrap() {
		let path = entry.unwrap().path();
		if path.is_dir() {
			cases(&path, out);
		} else if path.extension().is_some_and(|e| e == "ts") {
			out.push(path);
		}
	}
}

#[test]
fn conformance() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/TypeScript/tsc/testdata");
	if !root.exists() {
		eprintln!("skipped: no tests/TypeScript");
		return;
	}
	let baselines = root.join("baselines/reference/conformance");
	let mut files = Vec::new();
	cases(&root.join("tests/cases/conformance"), &mut files);
	files.sort();
	let mut wrong = Vec::new();
	let mut counted = 0;
	let mut left_out = 0;
	for file in files {
		let source = String::from_utf8_lossy(&fs::read(&file).unwrap()).into_owned();
		let name = file.file_name().unwrap().to_str().unwrap();
		// a .d.ts file is ambient by its name, which the parser is not told
		if name.ends_with(".d.ts") || has_several_files(&source) || BY_NAME.iter().any(|(n, _)| *n == name) {
			left_out += 1;
			continue;
		}
		let stem = file.file_stem().unwrap().to_str().unwrap();
		let mut codes = Vec::new();
		for entry in fs::read_dir(&baselines).unwrap() {
			let entry = entry.unwrap();
			let name = entry.file_name().into_string().unwrap();
			if name == format!("{stem}.errors.txt")
				|| (name.starts_with(&format!("{stem}(")) && name.ends_with(".errors.txt"))
			{
				let text = fs::read_to_string(entry.path()).unwrap();
				for (i, _) in text.match_indices("error TS") {
					let code: String = text[i + 8..].chars().take_while(|c| c.is_ascii_digit()).collect();
					codes.push(code);
				}
			}
		}
		// TypeScript reports early errors from its binder and checker, redeclarations say, so a
		// rejection is wrong only when the baseline is clean; a parse is wrong when the baseline has
		// a grammar diagnostic the parser could see
		let excused = |c: &String| CHECKER.iter().chain(NOT_YET).any(|(code, _)| code == c);
		let grammar_error = codes.iter().any(|c| c.starts_with('1') && c.len() == 4 && !excused(c));
		let any_error = !codes.is_empty();
		let mut request = Request::new(Entry::Program, 0);
		request.set("typescript");
		request.options.module = is_module(&source);
		let answer = parse(&source, &request, "");
		let failed = answer.starts_with("{\"error\"");
		counted += 1;
		if (failed && !any_error) || (!failed && grammar_error) {
			let what = if failed {
				answer[..answer.len().min(160)].to_owned()
			} else {
				format!(
					"parses, baseline says {}",
					codes
						.iter()
						.filter(|c| c.starts_with('1'))
						.cloned()
						.collect::<Vec<_>>()
						.join(",")
				)
			};
			wrong.push(format!("{}: {what}", file.strip_prefix(&root).unwrap().display()));
		}
	}
	eprintln!("{left_out} cases left out");
	eprintln!("{counted} cases");
	assert!(wrong.is_empty(), "{} disagree:\n{}", wrong.len(), wrong.join("\n"));
}
