//! Svelte: a component read by one walker, its markup, blocks and tags and the JavaScript
//! inside them in one tree; and CSS, a component's style element or a stylesheet on its own.

mod blocks;
mod css;
mod entities;
mod js;
mod markup;

use std::borrow::Cow;

use self::blocks::Open;
use self::entities::decode;
pub(crate) use self::js::typescript;
use self::markup::Once;
use crate::ast::{Ast, ElementType, List, NodeId, NodeKind};
use crate::error::{Code, SyntaxError};
use crate::interner::StrId;
use crate::lexer::scan::{find, is_space};
use crate::parser::{Options, Result};

/// Parses a Svelte component: its tree with the JavaScript inside it, positions of the whole
/// source. Returns the tree and its root.
pub(crate) fn parse_component(src: &str, options: Options, reused: Option<Box<Ast>>) -> (Box<Ast>, Result<NodeId>) {
	// a component's scripts are modules
	let options = options.with(Options::MODULE);
	let full = src.len() as u32;
	let cut = src.trim_end_matches(is_space);
	let mut walker = Walker::new(cut, full, options, reused);
	let root = walker.run();
	(walker.ast.take().unwrap(), root)
}

/// The whole source as a CSS stylesheet; see `Walker::stylesheet`.
pub(crate) fn parse_stylesheet(src: &str, options: Options, reused: Option<Box<Ast>>) -> (Box<Ast>, Result<NodeId>) {
	let mut walker = Walker::new(src, src.len() as u32, options, reused);
	let root = walker.stylesheet();
	(walker.ast.take().unwrap(), root)
}

/// Buffers given back once used, for the next of their kind.
struct Pool<T>(Vec<Vec<T>>);

impl<T> Default for Pool<T> {
	fn default() -> Self {
		Pool(Vec::new())
	}
}

impl<T> Pool<T> {
	fn take(&mut self) -> Vec<T> {
		self.0.pop().unwrap_or_default()
	}

	fn give(&mut self, mut buffer: Vec<T>) {
		buffer.clear();
		self.0.push(buffer);
	}
}

/// What the walker is inside: the component, an element, or a block.
enum Frame {
	Root {
		nodes: Vec<NodeId>,
		instance: Option<NodeId>,
		module: Option<NodeId>,
		css: Option<NodeId>,
	},
	Element {
		start: u32,
		name: (u32, u32),
		name_id: StrId,
		ty: ElementType,
		attributes: Vec<NodeId>,
		/// What a `this` attribute gave, on `svelte:element` and `svelte:component`.
		this: Option<NodeId>,
		nodes: Vec<NodeId>,
		/// It carries `shadowrootmode`: a `slot` inside it is a plain element.
		shadow: bool,
	},
	Block(Open),
}

/// The cursor over a component's source, or a stylesheet's, and the tree it writes; what it is
/// inside, and what the element being read holds so far.
struct Walker<'a> {
	src: &'a str,
	/// The source's length before the whitespace that ends it was cut: where the root ends.
	full: u32,
	options: Options,
	ast: Option<Box<Ast>>,
	at: u32,
	/// The JavaScript read last ended at one of its stops.
	stopped: bool,
	/// How many CSS blocks and pseudo-class argument lists are open.
	nesting: u32,
	frames: Vec<Frame>,
	/// The root elements read so far, each read once.
	once: Vec<ElementType>,
	/// Where the current tag's word starts, for `{@const}`, which leaves its keyword out.
	keyword: u32,
	/// The element the browser closed last, what closed it and how deep the stack was then.
	autoclosed: Option<(&'a str, &'a str, usize)>,
	nodes: Pool<NodeId>,
	/// The kinds and names the attributes of the element being read must not repeat.
	seen: Vec<(Once, StrId)>,
}

