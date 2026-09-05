//! Command line front end, mainly for the acorn conformance harness.
//!
//! `teasel [--module] [--typescript] [--comments] [--expression|--pattern|--params|--statement]
//! [--preserve-parens] [--offset N] FILE` prints ESTree JSON. `--offset` alone parses an expression. The pattern,
//! params and statement modes parse as a module, and params preserve parens, as an arrow's would.
//!
//! `teasel --batch` reads jobs from stdin, each a header line `MODE LENGTH` followed by LENGTH
//! bytes of source, and prints one JSON line per job. MODE is `module`, `script`, `expr:OFFSET`,
//! `pattern:OFFSET`, `params:OFFSET` or `stmt:OFFSET`, with a `ts-` prefix for TypeScript and
//! `+comments` to attach comments or `+undeclared-exports` to accept exports of names the source
//! never declares. Offsets are byte offsets into the source; the JSON output reports UTF-16
//! offsets like acorn.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::{Options, estree};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
	Program,
	Expression,
	Pattern,
	Params,
	Statement,
}

impl Mode {
	fn from_batch(mode: &str) -> Mode {
		match mode.split_once(':').map_or(mode, |(m, _)| m) {
			"expr" => Mode::Expression,
			"pattern" => Mode::Pattern,
			"params" => Mode::Params,
			"stmt" => Mode::Statement,
			_ => Mode::Program,
		}
	}

	/// The options each entry point takes in a batch.
	fn options(self, batch_mode: &str, undeclared_exports: bool) -> Options {
		match self {
			Mode::Program => Options {
				module: batch_mode != "script",
				allow_undeclared_exports: undeclared_exports,
				..Options::default()
			},
			Mode::Expression | Mode::Params => Options {
				module: true,
				preserve_parens: true,
				..Options::default()
			},
			Mode::Pattern | Mode::Statement => Options {
				module: true,
				..Options::default()
			},
		}
	}
}

fn run(source: &str, options: Options, mode: Mode, offset: u32, typescript: bool, comments: bool) -> String {
	if !source.is_char_boundary(offset as usize) {
		return format!(
			"{{\"error\":{{\"message\":\"offset {offset} is not a character boundary\",\"pos\":{offset}}}}}"
		);
	}
	macro_rules! run {
		($language:path) => {{
			use $language as language;
			let one = |(mut ast, id)| {
				if comments {
					teasel::comments::attach(&mut ast, source, id, offset);
				}
				estree::to_json(&ast, id, source)
			};
			match mode {
				Mode::Program => language::parse(source, options).map(|ast| {
					let root = ast.last();
					one((ast, root))
				}),
				Mode::Expression => language::parse_expression_at(source, offset, options).map(one),
				Mode::Pattern => language::parse_pattern_at(source, offset, options).map(one),
				Mode::Statement => language::parse_statement_at(source, offset, options).map(one),
				Mode::Params => language::parse_params_at(source, offset, options).map(|(mut ast, ids, _)| {
					if comments {
						teasel::comments::attach_all(&mut ast, source, &ids, offset);
					}
					estree::list_to_json(&ast, &ids, source)
				}),
			}
		}};
	}
	#[cfg(feature = "typescript")]
	let result = if typescript {
		run!(teasel::typescript)
	} else {
		run!(teasel)
	};
	#[cfg(not(feature = "typescript"))]
	let result = {
		if typescript {
			return String::from("{\"error\":{\"message\":\"built without TypeScript\",\"pos\":0}}");
		}
		run!(teasel)
	};
	result.unwrap_or_else(|error| estree::error_to_json(&error, source))
}

fn batch() -> io::Result<()> {
	let stdin = io::stdin();
	let mut input = stdin.lock();
	let stdout = io::stdout();
	let mut out = io::BufWriter::new(stdout.lock());
	let mut header = String::new();
	loop {
		header.clear();
		if input.read_line(&mut header)? == 0 {
			return Ok(());
		}
		let mut parts = header.trim_end().splitn(2, ' ');
		let mode_text = parts.next().unwrap_or("");
		let Some(length) = parts.next().and_then(|n| n.parse::<u64>().ok()) else {
			return Err(io::Error::new(
				io::ErrorKind::InvalidData,
				format!("malformed header: {header:?}"),
			));
		};
		let mut bytes = Vec::new();
		(&mut input).take(length).read_to_end(&mut bytes)?;
		if bytes.len() as u64 != length {
			return Err(io::Error::new(
				io::ErrorKind::UnexpectedEof,
				"source shorter than its header",
			));
		}
		let source = String::from_utf8_lossy(&bytes);
		let (typescript, mode_text) = match mode_text.strip_prefix("ts-") {
			Some(rest) => (true, rest),
			None => (false, mode_text),
		};
		let comments = mode_text.contains("+comments");
		let undeclared_exports = mode_text.contains("+undeclared-exports");
		let mode_text = mode_text.replace("+comments", "").replace("+undeclared-exports", "");
		let mode_text = mode_text.as_str();
		let mode = Mode::from_batch(mode_text);
		let offset = mode_text.split_once(':').and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
		let json = run(
			&source,
			mode.options(mode_text, undeclared_exports),
			mode,
			offset,
			typescript,
			comments,
		);
		out.write_all(json.as_bytes())?;
		out.write_all(b"\n")?;
		out.flush()?;
	}
}

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	if args.iter().any(|a| a == "--batch") {
		return match batch() {
			Ok(()) => ExitCode::SUCCESS,
			Err(e) => {
				eprintln!("{e}");
				ExitCode::FAILURE
			}
		};
	}
	let mut mode = Mode::Program;
	let mut offset = None;
	let mut module = false;
	let mut typescript = false;
	let mut comments = false;
	let mut preserve_parens = false;
	let mut file = None;
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--module" => module = true,
			"--typescript" => typescript = true,
			"--comments" => comments = true,
			"--preserve-parens" => preserve_parens = true,
			"--expression" => mode = Mode::Expression,
			"--pattern" => mode = Mode::Pattern,
			"--params" => mode = Mode::Params,
			"--statement" => mode = Mode::Statement,
			"--offset" => offset = args.next().and_then(|n| n.parse().ok()),
			_ => file = Some(arg),
		}
	}
	if mode == Mode::Program && offset.is_some() {
		mode = Mode::Expression;
	}
	let mut options = if mode == Mode::Program || mode == Mode::Expression {
		Options::default()
	} else {
		mode.options("", false)
	};
	options.module |= module;
	options.preserve_parens |= preserve_parens;
	let Some(file) = file else {
		eprintln!(
			"usage: teasel [--module] [--typescript] [--comments] [--expression|--pattern|--params|--statement] [--preserve-parens] [--offset N] FILE"
		);
		return ExitCode::FAILURE;
	};
	let source = match std::fs::read_to_string(&file) {
		Ok(s) => s,
		Err(e) => {
			eprintln!("{file}: {e}");
			return ExitCode::FAILURE;
		}
	};
	println!(
		"{}",
		run(&source, options, mode, offset.unwrap_or(0), typescript, comments)
	);
	ExitCode::SUCCESS
}
