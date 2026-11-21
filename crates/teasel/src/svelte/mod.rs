//! Svelte: a component read by one walker, its markup, blocks and tags written as rows, the
//! JavaScript inside read by the parser into the same tree; and CSS, a component's style element
//! or a stylesheet on its own.

mod blocks;
mod css;
mod entities;
mod js;
mod markup;
mod nodes;

use std::cell::RefCell;

use self::blocks::{Group, Open};
pub(crate) use self::js::typescript;
use self::nodes::{Name, Shapes};
pub(crate) use self::nodes::{NAMES, children, n, stylesheet_children};
use crate::ast::{Ast, HostGroup, List, NodeId, Opens, Value};
use crate::error::{Code, SyntaxError};
use crate::interner::StrId;
use crate::lexer::scan::{find, is_space};
use crate::lexer::unicode::is_id_continue;
use crate::parser::{Options, Result};

thread_local! {
	/// The shapes of Svelte's rows, numbered for good as the walker meets them.
	pub(super) static SHAPES: RefCell<Shapes> = RefCell::default();
}

/// Parses a Svelte component: its tree with the JavaScript inside it, positions of the whole
/// source. Returns the tree and its root.
pub(crate) fn parse_component(src: &str, options: Options, reused: Option<Box<Ast>>) -> (Box<Ast>, Result<NodeId>) {
	SHAPES.with(|shapes| {
		let full = src.len() as u32;
		let cut = src.trim_end_matches(is_space);
		let mut walker = Walker::new(cut, full, shapes.borrow_mut(), options, reused);
		let root = walker.run();
		(walker.ast.take().unwrap(), root)
	})
}

/// The whole source as a CSS stylesheet; see `Walker::stylesheet`. Its rows are numbered among
/// Svelte's, by the same names.
pub(crate) fn parse_stylesheet(src: &str, options: Options, reused: Option<Box<Ast>>) -> (Box<Ast>, Result<NodeId>) {
	SHAPES.with(|shapes| {
		let mut walker = Walker::new(src, src.len() as u32, shapes.borrow_mut(), options, reused);
		let root = walker.stylesheet();
		(walker.ast.take().unwrap(), root)
	})
}

/// Buffers given back once used, for the next of their kind.
pub(crate) struct Pool<T>(Vec<Vec<T>>);

impl<T> Default for Pool<T> {
	fn default() -> Self {
		Pool(Vec::new())
	}
}

impl<T> Pool<T> {
	pub(crate) fn take(&mut self) -> Vec<T> {
		self.0.pop().unwrap_or_default()
	}

	pub(crate) fn give(&mut self, mut buffer: Vec<T>) {
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
		ty: Name,
		attributes: Vec<NodeId>,
		/// The field a `this` attribute fills, on `svelte:element` and `svelte:component`.
		this: Option<(Name, Value)>,
		nodes: Vec<NodeId>,
		/// It carries `shadowrootmode`: a `slot` inside it is a plain element.
		shadow: bool,
		/// The patterns its directives declare in its scope.
		declared: Vec<NodeId>,
	},
	Block(Open),
}

/// The cursor over a component's source, or a stylesheet's, and the tree it writes: the rows,
/// numbered by shape, and the JavaScript read into it; what it is inside, and what the element
/// being read holds so far.
pub(crate) struct Walker<'a> {
	pub(crate) src: &'a str,
	/// The source's length before the whitespace that ends it was cut: where the root ends.
	pub(crate) full: u32,
	pub(crate) shapes: std::cell::RefMut<'a, Shapes>,
	pub(crate) options: Options,
	pub(crate) ast: Option<Box<Ast>>,
	pub(crate) at: u32,
	/// The JavaScript read last ended at one of its stops.
	pub(crate) stopped: bool,
	/// How many CSS blocks and pseudo-class argument lists are open.
	pub(crate) nesting: u32,
	frames: Vec<Frame>,
	/// The root elements read so far, each read once.
	once: Vec<Name>,
	/// Where the current tag's word starts, for `{@const}`, which leaves its keyword out.
	keyword: u32,
	/// The element the browser closed last, what closed it and how deep the stack was then.
	autoclosed: Option<(&'a str, &'a str, usize)>,
	/// The patterns the directives of the element being read declare.
	declared: Vec<NodeId>,
	nodes: Pool<NodeId>,
	groups: Pool<Group>,
	/// The kinds and names the attributes of the element being read must not repeat.
	seen: Vec<(Name, StrId)>,
}

impl<'a> Walker<'a> {
	pub(crate) fn new(
		src: &'a str,
		full: u32,
		shapes: std::cell::RefMut<'a, Shapes>,
		options: Options,
		reused: Option<Box<Ast>>,
	) -> Self {
		Walker {
			src,
			full,
			shapes,
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
			declared: Vec::new(),
			nodes: Pool::default(),
			groups: Pool::default(),
			seen: Vec::new(),
		}
	}

