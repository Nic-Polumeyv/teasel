//! The command line: one file, or a batch of jobs, answered as JSON.
//!
//! `teasel [--module] [--typescript] [--comments] [--scopes] [--parenthesized] [--erase]
//! [--recover] [--stylesheet|--svelte] FILE` prints the answer as JSON: the node as `node`, then
//! `end`; a parse that fails has `node` null and its error alone in `errors`, the code as the byte
//! the package's `CODES` names. The file is JavaScript, or a stylesheet or a Svelte component.
//!
//! `teasel --batch` reads jobs from stdin, each a header line `LANGUAGE FLAGS LENGTH` followed by
//! LENGTH bytes of source, and prints one JSON line per job. LANGUAGE is 0 for JavaScript, 1 for
//! CSS, 2 for Svelte, as the engine takes it; FLAGS is the option word as the bridges take it,
//! with nothing added: no module it does not ask for. The JSON output reports UTF-16 offsets, as
//! JavaScript counts them.

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use teasel::{Entry, Options, engine, estree};

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
		let mut words = header.split_ascii_whitespace();
		let mut word = || words.next().and_then(|w| w.parse::<u32>().ok());
		let (Some(language), Some(flags), Some(length), None) = (word(), word(), word(), words.next()) else {
			return Err(io::Error::new(
				io::ErrorKind::InvalidData,
				format!("malformed header: {header:?}"),
			));
		};
		let entry = engine::entry(f64::from(language)).map_err(|why| io::Error::new(io::ErrorKind::InvalidData, why))?;
		let mut bytes = Vec::new();
		(&mut input).take(length as u64).read_to_end(&mut bytes)?;
		if bytes.len() != length as usize {
			return Err(io::Error::new(
				io::ErrorKind::UnexpectedEof,
				"source shorter than its header",
			));
		}
		let source = String::from_utf8_lossy(&bytes);
		out.write_all(estree::parse(&source, entry, 0, Options(flags)).as_bytes())?;
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
	let mut entry = Entry::Program;
	let mut flags = 0;
	let mut file = None;
	for arg in args {
		match arg.as_str() {
			"--module" => flags |= Options::MODULE,
			"--typescript" => flags |= Options::TYPESCRIPT,
			"--comments" => flags |= Options::COMMENTS,
			"--scopes" => flags |= Options::SCOPES,
			"--parenthesized" => flags |= Options::PARENTHESIZED,
			"--erase" => flags |= Options::ERASE,
			"--recover" => flags |= Options::ERROR_RECOVERY,
			"--stylesheet" => entry = Entry::StyleSheet,
			"--svelte" => entry = Entry::Svelte,
			flag if flag.starts_with("--") => {
				eprintln!("teasel: {flag} is not an option");
				return ExitCode::FAILURE;
			}
			_ => file = Some(arg),
		}
	}
	let Some(file) = file else {
		eprintln!(
			"usage: teasel [--module] [--typescript] [--comments] [--scopes] [--parenthesized] [--erase] [--recover] [--stylesheet|--svelte] FILE"
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
	println!("{}", estree::parse(&source, entry, 0, Options(flags)));
	ExitCode::SUCCESS
}
