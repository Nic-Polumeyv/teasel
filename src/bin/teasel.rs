//! Command line front end, mainly for the acorn conformance harness.
//!
//! `teasel [--module] [--expression|--pattern|--params|--statement] [--preserve-parens] [--offset N] FILE`
//! prints ESTree JSON. `teasel --batch` reads jobs from stdin, each a header line `MODE LENGTH`
//! followed by LENGTH bytes of source, and prints one JSON line per job. MODE is `module`, `script`,
//! `expr:OFFSET`, `pattern:OFFSET`, `params:OFFSET` or `stmt:OFFSET`. Offsets are byte offsets into
//! the source; the JSON output reports UTF-16 offsets like acorn.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::ast::NodeId;
use teasel::{Options, estree, parse, parse_expression_at, parse_params_at, parse_pattern_at, parse_statement_at};

#[derive(Clone, Copy)]
enum Mode {
	Program,
	Expression(u32),
	Pattern(u32),
	Params(u32),
	Statement(u32),
}

impl Mode {
	fn offset(self) -> u32 {
		match self {
			Mode::Program => 0,
			Mode::Expression(o) | Mode::Pattern(o) | Mode::Params(o) | Mode::Statement(o) => o,
		}
	}
}

fn run(source: &str, options: Options, mode: Mode) -> String {
	let offset = mode.offset();
	if !source.is_char_boundary(offset as usize) {
		return format!(
			"{{\"error\":{{\"message\":\"offset {offset} is not a character boundary\",\"pos\":{offset}}}}}"
		);
	}
	let result = match mode {
		Mode::Program => parse(source, options).map(|ast| {
			let root = NodeId(ast.nodes.len() as u32 - 1);
			(ast, vec![root])
		}),
		Mode::Expression(o) => parse_expression_at(source, o, options).map(|(ast, id)| (ast, vec![id])),
		Mode::Pattern(o) => parse_pattern_at(source, o, options).map(|(ast, id)| (ast, vec![id])),
		Mode::Statement(o) => parse_statement_at(source, o, options).map(|(ast, id)| (ast, vec![id])),
		Mode::Params(o) => parse_params_at(source, o, options).map(|(ast, ids, _)| (ast, ids)),
	};
	match result {
		Ok((ast, roots)) => match mode {
			Mode::Params(_) => estree::list_to_json(&ast, &roots, source),
			_ => estree::to_json(&ast, roots[0], source),
		},
		Err(error) => estree::error_to_json(&error, source),
	}
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
		let mode = parts.next().unwrap_or("");
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
		let svelte = Options {
			module: true,
			preserve_parens: true,
			..Options::default()
		};
		let module = Options {
			module: true,
			..Options::default()
		};
		let offset = |m: &str| m.split_once(':').and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
		let (options, run_mode) = match mode {
			"module" => (module, Mode::Program),
			"script" => (Options::default(), Mode::Program),
			m if m.starts_with("expr:") => (svelte, Mode::Expression(offset(m))),
			m if m.starts_with("pattern:") => (module, Mode::Pattern(offset(m))),
			m if m.starts_with("params:") => (svelte, Mode::Params(offset(m))),
			m if m.starts_with("stmt:") => (module, Mode::Statement(offset(m))),
			_ => (Options::default(), Mode::Program),
		};
		let json = run(&source, options, run_mode);
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
	let mut options = Options::default();
	let mut mode = Mode::Program;
	let mut offset = 0;
	let mut file = None;
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--module" => options.module = true,
			"--preserve-parens" => options.preserve_parens = true,
			"--expression" => mode = Mode::Expression(0),
			"--pattern" => mode = Mode::Pattern(0),
			"--params" => mode = Mode::Params(0),
			"--statement" => mode = Mode::Statement(0),
			"--offset" => offset = args.next().and_then(|n| n.parse().ok()).unwrap_or(0),
			_ => file = Some(arg),
		}
	}
	let mode = match mode {
		Mode::Program => Mode::Program,
		Mode::Expression(_) => Mode::Expression(offset),
		Mode::Pattern(_) => Mode::Pattern(offset),
		Mode::Params(_) => Mode::Params(offset),
		Mode::Statement(_) => Mode::Statement(offset),
	};
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
	println!("{}", run(&source, options, mode));
	ExitCode::SUCCESS
}