	pub(crate) fn ast(&mut self) -> &mut Ast {
		self.ast.as_deref_mut().unwrap()
	}

	pub(crate) fn tree(&self) -> &Ast {
		self.ast.as_deref().unwrap()
	}

	pub(crate) fn len(&self) -> u32 {
		self.src.len() as u32
	}

	pub(crate) fn rest(&self) -> &'a str {
		&self.src[self.at as usize..]
	}

	pub(crate) fn byte(&self) -> Option<u8> {
		self.src.as_bytes().get(self.at as usize).copied()
	}

	pub(crate) fn char(&self) -> Option<char> {
		self.rest().chars().next()
	}

	pub(crate) fn matches(&self, s: &str) -> bool {
		self.rest().starts_with(s)
	}

	/// A word of Svelte's, followed by nothing that continues an identifier.
	pub(crate) fn word(&self, s: &str) -> bool {
		self.matches(s) && !self.rest()[s.len()..].starts_with(is_id_continue)
	}

	pub(crate) fn eat(&mut self, s: &str) -> bool {
		if self.matches(s) {
			self.at += s.len() as u32;
			true
		} else {
			false
		}
	}

	pub(crate) fn expect(&mut self, s: &str) -> Result<()> {
		if self.eat(s) {
			Ok(())
		} else {
			self.report_at(self.at, self.at, Code::Expected, Some(s))
		}
	}

	pub(crate) fn recovering(&self) -> bool {
		self.options.has(Options::ERROR_RECOVERY)
	}

	/// Under recovery the error is recorded on the tree and reading goes on; otherwise it ends
	/// the parse.
	pub(crate) fn report(&mut self, error: Box<SyntaxError>) -> Result<()> {
		if self.recovering() {
			self.ast().errors.push(*error);
			Ok(())
		} else {
			Err(error)
		}
	}

	pub(crate) fn report_at(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<()> {
		let error = self.error(pos, end, code, arg);
		self.report(error)
	}

	pub(crate) fn fail<T>(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<T> {
		Err(self.error(pos, end, code, arg))
	}

	pub(crate) fn error(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Box<SyntaxError> {
		let error = SyntaxError::new(pos, code).to(end);
		Box::new(match arg {
			Some(arg) => error.arg(self.intern(arg)),
			None => error,
		})
	}

	/// The word that ends the tag being read, skipped to under recovery.
	pub(crate) fn skip_to(&mut self, end: &str) {
		if let Some(i) = self.rest().find(end) {
			self.at += (i + end.len()) as u32;
		} else {
			self.at = self.len();
		}
	}

	pub(crate) fn space(&mut self) {
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

	pub(crate) fn require_space(&mut self) -> Result<()> {
		match self.char() {
			Some(c) if is_space(c) => {
				self.space();
				Ok(())
			}
			_ => self.fail(self.at, self.at, Code::Expected, Some("whitespace")),
		}
	}

	pub(crate) fn intern(&mut self, s: &str) -> StrId {
		self.ast().strings.intern(s)
	}

	fn within_depth(&mut self, at: u32) -> Result<()> {
		if self.frames.len() as u32 >= crate::parser::MAX_DEPTH {
			return self.fail(at, at + 1, Code::NestingDepth, None);
		}
		Ok(())
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
				self.text_node();
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
		let mut fields = [(n::fragment, Value::Null); 7];
		let mut count = 4;
		fields[..4].copy_from_slice(&[
			(n::css, css.map_or(Value::Null, Value::Node)),
			(n::js, Value::Nodes(List::EMPTY)),
			(n::options, Value::Null),
			(n::comments, Value::Comments),
		]);
		if let Some(module) = module {
			fields[count] = (n::module, Value::Node(module));
			count += 1;
		}
		let from = count;
		if let Some(instance) = instance {
			fields[count] = (n::instance, Value::Node(instance));
			count += 1;
		}
		fields[count] = (n::fragment, Value::Node(fragment));
		count += 1;
		// the scope belongs to the instance script's program, or to the fragment
		let node = match instance {
			Some(script) => match self.field_of(script, n::content) {
				Some(Value::Node(program)) => Some(program),
				_ => None,
			},
			None => Some(fragment),
		};
		let full = self.full;
		let base = self.ast().host_keys.len() as u32;
		let at = self.ast().host_groups.len() as u32;
		// the fragment's own scope inside the one the instance script opens, one scope around both
		for (from, node) in [(count - 1, Some(fragment)), (from, node)] {
			self.ast().host_groups.push(HostGroup {
				inside: List::EMPTY,
				from: base + from as u32,
				until: base + count as u32,
				node,
			});
		}
		let scope = Some(Opens {
			outside: List::EMPTY,
			groups: (at, 2),
		});
		Ok(self.opening(n::Root, 0, full, &fields[..count], scope))
	}

	fn text_node(&mut self) {
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
