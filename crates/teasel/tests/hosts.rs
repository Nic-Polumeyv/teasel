//! A document for each rule of the host plans beside them, its answer pinned beside it as
//! `NAME.json`: the tree with comments and scopes, the error when it has one. A name says what
//! else is on: `locations`, `erase`, or `recover` for `errorRecovery`. `UPDATE=1` rewrites the
//! pins once a change is meant.

mod common;

use std::fs;
use std::path::Path;
use teasel::Entry;
use teasel::json::{Request, parse_document};

#[test]
fn documents() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut wrong = Vec::new();
	let mut plans: Vec<_> = fs::read_dir(root.join("tests/hosts"))
		.unwrap()
		.map(|e| e.unwrap().path())
		.filter(|p| p.is_dir())
		.collect();
	plans.sort();
	for dir in plans {
		let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
		let plan = fs::read_to_string(root.join("tests/hosts").join(&name).join("plan.json")).unwrap();
		for file in sources(&dir) {
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
			let answer = common::pretty(&parse_document(&source, &plan, &request));
			if !common::pinned(&file.with_extension("json"), &answer) {
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

/// Every prefix of every host document, strict and recovering: unfinished input is an answer or
/// an error, never a panic.
#[test]
fn every_prefix_answers() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	for name in ["svelte", "vue"] {
		let plan = fs::read_to_string(root.join("tests/hosts").join(name).join("plan.json")).unwrap();
		for file in sources(&root.join("tests/hosts").join(name)) {
			let source = fs::read_to_string(&file).unwrap();
			for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
				for recover in [false, true] {
					let mut request = Request::new(Entry::Program, 0);
					request.set("comments");
					request.set("scopes");
					if recover {
						request.set("errorRecovery");
					}
					parse_document(&source[..end], &plan, &request);
				}
			}
		}
	}
}

/// The unfinished and odd inputs the audit found panicking, and the fallback from one expression
/// to statements, which must not settle for what recovery makes of the expression.
#[test]
fn unfinished_input() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let svelte = fs::read_to_string(root.join("tests/hosts/svelte/plan.json")).unwrap();
	let vue = fs::read_to_string(root.join("tests/hosts/vue/plan.json")).unwrap();
	let parse = |source: &str, plan: &str, recover: bool| {
		let mut request = Request::new(Entry::Program, 0);
		request.set("comments");
		if recover {
			request.set("errorRecovery");
		}
		parse_document(source, plan, &request)
	};
	assert!(parse("<a x=\"", &svelte, true).contains("\"type\":\"Root\""));
	let comment = parse("<a /*xx", &svelte, true);
	assert!(comment.contains("\"value\":\"xx\""), "{comment}");
	assert!(parse("<script>\"</script>", &svelte, true).contains("\"type\":\"Root\""));
	assert!(parse("<a @x=\"@\"/>", &vue, false).contains("\"error\""));
	let elements = format!("{}x{}", "<a>".repeat(40_000), "</a>".repeat(40_000));
	let branches = format!("{{#if a}}{}{{/if}}", "{:else if a}".repeat(40_000));
	// the executor recurses per nested record; debug frames need more than a test thread's stack
	std::thread::Builder::new()
		.stack_size(64 << 20)
		.spawn(move || {
			for (open, close) in [("<a>", "</a>"), ("{#if a}", "{/if}")] {
				let source = format!("{}x{}", open.repeat(1000), close.repeat(1000));
				let answer = parse(&source, &svelte, false);
				assert!(answer.contains("\"type\":\"Root\""), "{answer}");
				let source = format!("{}x{}", open.repeat(1001), close.repeat(1001));
				let answer = parse(&source, &svelte, false);
				assert!(answer.contains("\"code\":\"nesting_depth\""), "{answer}");
			}
			for deep in [elements, branches] {
				let answer = parse(&deep, &svelte, false);
				assert!(answer.contains("\"code\":\"nesting_depth\""), "{answer}");
			}
		})
		.unwrap()
		.join()
		.unwrap();
	let program = parse("<button @click=\"let x = 1\"/>", &vue, true);
	assert!(
		program.contains("\"type\":\"VariableDeclaration\"") && !program.contains("\"errors\":[{"),
		"{program}"
	);
	let program = parse("<button @click=\"if (ok) foo()\"/>", &vue, true);
	assert!(
		program.contains("\"type\":\"IfStatement\"") && !program.contains("\"errors\":[{"),
		"{program}"
	);
}

