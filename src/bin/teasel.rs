//! Command line front end, mainly for the acorn conformance harness.
//!
//! `teasel [--module] [--typescript] [--comments] [--expression|--pattern|--params|--statement]
//! [--preserve-parens] [--erase] [--offset N] FILE` prints ESTree JSON, wrapped with `end` for everything but
//! a program. `--offset` alone parses an expression. The pattern, params and statement modes parse
//! as a module.
//!
//! `teasel --batch` reads jobs from stdin, each a header line `MODE LENGTH` followed by LENGTH
//! bytes of source, and prints one JSON line per job. MODE is `module`, `script`, `expr:OFFSET`,
//! `pattern:OFFSET`, `params:OFFSET` or `stmt:OFFSET`, whose answers wrap the node or the parameters
//! with `end`, the offset after what the parse consumed, with a `ts-` prefix for TypeScript and
//! `+comments` to attach comments, `+undeclared-exports` to accept exports of names the source
//! never declares, `+until-as` to end an expression at the host's `as` or `+erase` to erase
//! TypeScript from the output. In a batch, expressions
//! preserve parens. Offsets are byte offsets into the source; the JSON output reports UTF-16
//! offsets like acorn.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::json::{Entry, Request};
use teasel::{Options, json};

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

	fn entry(self) -> Entry {
		match self {
			Mode::Program => Entry::Program,
			Mode::Expression => Entry::Expression,
			Mode::Pattern => Entry::Pattern,
			Mode::Params => Entry::Params,
			Mode::Statement => Entry::Statement,
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
			Mode::Expression => Options {
				module: true,
				preserve_parens: true,
				..Options::default()
			},
			Mode::Pattern | Mode::Params | Mode::Statement => Options {
				module: true,
				..Options::default()
			},
		}
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
		let until_as = mode_text.contains("+until-as");
		let erase = mode_text.contains("+erase");
		let mode_text = mode_text
			.replace("+comments", "")
			.replace("+undeclared-exports", "")
			.replace("+until-as", "")
			.replace("+erase", "");
		let mode_text = mode_text.as_str();
		let mode = Mode::from_batch(mode_text);
		let offset = mode_text.split_once(':').and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
		let mut options = mode.options(mode_text, undeclared_exports);
		options.until_as = until_as;
		let request = Request {
			entry: mode.entry(),
			offset,
			typescript,
			comments,
			locations: true,
			erase,
			end: None,
			options,
		};
		let json = json::parse(&source, &request);
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
	let mut erase = false;
	let mut file = None;
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--module" => module = true,
			"--typescript" => typescript = true,
			"--comments" => comments = true,
			"--preserve-parens" => preserve_parens = true,
			"--erase" => erase = true,
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
	let request = Request {
		entry: mode.entry(),
		offset: offset.unwrap_or(0),
		typescript,
		comments,
		locations: true,
		erase,
		end: None,
		options,
	};
	println!("{}", json::parse(&source, &request));
	ExitCode::SUCCESS
}
