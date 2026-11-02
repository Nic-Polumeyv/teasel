use crate::ast::{CommentRow, Loc, Position, Span};
use crate::handed::Handed;

/// Maps byte offsets to UTF-16 offsets and line/column pairs, the positions JavaScript counts,
/// and UTF-16 offsets back to bytes.
pub(crate) struct Positions {
	/// After each non-ASCII character: its end byte offset and the bytes-minus-code-units gap so far.
	gaps: Vec<(u32, u32)>,
	/// Each line's start as a byte offset and a UTF-16 offset; only with `lines`.
	line_starts: Vec<(u32, u32)>,
	len: u32,
	lines: bool,
	/// Whether a host's own nodes are located too, or only the JavaScript ones.
	hosts: bool,
}

/// Where the last node's start landed: starts come in source order, and a node's end follows
/// its start, so each lookup tries a few entries on from its hint before a binary search.
#[derive(Default)]
pub(crate) struct Cursor {
	gap: usize,
	line: usize,
}

impl Positions {
	/// As `options` asks: the line table when anything is located, and whether host nodes are.
	pub(crate) fn of(source: &str, options: crate::Options) -> Self {
		Positions {
			hosts: !options.has(crate::Options::LOCATIONS_JS),
			..Self::new(source, options.locations())
		}
	}

	/// `lines` builds the line table, which only `loc` needs.
	pub(crate) fn new(source: &str, lines: bool) -> Self {
		let bytes = source.as_bytes();
		let mut gaps = Vec::new();
		let mut line_starts = vec![(0, 0)];
		let mut gap = 0u32;
		let mut i = 0;
		if !lines && bytes.is_ascii() {
			i = bytes.len();
		}
		while i < bytes.len() {
			i = if lines {
				crate::lexer::scan::find(bytes, i, *b"\n\r", true)
			} else {
				crate::lexer::scan::find(bytes, i, [], true)
			};
			let Some(&b) = bytes.get(i) else { break };
			if b < 0x80 {
				i += 1;
				if lines && (b == b'\n' || (b == b'\r' && bytes.get(i) != Some(&b'\n'))) {
					line_starts.push((i as u32, i as u32 - gap));
				}
			} else {
				let len = if b >= 0xf0 {
					4
				} else if b >= 0xe0 {
					3
				} else {
					2
				};
				let separator = len == 3 && crate::lexer::scan::is_separator(&bytes[i..]);
				i += len;
				gap += len as u32 - if len == 4 { 2 } else { 1 };
				gaps.push((i as u32, gap));
				if lines && separator {
					line_starts.push((i as u32, i as u32 - gap));
				}
			}
		}
		Self {
			gaps,
			line_starts,
			len: source.len() as u32,
			lines,
			hosts: true,
		}
	}

	/// The byte offset of a UTF-16 offset, or why there is none; a position inside a surrogate
	/// pair maps to the byte after the character's first, which is not a character boundary.
	pub(crate) fn byte_offset(&self, utf16: f64) -> Result<u32, String> {
		if !(utf16 >= 0.0 && utf16.fract() == 0.0 && utf16 <= u32::MAX as f64) {
			return Err(format!("offset {utf16} is not a valid position"));
		}
		let target = utf16 as u32;
		let total_gap = self.gaps.last().map_or(0, |g| g.1);
		if target > self.len - total_gap {
			return Err(format!("offset {target} is past the end of the source"));
		}
		let i = self.gaps.partition_point(|g| g.0 - g.1 <= target);
		Ok(target + if i == 0 { 0 } else { self.gaps[i - 1].1 })
	}

	pub(crate) fn map_values(&self, values: &mut [crate::ast::Value]) {
		if self.gaps.is_empty() {
			return;
		}
		let mut cursor = Cursor::default();
		for value in values {
			if let crate::ast::Value::Slice(start, end) = value {
				*start = self.offset(&mut cursor, *start);
				*end = self.offset(&mut cursor, *end);
			}
		}
	}

