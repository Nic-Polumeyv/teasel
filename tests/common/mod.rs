//! What the pinned tests share: the compact answer laid out one value per line, so a pin reads
//! and diffs, and the pin itself, written on `UPDATE=1` and compared otherwise.

use std::fs;
use std::path::Path;

pub fn pretty(json: &str) -> String {
	let mut out = String::with_capacity(json.len() * 2);
	let mut depth = 0usize;
	let mut chars = json.chars().peekable();
	let newline = |out: &mut String, depth: usize| {
		out.push('\n');
		out.extend(std::iter::repeat_n('\t', depth));
	};
	while let Some(c) = chars.next() {
		match c {
			'"' => {
				out.push('"');
				while let Some(c) = chars.next() {
					out.push(c);
					match c {
						'\\' => out.push(chars.next().unwrap()),
						'"' => break,
						_ => {}
					}
				}
			}
			'{' | '[' => {
				out.push(c);
				let closer = if c == '{' { '}' } else { ']' };
				if chars.peek() == Some(&closer) {
					out.push(chars.next().unwrap());
				} else {
					depth += 1;
					newline(&mut out, depth);
				}
			}
			'}' | ']' => {
				depth -= 1;
				newline(&mut out, depth);
				out.push(c);
			}
			',' => {
				out.push(c);
				newline(&mut out, depth);
			}
			':' => out.push_str(": "),
			_ => out.push(c),
		}
	}
	out.push('\n');
	out
}

/// Whether the answer is what `pin` holds; with `UPDATE=1` it is written there instead.
pub fn pinned(pin: &Path, answer: &str) -> bool {
	if std::env::var_os("UPDATE").is_some() {
		fs::write(pin, answer).unwrap();
		return true;
	}
	fs::read_to_string(pin).ok().as_deref() == Some(answer)
}

/// Every file under `dir` but the pins, sorted.
pub fn inputs(dir: &Path) -> Vec<std::path::PathBuf> {
	let mut files: Vec<_> = fs::read_dir(dir)
		.unwrap()
		.map(|e| e.unwrap().path())
		.filter(|f| f.is_file() && f.extension().is_some_and(|e| e != "json"))
		.collect();
	files.sort();
	files
}
