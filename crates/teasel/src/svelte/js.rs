//! The JavaScript inside a component: read by the parser into the walker's tree, at the cursor
//! or over a script's content, and the identifiers the walker reads itself.

use super::Walker;
use crate::ast::{List, NodeId, NodeKind};
use crate::error::Code;
use crate::lexer::scan::{find, find_pair, is_space};
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry as JsEntry, Options, Parser, Result};

/// Whether the document is TypeScript: a script tag says `lang="ts"`.
pub(crate) fn typescript(src: &str) -> bool {
	tag_carries(src, "script", &[("lang", Some("ts"))])
}

/// Where the tag ends: the first `>` outside quotes, or the end.
fn tag_end(after: &str) -> usize {
	let mut quote = None;
	for (i, c) in after.char_indices() {
		match quote {
			Some(q) if c == q => quote = None,
			Some(_) => {}
			None if c == '"' || c == '\'' => quote = Some(c),
			None if c == '>' => return i,
			None => {}
		}
	}
	after.len()
}

/// Whether a `<name` tag in `src`, outside HTML comments, carries one of `wanted`: an attribute,
/// with the text value it needs, if any.
pub(crate) fn tag_carries(src: &str, name: &str, wanted: &[(&str, Option<&str>)]) -> bool {
	if wanted.is_empty() {
		return false;
	}
	// stopping at every `<` took 1.22 ms a pass over Svelte's tests, the lexer's scan for `<` 0.64, this 0.52
	let (bytes, first) = (src.as_bytes(), name.as_bytes().first().copied());
	let mut at = 0;
	loop {
		at = match first {
			Some(first) => find_pair(bytes, at, b'<', [b'!', first]),
			None => find(bytes, at, *b"<", false),
		};
		if at >= src.len() {
			return false;
		}
		let rest = &src[at + 1..];
		if let Some(after) = rest.strip_prefix("!--") {
			let Some(j) = after.find("-->") else {
				return false;
			};
			at = src.len() - after.len() + j + 3;
			continue;
		}
		let Some(after) = rest.strip_prefix(name).filter(|after| after.starts_with(is_space)) else {
			at += 1;
			continue;
		};
		let tag_end = tag_end(after);
		let mut attributes = after[..tag_end].trim_start_matches(is_space);
		while !attributes.is_empty() {
			let name_len = attributes
				.find(|c: char| is_space(c) || c == '=' || c == '/')
				.unwrap_or(attributes.len());
			let name = &attributes[..name_len];
			attributes = attributes[name_len..].trim_start_matches(is_space);
			let mut value = None;
			if let Some(after) = attributes.strip_prefix('=') {
				let after = after.trim_start_matches(is_space);
				let (text, len) = match after.chars().next() {
					Some(q @ ('"' | '\'')) => {
						let close = after[1..].find(q).unwrap_or(after.len() - 1);
						(&after[1..1 + close], (1 + close + 1).min(after.len()))
					}
					_ => {
						let len = after.find(is_space).unwrap_or(after.len());
						(&after[..len], len)
					}
				};
				value = Some(text);
				attributes = after[len..].trim_start_matches(is_space);
			} else if name.is_empty() {
				let mut chars = attributes.chars();
				chars.next();
				attributes = chars.as_str();
			}
			if wanted
				.iter()
				.any(|&(attribute, wanted)| name == attribute && wanted.is_none_or(|w| value == Some(w)))
			{
				return true;
			}
		}
		at = src.len() - after.len() + tag_end;
	}
}

/// A word that cannot name a binding.
pub(crate) fn reserved(word: &str) -> bool {
	use crate::lexer::token::word::{ENUM, KEYWORD, STRICT, flags};
	flags(word) & (KEYWORD | STRICT | ENUM) != 0
}

impl<'a> Walker<'a> {
	/// One expression read strictly, so that recovery cannot stand in for what is not one, and
	/// taken back unless `keep` says it ended where it should.
	pub(crate) fn trial(&mut self, stop: &str, keep: impl FnOnce(&mut Self) -> bool) -> Option<List> {
		let (start, comments, mark) = (self.at, self.tree().comments.len(), self.tree().mark());
		let options = self.options;
		self.options = options.without(Options::ERROR_RECOVERY);
		let read = self.js(JsEntry::Expression, stop);
		self.options = options;
		if let Ok(roots) = read
			&& keep(self)
		{
			return Some(roots);
		}
		self.at = start;
		self.ast().comments.truncate(comments);
		self.ast().truncate(mark);
		None
	}