	/// Every node's span as UTF-16 offsets into `spans`, when the source has gaps, and its lines
	/// and columns into `locs`, when lines are on.
	pub(crate) fn map_nodes(&self, nodes: &[crate::ast::Node], spans: &mut Handed<Span>, locs: &mut Handed<Loc>) {
		spans.clear();
		locs.clear();
		if self.gaps.is_empty() && !self.lines {
			return;
		}
		if !self.gaps.is_empty() {
			spans.reserve(nodes.len());
		}
		if self.lines {
			locs.reserve(nodes.len());
		}
		let mut cursor = Cursor::default();
		for node in nodes {
			let (start, gap) = self.offset_from(cursor.gap, node.start);
			cursor.gap = gap;
			let (end, _) = self.offset_from(gap, node.end);
			if !self.gaps.is_empty() {
				spans.push(Span { start, end });
			}
			if self.lines {
				if !self.hosts && matches!(node.kind, crate::ast::NodeKind::Host(_)) {
					locs.push(Loc::default());
					continue;
				}
				locs.push(self.loc(&mut cursor, node.start, start, node.end, end));
			}
		}
	}

	/// One span as JavaScript counts it, with its lines and columns when they are on for its kind of node.
	pub(crate) fn span(&self, cursor: &mut Cursor, start: u32, end: u32, host: bool) -> (Span, Option<Loc>) {
		let (from, gap) = self.offset_from(cursor.gap, start);
		cursor.gap = gap;
		let (to, _) = self.offset_from(gap, end);
		let loc = (self.lines && (self.hosts || !host)).then(|| self.loc(cursor, start, from, end, to));
		(Span { start: from, end: to }, loc)
	}

	/// The lines and columns of a span whose offsets `offset` already mapped.
	fn loc(&self, cursor: &mut Cursor, from: u32, start: u32, to: u32, end: u32) -> Loc {
		let (sl, sc) = self.line_column(cursor.line, from, start);
		cursor.line = sl;
		let (el, ec) = self.line_column(sl, to, end);
		Loc {
			start: Position {
				line: sl as u32,
				column: sc,
			},
			end: Position {
				line: el as u32,
				column: ec,
			},
		}
	}

	/// Where each error is, as JavaScript counts: `pos`, `end`, and the line and column of `pos`;
	/// the line table is built up to the last of them when there is none.
	pub(crate) fn of_errors(&self, source: &str, errors: &[crate::SyntaxError]) -> Vec<[u32; 4]> {
		let upto;
		let positions = if self.lines || errors.is_empty() {
			self
		} else {
			let last = errors.iter().map(|e| e.pos.max(e.end)).max().unwrap() as usize;
			let end = source.ceil_char_boundary(last);
			upto = Positions::new(&source[..end], true);
			&upto
		};
		let mut cursor = Cursor::default();
		errors
			.iter()
			.map(|error| {
				let pos = positions.offset(&mut cursor, error.pos);
				let (line, column) = positions.line_column(cursor.line, error.pos, pos);
				[pos, positions.offset(&mut cursor, error.end), line as u32, column]
			})
			.collect()
	}

	/// Each comment's row, its `loc` zeros unless lines are on.
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
				loc: if self.lines {
					self.loc(&mut cursor, comment.start, start, comment.end, end)
				} else {
					Loc::default()
				},
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

	/// The line of `byte` and its column, given `byte` already mapped by `offset`.
	fn line_column(&self, hint: usize, byte: u32, offset: u32) -> (usize, u32) {
		let byte = byte.min(self.len);
		let line = locate(&self.line_starts, hint.max(1), byte, |l| l.0);
		(line, offset - self.line_starts[line - 1].1)
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
	fn byte_offsets() {
		let positions = Positions::new("aé𝒳b", false);
		let bytes: Vec<_> = (0..=6).map(|n| positions.byte_offset(n as f64).ok()).collect();
		assert_eq!(bytes, [Some(0), Some(1), Some(3), Some(4), Some(7), Some(8), None]);
		assert!(positions.byte_offset(-1.0).is_err());
		assert!(positions.byte_offset(1.5).is_err());
		assert_eq!(Positions::new("abc", true).byte_offset(3.0), Ok(3));
	}
}