impl<'a> Walker<'a> {
	fn new(src: &'a str, full: u32, options: Options, reused: Option<Box<Ast>>) -> Self {
		Walker {
			src,
			full,
			options,
			ast: Some(reused.unwrap_or_else(|| Box::new(Ast::sized(full as usize)))),
			at: 0,
			stopped: false,
			nesting: 0,
			frames: vec![Frame::Root {
				nodes: Vec::new(),
				instance: None,
				module: None,
				css: None,
			}],
			once: Vec::new(),
			keyword: 0,
			autoclosed: None,
			nodes: Pool::default(),
			seen: Vec::new(),
		}
	}

	// ── the cursor

	fn len(&self) -> u32 {
		self.src.len() as u32
	}

	fn rest(&self) -> &'a str {
		&self.src[self.at as usize..]
	}

	fn byte(&self) -> Option<u8> {
		self.src.as_bytes().get(self.at as usize).copied()
	}

	fn char(&self) -> Option<char> {
		self.rest().chars().next()
	}

	fn matches(&self, s: &str) -> bool {
		self.rest().starts_with(s)
	}

	fn eat(&mut self, s: &str) -> bool {
		if self.matches(s) {
			self.at += s.len() as u32;
			true
		} else {
			false
		}
	}

	fn expect(&mut self, s: &str) -> Result<()> {
		if self.eat(s) {
			Ok(())
		} else {
			self.report_at(self.at, self.at, Code::Expected, Some(s))
		}
	}

	/// The word that ends the tag being read, skipped to under recovery.
	fn skip_to(&mut self, end: &str) {
		if let Some(i) = self.rest().find(end) {
			self.at += (i + end.len()) as u32;
		} else {
			self.at = self.len();
		}
	}

	fn space(&mut self) {
		let bytes = self.src.as_bytes();
		let mut i = self.at as usize;
		// ASCII by its bytes, the rest by its characters
		while let Some(&b) = bytes.get(i) {
			if b < 0x80 {
				if !matches!(b, b' ' | b'\t'..=b'\r') {
					break;
				}
				i += 1;
			} else {
				let c = self.src[i..].chars().next().unwrap();
				if !is_space(c) {
					break;
				}
				i += c.len_utf8();
			}
		}
		self.at = i as u32;
	}

	// ── errors

	fn recovering(&self) -> bool {
		self.options.has(Options::ERROR_RECOVERY)
	}

	/// Under recovery the error is recorded on the tree and reading goes on; otherwise it ends
	/// the parse.
	fn report(&mut self, error: Box<SyntaxError>) -> Result<()> {
		if self.recovering() {
			self.ast().errors.push(*error);
			Ok(())
		} else {
			Err(error)
		}
	}

	fn report_at(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<()> {
		let error = self.error(pos, end, code, arg);
		self.report(error)
	}

	fn fail<T>(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<T> {
		Err(self.error(pos, end, code, arg))
	}

	fn error(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Box<SyntaxError> {
		let error = SyntaxError::new(pos, code).to(end);
		Box::new(match arg {
			Some(arg) => error.arg(self.intern(arg)),
			None => error,
		})
	}

	// ── the tree

	fn ast(&mut self) -> &mut Ast {
		self.ast.as_deref_mut().unwrap()
	}

	fn tree(&self) -> &Ast {
		self.ast.as_deref().unwrap()
	}

	fn intern(&mut self, s: &str) -> StrId {
		self.ast().strings.intern(s)
	}

	fn list(&mut self, nodes: &[NodeId]) -> List {
		self.ast().add_list_from(nodes.iter().map(|&id| Some(id)))
	}

	/// The nodes as a fragment: positioned for the walks that order nodes, though none are written,
	/// at the cursor when there are none. The buffer is given back.
	fn fragment(&mut self, nodes: Vec<NodeId>) -> NodeId {
		let list = self.list(&nodes);
		let (start, end) = match (nodes.first(), nodes.last()) {
			(Some(&first), Some(&last)) => (self.tree().node(first).start, self.tree().node(last).end),
			_ => (self.at, self.at),
		};
		self.nodes.give(nodes);
		self.ast().add(NodeKind::Fragment { nodes: list }, start, end)
	}

	/// A text node over `start..end`, its character references decoded as content's or as an
	/// attribute value's.
	fn text_chunk(&mut self, start: u32, end: u32, attribute: bool) -> NodeId {
		let raw = &self.src[start as usize..end as usize];
		let kind = match decode(raw, attribute) {
			Cow::Borrowed(_) => NodeKind::Text,
			Cow::Owned(data) => NodeKind::TextDecoded {
				data: self.intern(&data),
			},
		};
		self.ast().add(kind, start, end)
	}

	fn append(&mut self, node: NodeId) {
		match self.frames.last_mut().unwrap() {
			Frame::Root { nodes, .. } | Frame::Element { nodes, .. } | Frame::Block(Open { nodes, .. }) => {
				nodes.push(node)
			}
		}
	}

	// ── the component

	fn run(&mut self) -> Result<NodeId> {
		let len = self.len();
		while self.at < len {
			let before = self.at;
			let byte = self.src.as_bytes()[self.at as usize];
			let result = if byte == b'<' && self.tag_start(self.at as usize) {
				self.element()
			} else if byte == b'{' {
				self.construct()
			} else {
				self.text();
				Ok(())
			};
			if let Err(error) = result {
				self.report(error)?;
				// whatever could not be read, something is
				if self.at == before {
					self.at += self.char().map_or(1, |c| c.len_utf8() as u32);
				}
			}
		}
		while self.frames.len() > 1 {
			// under recovery, what is open ends with the source
			self.pop_unclosed(len)?;
		}
		let errors = &mut self.ast().errors;
		errors.sort_by_key(|error| error.pos);
		errors.dedup_by(|a, b| a.pos == b.pos && a.code == b.code);
		let Some(Frame::Root {
			nodes,
			instance,
			module,
			css,
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		let fragment = self.fragment(nodes);
		let scripts = self.ast().add_list(&[module, instance]);
		let full = self.full;
		Ok(self.ast().add(NodeKind::Root { css, scripts, fragment }, 0, full))
	}

	/// Text in content, up to the tag or the `{` that ends it.
	fn text(&mut self) {
		let start = self.at;
		let bytes = self.src.as_bytes();
		// the first character is text whatever it is
		let mut i = self.at as usize + self.char().map_or(1, char::len_utf8);
		loop {
			i = find(bytes, i, *b"<{", false);
			if i >= bytes.len() || bytes[i] == b'{' || self.tag_start(i) {
				break;
			}
			i += 1;
		}
		self.at = i as u32;
		let node = self.text_chunk(start, self.at, false);
		self.append(node);
	}

	fn within_depth(&mut self, at: u32) -> Result<()> {
		if self.frames.len() as u32 >= crate::parser::MAX_DEPTH {
			return self.fail(at, at + 1, Code::NestingDepth, None);
		}
		Ok(())
	}

	/// Closes what is on top at `at` as unclosed: an `{:else if}` node is listed as it is, its
	/// block being what stands unclosed.
	#[cold]
	fn pop_unclosed(&mut self, at: u32) -> Result<()> {
		match self.frames.last().unwrap() {
			Frame::Element { start, name, .. } => {
				let (start, what) = (*start, &self.src[name.0 as usize..name.1 as usize]);
				self.report_at(start, start + 1, Code::Unclosed, Some(what))?;
				self.close_top(at);
			}
			Frame::Block(open) => {
				if !open.elseif {
					let (start, what) = (open.start, open.block.construct().display());
					self.report_at(start, start + 1, Code::Unclosed, Some(what))?;
				}
				self.pop_block(at);
			}
			Frame::Root { .. } => unreachable!(),
		}
		Ok(())
	}
}