// cargo test --release --test hosts host_phases -- --ignored --nocapture; TEASEL_HOST_BENCH=file adds a document of its own
#[test]
#[ignore]
fn host_phases() {
	use teasel::json::Prepared;
	let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
	let plan = teasel::json::plan(&fs::read_to_string(root.join("tests/hosts/svelte/plan.json")).unwrap()).unwrap();
	let mut documents = vec![(
		"200 each blocks".to_string(),
		format!(
			"<script>let items = [1,2,3];</script>\n{}",
			"{#each items as item}<p class=\"row\">{item + 1}</p>{/each}\n".repeat(200)
		),
	)];
	if let Ok(path) = std::env::var("TEASEL_HOST_BENCH") {
		documents.push((path.clone(), fs::read_to_string(path).unwrap()));
	}
	for (name, source) in &documents {
		for flags in ["module", "module scopes comments locations"] {
			if std::env::var("TEASEL_HOST_FLAGS").is_ok_and(|selected| selected != flags) {
				continue;
			}
			let prepared = Prepared::borrowed(source, Request::from_names(flags));
			let mut best = f64::MAX;
			for _ in 0..300 {
				let t = std::time::Instant::now();
				prepared.in_place(Entry::Program, 0.0, None, "", Some(&plan)).unwrap();
				best = best.min(t.elapsed().as_secs_f64() * 1e6);
			}
			eprintln!("{best:9.2} µs  {name} {flags}");
		}
	}
}

/// The documents of a host directory: not its plan or the pins.
fn sources(dir: &Path) -> Vec<std::path::PathBuf> {
	common::inputs(dir)
		.into_iter()
		.filter(|f| f.file_stem().is_some_and(|s| s != "plan") && f.extension().is_some_and(|e| e != "plan"))
		.collect()
}

#[cfg(target_os = "linux")]
#[test]
#[ignore]
fn host_profile() {
	use teasel::json::Prepared;
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let plan = teasel::json::plan(&fs::read_to_string(root.join("tests/hosts/svelte/plan.json")).unwrap()).unwrap();
	let source = format!(
		"<script>let items = [1,2,3];</script>\n{}",
		"{#each items as item}<p class=\"row\">{item + 1}</p>{/each}\n".repeat(200)
	);
	let prepared = Prepared::borrowed(&source, Request::from_names("module scopes comments locations"));
	let guard = pprof::ProfilerGuardBuilder::default()
		.frequency(4000)
		.blocklist(&["libc", "libgcc", "pthread", "vdso"])
		.build()
		.unwrap();
	for _ in 0..2000 {
		prepared.in_place(Entry::Program, 0.0, None, "", Some(&plan)).unwrap();
	}

	let report = guard.report().build().unwrap();
	let file = std::fs::File::create(root.join("../../target/host.svg")).unwrap();
	report.flamegraph(file).unwrap();
	let mut by_frame: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
	let mut total = 0usize;
	for (frames, count) in &report.data {
		total += *count as usize;
		let names: Vec<String> = frames
			.frames
			.iter()
			.flat_map(|f| {
				f.iter().map(|s| {
					s.name
						.as_deref()
						.map(|n| rustc_demangle::demangle(&String::from_utf8_lossy(n)).to_string())
						.unwrap_or_default()
				})
			})
			.collect();
		if let Some(top) = names.first() {
			by_frame.entry(top.clone()).or_default().0 += *count as usize;
		}
		let mut seen = std::collections::HashSet::new();
		for name in &names {
			if seen.insert(name.clone()) {
				by_frame.entry(name.clone()).or_default().1 += *count as usize;
			}
		}
	}
	let mut rows: Vec<_> = by_frame.into_iter().collect();
	rows.sort_by_key(|row| std::cmp::Reverse(row.1.0));
	eprintln!("samples {total}");
	eprintln!("{:>6} {:>6}  frame", "self%", "incl%");
	for (name, (own, incl)) in rows.iter().take(40) {
		eprintln!(
			"{:6.1} {:6.1}  {}",
			*own as f64 * 100.0 / total as f64,
			*incl as f64 * 100.0 / total as f64,
			name.replace("teasel::", "")
				.replace("core::", "")
				.replace("alloc::", "")
				.chars()
				.take(400)
				.collect::<String>()
		);
	}
}
