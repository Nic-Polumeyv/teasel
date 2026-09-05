//! Command line front end, mainly for the acorn conformance harness.
//!
//! `teasel [--module] [--expression|--pattern|--params|--statement] [--preserve-parens] [--offset N] FILE`
//! prints ESTree JSON. `--offset` alone parses an expression. The pattern, params and statement
//! modes parse as a module, and params preserve parens, the way the Svelte compiler drives acorn.
//!
//! `teasel --batch` reads jobs from stdin, each a header line `MODE LENGTH` followed by LENGTH
//! bytes of source, and prints one JSON line per job. MODE is `module`, `script`, `expr:OFFSET`,
//! `pattern:OFFSET`, `params:OFFSET` or `stmt:OFFSET`. Offsets are byte offsets into the source;
//! the JSON output reports UTF-16 offsets like acorn.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::{Options, estree, parse, parse_expression_at, parse_params_at, parse_pattern_at, parse_statement_at};

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

	/// The options the Svelte compiler uses with this entry point.
	fn options(self, batch_mode: &str) -> Options {
		match self {
			Mode::Program => Options {
				module: batch_mode == "module",
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

fn run(source: &str, options: Options, mode: Mode, offset: u32) -> String {
	if !source.is_char_boundary(offset as usize) {
		return format!(
			"{{\"error\":{{\"message\":\"offset {offset} is not a character boundary\",\"pos\":{offset}}}}}"
		);
	}
	let result = match mode {
		Mode::Program => parse(source, options).map(|ast| {
			let root = ast.last();
			estree::to_json(&ast, root, source)
		}),
		Mode::Expression => {
			parse_expression_at(source, offset, options).map(|(ast, id)| estree::to_json(&ast, id, source))
		}
		Mode::Pattern => parse_pattern_at(source, offset, options).map(|(ast, id)| estree::to_json(&ast, id, source)),
		Mode::Statement => {
			parse_statement_at(source, offset, options).map(|(ast, id)| estree::to_json(&ast, id, source))
		}
		Mode::Params => {
			parse_params_at(source, offset, options).map(|(ast, ids, _)| estree::list_to_json(&ast, &ids, source))
		}
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
		let mode = Mode::from_batch(mode_text);
		let offset = mode_text.split_once(':').and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
		let json = run(&source, mode.options(mode_text), mode, offset);
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
	let mut preserve_parens = false;
	let mut file = None;
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--module" => module = true,
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
		mode.options("")
	};
	options.module |= module;
	options.preserve_parens |= preserve_parens;
	let Some(file) = file else {
		eprintln!(
			"usage: teasel [--module] [--expression|--pattern|--params|--statement] [--preserve-parens] [--offset N] FILE"
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
	println!("{}", run(&source, options, mode, offset.unwrap_or(0)));
	ExitCode::SUCCESS
}
