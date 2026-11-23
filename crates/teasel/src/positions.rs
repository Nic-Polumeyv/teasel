use crate::ast::{CommentRow, Span};
use crate::handed::Handed;

/// Maps byte offsets to UTF-16 offsets, the positions JavaScript counts, and UTF-16 offsets back
/// to bytes.
pub(crate) struct Positions {
	/// After each non-ASCII character: its end byte offset and the bytes-minus-code-units gap so far.
	gaps: Vec<(u32, u32)>,
	len: u32,
}

/// Where the last node's start landed: starts come in source order, and a node's end follows
/// its start, so each lookup tries a few entries on from its hint before a binary search.
#[derive(Default)]
pub(crate) struct Cursor {
	gap: usize,
}

impl Positions {
	pub(crate) fn new(source: &str) -> Self {
		let bytes = source.as_bytes();
		let mut gaps = Vec::new();
		let mut gap = 0u32;
		let mut i = if bytes.is_ascii() { bytes.len() } else { 0 };
		while i < bytes.len() {
			i = crate::lexer::scan::find(bytes, i, [], true);
			let Some(&b) = bytes.get(i) else { break };
			let len = if b >= 0xf0 {
				4
			} else if b >= 0xe0 {
				3
			} else {
				2
			};
			i += len;
			gap += len as u32 - if len == 4 { 2 } else { 1 };
			gaps.push((i as u32, gap));
		}
		Self {
			gaps,
			len: source.len() as u32,
		}
	}

	pub(crate) fn map_slices(&self, slices: &mut [crate::ast::Span]) {
		if self.gaps.is_empty() {
			return;
		}
		let mut cursor = Cursor::default();
		for slice in slices {
			slice.start = self.offset(&mut cursor, slice.start);
			slice.end = self.offset(&mut cursor, slice.end);
		}
	}

	/// The UTF-16 offset each line of `source` starts at, the first line's 0: a line ends at
	/// `\n`, at `\r` not before `\n`, and at U+2028 and U+2029.
	fn lines<'a>(&'a self, source: &'a str) -> impl Iterator<Item = u32> + 'a {
		let bytes = source.as_bytes();
		let mut cursor = Cursor::default();
		let mut next = Some(0);
		std::iter::from_fn(move || {
			let start = next?;
			let end = start + crate::lexer::scan::line_end(&bytes[start..]);
			next = bytes.get(end).map(|&b| {
				end + match b {
					b'\r' if bytes.get(end + 1) == Some(&b'\n') => 2,
					b'\n' | b'\r' => 1,
					_ => 3,
				}
			});
			Some(self.offset(&mut cursor, start as u32))
		})
	}

	/// Every node's span as UTF-16 offsets into `spans`, when the source has gaps.
	pub(crate) fn map_nodes(&self, nodes: &[crate::ast::Node], spans: &mut Handed<Span>) {
		spans.clear();
		if self.gaps.is_empty() {
			return;
		}
		spans.reserve(nodes.len());
		let mut cursor = Cursor::default();
		for node in nodes {
			spans.push(self.span(&mut cursor, node.start, node.end));
		}
	}

	/// One span as JavaScript counts it.
	pub(crate) fn span(&self, cursor: &mut Cursor, start: u32, end: u32) -> Span {
		let (from, gap) = self.offset_from(cursor.gap, start);
		cursor.gap = gap;
		let (to, _) = self.offset_from(gap, end);
		Span { start: from, end: to }
	}

	/// Where each error is, as JavaScript counts: `pos`, `end`, and the line and column of `pos`,
	/// from the lines up to the last of them.
	pub(crate) fn of_errors(&self, source: &str, errors: &[crate::SyntaxError]) -> Vec<[u32; 4]> {
		let Some(last) = errors.iter().map(|e| e.pos.max(e.end)).max() else {
			return Vec::new();
		};
		let upto = source.ceil_char_boundary(last as usize);
		let lines: Vec<u32> = self.lines(&source[..upto]).collect();
		let mut cursor = Cursor::default();
		errors
			.iter()
			.map(|error| {
				let pos = self.offset(&mut cursor, error.pos);
				let end = self.offset(&mut cursor, error.end);
				let line = lines.partition_point(|&start| start <= pos);
				[pos, end, line as u32, pos - lines[line - 1]]
			})
			.collect()
	}

	/// Each comment's row.
	pub(crate) fn map_comments(&self, comments: &[crate::ast::Comment], out: &mut Handed<CommentRow>) {
		out.clear();
		let mut cursor = Cursor::default();
		for comment in comments {
			let text = comment.text_range();
			let start = self.offset(&mut cursor, comment.start);
			let from = self.offset(&mut cursor, text.start as u32);
			let to = self.offset(&mut cursor, text.end as u32);
			let end = self.offset(&mut cursor, comment.end);
			out.push(CommentRow {
				block: comment.is_block(),
				text: Span { start: from, end: to },
				span: Span { start, end },
			});
		}
	}

	pub(crate) fn offset(&self, cursor: &mut Cursor, byte: u32) -> u32 {
		let (offset, gap) = self.offset_from(cursor.gap, byte);
		cursor.gap = gap;
		offset
	}

	/// The UTF-16 offset of `byte` and the gap entry it lies after, looked up from `hint`.
	fn offset_from(&self, hint: usize, byte: u32) -> (u32, usize) {
		let byte = byte.min(self.len);
		if self.gaps.is_empty() {
			return (byte, 0);
		}
		let gap = locate(&self.gaps, hint, byte, |g| g.0);
		(byte - if gap == 0 { 0 } else { self.gaps[gap - 1].1 }, gap)
	}
}

/// The number of `items` whose key is at most `byte`, trying `hint` and the few after it first.
fn locate<T>(items: &[T], hint: usize, byte: u32, key: impl Fn(&T) -> u32) -> usize {
	for p in hint..=(hint + 3).min(items.len()) {
		if (p == 0 || key(&items[p - 1]) <= byte) && (p == items.len() || key(&items[p]) > byte) {
			return p;
		}
	}
	items.partition_point(|item| key(item) <= byte)
}

#[cfg(test)]
mod tests {
	use super::Positions;

	#[test]
	fn lines() {
		let lines = |text: &str| Positions::new(text).lines(text).collect::<Vec<_>>();
		assert_eq!(lines("a\nb\r\nc\rd\u{2028}e\u{2029}f𝒳\ng"), [0, 2, 5, 7, 9, 11, 15]);
		assert_eq!(lines(""), [0]);
		assert_eq!(lines("\r\n\r"), [0, 2, 3]);
	}
}