	/// The JavaScript at the cursor, read by the parser into the same tree; the
	/// cursor moves past it.
	pub(crate) fn js(&mut self, entry: JsEntry, stop: &str) -> Result<List> {
		if self.options.typescript() {
			self.js_in::<true>(entry, stop)
		} else {
			self.js_in::<false>(entry, stop)
		}
	}

	fn js_in<const TS: bool>(&mut self, entry: JsEntry, stop: &str) -> Result<List> {
		let ast = self.ast.take().unwrap();
		let src = self.src;
		let mut parser = Parser::<TS>::new(src, self.at, self.options, stop, ast);
		let first = parser.tok.start;
		let roots = match parser.start().and_then(|()| parser.read_entry(entry)) {
			// the placeholder spans what was read: a host copies an expression's text by its range
			Err(error) if parser.recovering() => {
				let at = error.pos;
				parser.record(Err(error)).unwrap();
				parser.skip_to_end();
				parser.prev_end = parser.prev_end.max(at);
				if entry == JsEntry::Params {
					Ok(List::EMPTY)
				} else {
					let name = parser.intern("");
					let end = parser.consumed_end().max(first);
					let placeholder = parser.add_with_end(NodeKind::Identifier { name }, first, end);
					Ok(parser.list_of(&[placeholder]))
				}
			}
			result => result,
		};
		let end = parser.consumed_end();
		self.stopped = roots.is_ok() && parser.stopped();
		self.ast = Some(parser.finish());
		let roots = roots?;
		self.at = end;
		Ok(roots)
	}

	/// The one root a JavaScript entry read.
	pub(crate) fn first(&self, roots: List) -> NodeId {
		self.tree().nth(roots, 0).unwrap()
	}

	pub(crate) fn program(&mut self, start: u32, end: u32) -> Result<NodeId> {
		if self.options.typescript() {
			self.program_in::<true>(start, end)
		} else {
			self.program_in::<false>(start, end)
		}
	}

	fn program_in<const TS: bool>(&mut self, start: u32, end: u32) -> Result<NodeId> {
		let ast = self.ast.take().unwrap();
		let src = &self.src[..end as usize];
		// the template may declare what the script exports
		let options = self.options.with(Options::ALLOW_UNDECLARED_EXPORTS);
		let mut parser = Parser::<TS>::new(src, start, options, "", ast);
		let program = parser.start().and_then(|()| parser.parse_program());
		self.ast = Some(parser.finish());
		program
	}

	/// An empty identifier standing where one could not be read.
	pub(crate) fn placeholder(&mut self, start: u32, end: u32) -> NodeId {
		let name = self.intern("");
		self.ast().add(NodeKind::Identifier { name }, start, end)
	}

	/// An identifier the host reads itself, as a node.
	pub(crate) fn identifier(&mut self) -> Result<NodeId> {
		let start = self.at;
		match self.char() {
			Some(c) if is_id_start(c) => self.at += c.len_utf8() as u32,
			_ => {
				// under recovery an empty identifier stands where one was expected
				self.report_at(start, start, Code::Expected, Some("an identifier"))?;
				return Ok(self.placeholder(start, start));
			}
		}
		while let Some(c) = self.char() {
			if !is_id_continue(c) {
				break;
			}
			self.at += c.len_utf8() as u32;
		}
		let end = self.at;
		let word = &self.src[start as usize..end as usize];
		if reserved(word) || (self.options.has(Options::MODULE) && word == "await") {
			self.report_at(start, end, Code::ReservedWord, Some(word))?;
		}
		let name = self.intern(word);
		Ok(self.ast().add(NodeKind::Identifier { name }, start, end))
	}

	/// The expression one reads in `{expression}`.
	pub(super) fn expression_of(&mut self, stop: &str) -> Result<NodeId> {
		let roots = self.js(JsEntry::Expression, stop)?;
		Ok(self.first(roots))
	}

	pub(super) fn pattern(&mut self, stop: &str) -> Result<NodeId> {
		let roots = self.js(JsEntry::Pattern, stop)?;
		Ok(self.first(roots))
	}
}
