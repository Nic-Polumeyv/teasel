//! Command line front end, mainly for the acorn conformance harness.
//!
//! `teasel [--module] [--expression] [--preserve-parens] [--offset N] FILE` prints ESTree JSON.
//! `teasel --batch` reads jobs from stdin, each a header line `MODE LENGTH` followed by LENGTH
//! bytes of source, and prints one JSON line per job. MODE is `module`, `script` or `expr:OFFSET`.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::ast::NodeId;
use teasel::{Options, estree, parse, parse_expression_at};

fn run(source: &str, options: Options, expression: Option<u32>) -> String {
	let result = match expression {
		Some(offset) => parse_expression_at(source, offset, options).map(|(ast, id)| (ast, id)),
		None => parse(source, options).map(|ast| {
			let root = NodeId(ast.nodes.len() as u32 - 1);
			(ast, root)
		}),
	};
	match result {
		Ok((ast, root)) => estree::to_json(&ast, root, source),
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
		let length: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
		let mut bytes = vec![0; length];
		input.read_exact(&mut bytes)?;
		let source = String::from_utf8_lossy(&bytes);
		let (options, expression) = match mode {
			"module" => (
				Options {
					module: true,
					..Options::default()
				},
				None,
			),
			"script" => (Options::default(), None),
			m if m.starts_with("expr:") => (
				Options {
					module: true,
					preserve_parens: true,
					..Options::default()
				},
				Some(m[5..].parse().unwrap_or(0)),
			),
			_ => (Options::default(), None),
		};
		let json = run(&source, options, expression);
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
	let mut expression = None;
	let mut file = None;
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--module" => options.module = true,
			"--preserve-parens" => options.preserve_parens = true,
			"--expression" => expression = Some(expression.unwrap_or(0)),
			"--offset" => expression = Some(args.next().and_then(|n| n.parse().ok()).unwrap_or(0)),
			_ => file = Some(arg),
		}
	}
	let Some(file) = file else {
		eprintln!("usage: teasel [--module] [--expression] [--preserve-parens] [--offset N] FILE");
		return ExitCode::FAILURE;
	};
	let source = match std::fs::read_to_string(&file) {
		Ok(s) => s,
		Err(e) => {
			eprintln!("{file}: {e}");
			return ExitCode::FAILURE;
		}
	};
	println!("{}", run(&source, options, expression));
	ExitCode::SUCCESS
}
