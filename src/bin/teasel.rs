//! Command line front end, mainly for the acorn conformance harness.
//!
//! `teasel [--module] [--typescript] [--comments] [--scopes] [--expression|--pattern|--params|--statement|--type-parameters]
//! [--parenthesized] [--erase] [--offset N] FILE` prints the answer as JSON: the node (or the
//! parameters) as `node`, then `end`, the offset after what the parse consumed. `--offset` alone
//! parses an expression. The pattern, params and statement modes parse as a module.
//!
//! `teasel --batch [--host GRAMMAR]` reads jobs from stdin, each a header line `MODE LENGTH` followed by LENGTH
//! bytes of source, and prints one JSON line per job. MODE is `module`, `script`, `expr:OFFSET`,
//! `pattern:OFFSET`, `params:OFFSET`, `stmt:OFFSET`, `typeparams:OFFSET` or `doc` for a whole
//! document of the host language the grammar file describes, with a `ts-` prefix
//! for TypeScript and `+comments` to attach comments, `+scopes` for the scope analysis,
//! `+parenthesized` to mark parenthesized nodes, `+undeclared-exports` to accept exports of names
//! the source never declares, `+stop:TOKEN` to end a parse-at entry at one of the host's tokens or
//! `+erase` to erase TypeScript from the output. Offsets are byte offsets into the source; the
//! JSON output reports UTF-16 offsets like acorn.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::json::Request;
use teasel::{Entry, Options, json};

/// A batch header's mode: its entry, offset and switches, which may come before or after the offset.
fn batch_mode(mode: &str) -> (Entry, u32, impl Iterator<Item = &str>) {
	let (head, tail) = mode.split_once(':').unwrap_or((mode, ""));
	let digits = tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len();
	let mut head = head.split('+');
	let entry = match head.next().unwrap_or("") {
		"expr" => Entry::Expression,
		"pattern" => Entry::Pattern,
		"params" => Entry::Params,
		"stmt" => Entry::Statement,
		"typeparams" => Entry::TypeParameters,
		_ => Entry::Program,
	};
	let switches = head.chain(tail[digits..].split('+').skip(1));
	(entry, tail[..digits].parse().unwrap_or(0), switches)
}

fn batch(grammar: Option<String>) -> io::Result<()> {
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
		let (entry, offset, switches) = batch_mode(mode_text);
		let mut request = Request {
			entry,
			offset,
			typescript,
			locations: true,
			options: Options {
				module: !mode_text.starts_with("script"),
				..Options::default()
			},
			..Request::default()
		};
		let mut stop = String::new();
		for switch in switches {
			match switch {
				"comments" => request.comments = true,
				"scopes" => request.scopes = true,
				"erase" => request.erase = true,
				"parenthesized" => request.options.parenthesized = true,
				"recover" => request.options.error_recovery = true,
				_ if switch.starts_with("stop:") => {
					if !stop.is_empty() {
						stop.push(' ');
					}
					stop.push_str(&switch[5..]);
				}
				"undeclared-exports" if entry == Entry::Program => request.options.allow_undeclared_exports = true,
				_ => {}
			}
		}
		let json = match (&grammar, mode_text.starts_with("doc")) {
			(Some(grammar), true) => json::parse_document(&source, grammar, &request),
			_ => json::parse(&source, &request, &stop),
		};
		out.write_all(json.as_bytes())?;
		out.write_all(b"\n")?;
		out.flush()?;
	}
}

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	if args.iter().any(|a| a == "--batch") {
		let grammar = args
			.iter()
			.position(|a| a == "--host")
			.and_then(|i| args.get(i + 1))
			.map(|path| std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}")));
		return match batch(grammar) {
			Ok(()) => ExitCode::SUCCESS,
			Err(e) => {
				eprintln!("{e}");
				ExitCode::FAILURE
			}
		};
	}
	let mut entry = Entry::Program;
	let mut offset = None;
	let mut module = false;
	let mut typescript = false;
	let mut comments = false;
	let mut scopes = false;
	let mut parenthesized = false;
	let mut erase = false;
	let mut host = None;
	let mut file = None;
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--module" => module = true,
			"--typescript" => typescript = true,
			"--comments" => comments = true,
			"--scopes" => scopes = true,
			"--parenthesized" => parenthesized = true,
			"--erase" => erase = true,
			"--expression" => entry = Entry::Expression,
			"--pattern" => entry = Entry::Pattern,
			"--params" => entry = Entry::Params,
			"--statement" => entry = Entry::Statement,
			"--type-parameters" => entry = Entry::TypeParameters,
			"--offset" => offset = args.next().and_then(|n| n.parse().ok()),
			"--host" => host = args.next(),
			_ => file = Some(arg),
		}
	}
	if entry == Entry::Program && offset.is_some() {
		entry = Entry::Expression;
	}
	let options = Options {
		module: module || !matches!(entry, Entry::Program | Entry::Expression),
		parenthesized,
		..Options::default()
	};
	let Some(file) = file else {
		eprintln!(
			"usage: teasel [--module] [--typescript] [--comments] [--scopes] [--expression|--pattern|--params|--statement|--type-parameters] [--parenthesized] [--erase] [--offset N] FILE"
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
		entry,
		offset: offset.unwrap_or(0),
		typescript,
		comments,
		scopes,
		locations: true,
		erase,
		end: None,
		options,
	};
	if let Some(host) = host {
		let grammar = match std::fs::read_to_string(&host) {
			Ok(s) => s,
			Err(e) => {
				eprintln!("{host}: {e}");
				return ExitCode::FAILURE;
			}
		};
		println!("{}", json::parse_document(&source, &grammar, &request));
		return ExitCode::SUCCESS;
	}
	println!("{}", json::parse(&source, &request, ""));
	ExitCode::SUCCESS
}
