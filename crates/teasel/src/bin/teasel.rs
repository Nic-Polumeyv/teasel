//! Command line front end: one file, or a batch of jobs, answered as JSON.
//!
//! `teasel [--module] [--typescript] [--comments] [--scopes] [--expression|--pattern|--params|--statement|--type-parameters]
//! [--parenthesized] [--erase] [--offset N] FILE` prints
//! the answer as JSON: the node (or the parameters) as `node`, then `end`, the offset after what
//! the parse consumed; a parse that fails has `node` null and its error alone in `errors`, the
//! code as the byte the package's `CODES` names. `--offset` alone parses an expression. The
//! pattern, params and statement modes parse as a module.
//!
//! `teasel --batch [--host GRAMMAR]` reads jobs from stdin, each a header line `ENTRY OFFSET FLAGS LENGTH`
//! followed by LENGTH bytes of source, and prints one JSON line per job. ENTRY is the entry's number as
//! the engine takes it, or 7 for a whole document of the host language the grammar's JSON file
//! describes; OFFSET is a byte offset into the source and FLAGS the option word as the bridges take
//! it, with nothing added: no module it does not ask for. The JSON output reports UTF-16 offsets,
//! as JavaScript counts them.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::json::Request;
use teasel::{Entry, Options, json};

const DOCUMENT: u32 = 7;

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
		let mut words = header.split_ascii_whitespace();
		let mut word = || words.next().and_then(|w| w.parse::<u32>().ok());
		let (Some(entry), Some(offset), Some(flags), Some(length), None) =
			(word(), word(), word(), word(), words.next())
		else {
			return Err(io::Error::new(
				io::ErrorKind::InvalidData,
				format!("malformed header: {header:?}"),
			));
		};
		let mut bytes = Vec::new();
		(&mut input).take(length as u64).read_to_end(&mut bytes)?;
		if bytes.len() != length as usize {
			return Err(io::Error::new(
				io::ErrorKind::UnexpectedEof,
				"source shorter than its header",
			));
		}
		let source = String::from_utf8_lossy(&bytes);
		let json = match (Entry::from_index(entry), &grammar) {
			(Some(entry), _) => json::parse(
				&source,
				&Request {
					entry,
					offset,
					..Request::from_flags(flags)
				},
			),
			(None, Some(grammar)) if entry == DOCUMENT => json::parse_document(&source, grammar, Options(flags)),
			(None, _) => {
				return Err(io::Error::new(
					io::ErrorKind::InvalidData,
					format!("entry {entry} needs --host or is no entry"),
				));
			}
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
	let mut flags = 0;
	let mut host = None;
	let mut file = None;
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--module" => flags |= Options::MODULE,
			"--typescript" => flags |= Options::TYPESCRIPT,
			"--comments" => flags |= Options::COMMENTS,
			"--scopes" => flags |= Options::SCOPES,
			"--parenthesized" => flags |= Options::PARENTHESIZED,
			"--erase" => flags |= Options::ERASE,
			"--expression" => entry = Entry::Expression,
			"--pattern" => entry = Entry::Pattern,
			"--params" => entry = Entry::Params,
			"--statement" => entry = Entry::Statement,
			"--type-parameters" => entry = Entry::TypeParameters,
			"--stylesheet" => entry = Entry::StyleSheet,
			"--offset" => offset = args.next().and_then(|n| n.parse().ok()),
			"--host" => host = args.next(),
			_ => file = Some(arg),
		}
	}
	if entry == Entry::Program && offset.is_some() {
		entry = Entry::Expression;
	}
	if !matches!(entry, Entry::Program | Entry::Expression) {
		flags |= Options::MODULE;
	}
	let Some(file) = file else {
		eprintln!(
			"usage: teasel [--module] [--typescript] [--comments] [--scopes] [--expression|--pattern|--params|--statement|--type-parameters|--stylesheet] [--parenthesized] [--erase] [--offset N] FILE"
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
	if let Some(host) = host {
		if entry != Entry::Program {
			eprintln!("--host reads the whole file as a document, with no entry or --offset");
			return ExitCode::FAILURE;
		}
		let grammar = match std::fs::read_to_string(&host) {
			Ok(s) => s,
			Err(e) => {
				eprintln!("{host}: {e}");
				return ExitCode::FAILURE;
			}
		};
		println!("{}", json::parse_document(&source, &grammar, Options(flags)));
		return ExitCode::SUCCESS;
	}
	let request = Request {
		entry,
		offset: offset.unwrap_or(0),
		..Request::from_flags(flags)
	};
	println!("{}", json::parse(&source, &request));
	ExitCode::SUCCESS
}
