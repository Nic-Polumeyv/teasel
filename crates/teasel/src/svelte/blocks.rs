//! What starts with `{`: the markers, the blocks with their branches and bodies, and the tags.

use super::nodes::{Name, n};
use super::{Frame, Walker};
use crate::ast::{HostGroup, List, NodeId, NodeKind, Opens, Value, VariableKind};
use crate::error::Code;
use crate::lexer::scan::is_space;
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry as JsEntry, Result};

/// A construct that starts with `{`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Construct {
	If,
	Each,
	Await,
	Key,
	Snippet,
	Html,
	Debug,
	Const,
	Render,
	Attach,
	Spread,
	Declaration,
	Expression,
}

impl Construct {
	pub(super) fn block(self) -> Option<Block> {
		Some(match self {
			Construct::If => Block::If,
			Construct::Each => Block::Each,
			Construct::Await => Block::Await,
			Construct::Key => Block::Key,
			Construct::Snippet => Block::Snippet,
			_ => return None,
		})
	}

	/// Whether it stands in content; the others stand among attributes.
	pub(super) fn in_content(self) -> bool {
		!matches!(self, Construct::Attach | Construct::Spread)
	}

	/// The marker and its end word, as messages name the open.
	pub(super) fn display(self) -> &'static str {
		match self {
			Construct::If => "{#if}",
			Construct::Each => "{#each}",
			Construct::Await => "{#await}",
			Construct::Key => "{#key}",
			Construct::Snippet => "{#snippet}",
			Construct::Html => "{@html}",
			Construct::Debug => "{@debug}",
			Construct::Const => "{@const}",
			Construct::Render => "{@render}",
			Construct::Attach => "{@attach}",
			Construct::Spread => "{...}",
			Construct::Declaration | Construct::Expression => "{}",
		}
	}
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Which {
	Open,
	Branch(u8),
	Close,
}

/// A marker after its `{`: its parts, whether whitespace must follow it, and what messages call it.
struct Piece {
	construct: Construct,
	which: Which,
	parts: &'static [&'static str],
	space: bool,
	display: &'static str,
}

const fn piece(
	construct: Construct,
	which: Which,
	parts: &'static [&'static str],
	space: bool,
	display: &'static str,
) -> Piece {
	Piece {
		construct,
		which,
		parts,
		space,
		display,
	}
}

/// Every marker of more than its `{`, in the order they are tried.
static PIECES: [Piece; 20] = [
	piece(Construct::If, Which::Open, &["#if"], true, "{#if}"),
	piece(Construct::If, Which::Branch(0), &[":else", "if"], true, "{:else if}"),
	piece(Construct::If, Which::Branch(1), &[":else"], false, "{:else}"),
	piece(Construct::If, Which::Close, &["/if"], false, "{/if}"),
	piece(Construct::Each, Which::Open, &["#each"], true, "{#each}"),
	piece(Construct::Each, Which::Branch(0), &[":else"], false, "{:else}"),
	piece(Construct::Each, Which::Close, &["/each"], false, "{/each}"),
	piece(Construct::Await, Which::Open, &["#await"], true, "{#await}"),
	piece(Construct::Await, Which::Branch(0), &[":then"], false, "{:then}"),
	piece(Construct::Await, Which::Branch(1), &[":catch"], false, "{:catch}"),
	piece(Construct::Await, Which::Close, &["/await"], false, "{/await}"),
	piece(Construct::Key, Which::Open, &["#key"], true, "{#key}"),
	piece(Construct::Key, Which::Close, &["/key"], false, "{/key}"),
	piece(Construct::Snippet, Which::Open, &["#snippet"], true, "{#snippet}"),
	piece(Construct::Snippet, Which::Close, &["/snippet"], false, "{/snippet}"),
	piece(Construct::Html, Which::Open, &["@html"], true, "{@html}"),
	piece(Construct::Debug, Which::Open, &["@debug"], false, "{@debug}"),
	piece(Construct::Const, Which::Open, &["@const"], true, "{@const}"),
	piece(Construct::Render, Which::Open, &["@render"], true, "{@render}"),
	piece(Construct::Attach, Which::Open, &["@attach"], true, "{@attach}"),
];

static SPREAD: Piece = piece(Construct::Spread, Which::Open, &["..."], false, "{...}");

/// The pieces whose second part starts with `byte`, in written order.
fn pieces_after(byte: u8) -> impl Iterator<Item = &'static Piece> {
	PIECES
		.iter()
		.chain(std::iter::once(&SPREAD))
		.filter(move |piece| piece.parts[0].as_bytes()[0] == byte)
}

/// A whole marker at the cursor: its construct, which piece, and where its last part's word starts.
#[derive(Clone, Copy)]
pub(super) struct Mark {
	pub(super) construct: Construct,
	pub(super) which: Which,
	pub(super) word: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Block {
	If,
	Each,
	Await,
	Key,
	Snippet,
}

// the slots of each block's node, in the order its rows hold them
const TEST: usize = 0;
const CONSEQUENT: usize = 1;
const BRANCHES: usize = 2;
const ALTERNATE: usize = 3;
const EXPRESSION: usize = 0;
const CONTEXT: usize = 1;
const INDEX: usize = 2;
const KEY: usize = 3;
const BODY: usize = 4;
const FALLBACK: usize = 5;
const VALUE: usize = 1;
const THEN: usize = 2;
const ERROR: usize = 3;
const CATCH: usize = 4;
const PENDING: usize = 5;
const TYPE_PARAMS: usize = 1;
const PARAMETERS: usize = 2;
const FRAGMENT_OF_SNIPPET: usize = 3;
const FRAGMENT_OF_KEY: usize = 1;
/// No body is being filled.
const NONE: usize = usize::MAX;

/// A block's fields in order, each with the value it takes when nothing filled it, or none.
type Fields = &'static [(Name, Option<Value>)];

const NULL: Option<Value> = Some(Value::Null);

static IF: Fields = &[
	(n::test, NULL),
	(n::consequent, NULL),
	(n::branches, Some(Value::Nodes(List::EMPTY))),
	(n::alternate, NULL),
];
static ELSE_IF: Fields = &[(n::test, NULL), (n::consequent, NULL)];
static EACH: Fields = &[
	(n::expression, NULL),
	(n::context, NULL),
	(n::index, None),
	(n::key, None),
	(n::body, NULL),
	(n::fallback, None),
];
static AWAIT: Fields = &[
	(n::expression, NULL),
	(n::value, NULL),
	(n::then, NULL),
	(n::error, NULL),
	(n::r#catch, NULL),
	(n::pending, NULL),
];
static KEY_BLOCK: Fields = &[(n::expression, NULL), (n::fragment, NULL)];
static SNIPPET: Fields = &[
	(n::expression, NULL),
	(n::typeParams, None),
	(n::parameters, NULL),
	(n::fragment, NULL),
];

impl Block {
	fn ty(self) -> Name {
		match self {
			Block::If => n::IfBlock,
			Block::Each => n::EachBlock,
			Block::Await => n::AwaitBlock,
			Block::Key => n::KeyBlock,
			Block::Snippet => n::SnippetBlock,
		}
	}

	fn shape(self) -> Fields {
		match self {
			Block::If => IF,
			Block::Each => EACH,
			Block::Await => AWAIT,
			Block::Key => KEY_BLOCK,
			Block::Snippet => SNIPPET,
		}
	}

	pub(super) fn construct(self) -> Construct {
		match self {
			Block::If => Construct::If,
			Block::Each => Construct::Each,
			Block::Await => Construct::Await,
			Block::Key => Construct::Key,
			Block::Snippet => Construct::Snippet,
		}
	}

	fn close(self) -> &'static str {
		match self {
			Block::If => "{/if}",
			Block::Each => "{/each}",
			Block::Await => "{/await}",
			Block::Key => "{/key}",
			Block::Snippet => "{/snippet}",
		}
	}
}

/// A body's scope as the head before it read it: the body's slot, the slots the scope holds,
/// and the patterns declared in it.
pub(super) struct Group {
	body: usize,
	fields: u8,
	inside: Vec<NodeId>,
}

/// A block being read, or the `{:else if}` node being read under its if block.
pub(super) struct Open {
	pub(super) start: u32,
	pub(super) block: Block,
	pub(super) elseif: bool,
	slots: [Option<Value>; 6],
	/// The slot `nodes` will fill: a body, or `branches` while `{:else if}` nodes come.
	body: usize,
	pub(super) nodes: Vec<NodeId>,
	groups: Vec<Group>,
	outside: Vec<NodeId>,
}

impl Open {
	fn shape(&self) -> Fields {
		if self.elseif { ELSE_IF } else { self.block.shape() }
	}

	/// The slot holds the `{:else if}` nodes, not a fragment.
	fn listing(&self) -> bool {
		self.block == Block::If && !self.elseif && self.body == BRANCHES
	}

	fn set(&mut self, slot: usize, value: Value) {
		self.slots[slot].get_or_insert(value);
	}
}

impl<'a> Walker<'a> {
	/// How far the open of a construct other than `{expression}` reads at the `{` at `at`.
	pub(super) fn opens_reach(&self, at: u32) -> u32 {
		let end = at as usize + 1;
		let after = self.trivia(end);
		let mut reach = end as u32;
		if let Some(&byte) = self.src.as_bytes().get(after) {
			for piece in pieces_after(byte).filter(|piece| piece.which == Which::Open) {
				reach = reach.max(match self.marker_after(end, after, piece) {
					Ok((end, _)) | Err(end) => end,
				});
			}
		}
		reach
	}

	// ── markers

	/// Whitespace and comments from `i` on: what may stand between a marker's parts.
	fn trivia(&self, mut i: usize) -> usize {
		let len = self.src.len();
		loop {
			let rest = &self.src[i..];
			i += rest.len() - rest.trim_start_matches(is_space).len();
			let rest = &self.src[i..];
			if let Some(comment) = rest.strip_prefix("/*") {
				match comment.find("*/") {
					Some(n) => i += n + 4,
					None => return len,
				}
			} else if rest.starts_with("//") {
				i += rest.find('\n').unwrap_or(rest.len());
			} else {
				return i;
			}
		}
	}

	/// How `piece`'s marker stands at the `{` that ends at `end`, `after` past the trivia after
	/// it: whole, `Ok((end, word))` with `word` where its last part's word starts; or stopped,
	/// `Err(reached)`. A stopped marker reached into a part only once it read a part before it
	/// whole, or the punctuation the part starts with. The last part of a marker that whitespace
	/// must follow may run into a word: the missing whitespace is reported then.
	fn marker_after(&self, end: usize, after: usize, piece: &Piece) -> std::result::Result<(u32, u32), u32> {
		let src = self.src;
		let runs_on = |part: &str, i: usize, last: bool| {
			!(last && piece.space) && part.ends_with(is_id_continue) && src[i..].starts_with(is_id_continue)
		};
		let mut i = end;
		for (n, part) in piece.parts.iter().enumerate() {
			let from = if n == 0 { after } else { self.trivia(i) };
			let common = src.as_bytes()[from..]
				.iter()
				.zip(part.bytes())
				.take_while(|(a, b)| **a == *b)
				.count();
			let last = n + 1 == piece.parts.len();
			if common < part.len() || runs_on(part, from + common, last) {
				let sign = part.len() - part.trim_start_matches(|c| !is_id_continue(c)).len();
				let into = common > 0 && (n > 0 || (sign > 0 && common >= sign));
				return Err(if into { (from + common) as u32 } else { i as u32 });
			}
			i = from + part.len();
			if last {
				let word = from + part.find(is_id_start).unwrap_or(0);
				return Ok((i as u32, word as u32));
			}
		}
		unreachable!()
	}

	/// The whole markers at the `{` at `at` that end furthest, into `marks` in written order;
	/// where they end, and how far a marker that stopped read.
	pub(super) fn scan(&self, at: u32, marks: &mut Vec<Mark>) -> (u32, u32) {
		marks.clear();
		let end = at as usize + 1;
		let (mut whole, mut reached) = (end as u32, end as u32);
		for construct in [Construct::Declaration, Construct::Expression] {
			marks.push(Mark {
				construct,
				which: Which::Open,
				word: at,
			});
		}
		let after = self.trivia(end);
		if let Some(&byte) = self.src.as_bytes().get(after)
			&& matches!(byte, b'#' | b':' | b'/' | b'@' | b'.')
		{
			for piece in pieces_after(byte) {
				match self.marker_after(end, after, piece) {
					Ok((end, word)) => {
						if end > whole {
							whole = end;
							marks.clear();
						}
						if end == whole {
							marks.push(Mark {
								construct: piece.construct,
								which: piece.which,
								word,
							});
						}
					}
					Err(end) => reached = reached.max(end),
				}
			}
		}
		(whole, reached)
	}

	fn innermost_block(&self) -> Option<Block> {
		match self.frames.last() {
			Some(Frame::Block(open)) => Some(open.block),
			_ => None,
		}
	}

	/// How deep the innermost open `block` stands below the top frame; MAX when none is open.
	fn depth(&self, block: Block) -> usize {
		self.frames
			.iter()
			.rev()
			.position(|frame| matches!(frame, Frame::Block(open) if open.block == block))
			.unwrap_or(usize::MAX)
	}

	/// A construct at the cursor, in content: the branch of the block it is in, a close, or the
	/// first open whose form starts there; a marker that stopped further than every whole one is
	/// an error.
	pub(super) fn construct(&mut self) -> Result<()> {
		let start = self.at;
		let mut marks = Vec::new();
		let (whole, reached) = self.scan(start, &mut marks);
		if reached > whole {
			return self.stopped(start, reached);
		}
		let inner = self.innermost_block();
		let (mut branch, mut close, mut misplaced, mut misplaced_branch) = (None, None, None, false);
		for mark in &marks {
			match mark.which {
				Which::Branch(i) if inner.is_some() && mark.construct.block() == inner => {
					branch.get_or_insert((mark.construct, i, mark.word));
				}
				Which::Branch(_) => misplaced_branch = true,
				Which::Close => {
					let block = mark.construct.block().unwrap();
					let depth = self.depth(block);
					if close.is_none_or(|(_, _, known)| depth < known) {
						close = Some((block, mark.word, depth));
					}
				}
				Which::Open if mark.construct.in_content() => {}
				Which::Open => {
					misplaced.get_or_insert((mark.construct, mark.word));
				}
			}
		}
		if let Some((construct, i, word)) = branch {
			self.at = whole;
			return self.branch(construct.block().unwrap(), i, start, word);
		}
		if let Some((block, word, _)) = close {
			self.at = whole;
			return self.close_block(block, start, word);
		}
		// the opens in written order: the first whose form starts here, unless it declines
		for mark in &marks {
			if mark.which != Which::Open || !mark.construct.in_content() || !self.claims(mark.construct, whole) {
				continue;
			}
			self.at = whole;
			self.keyword = mark.word;
			if self.open(mark.construct, start)? {
				return Ok(());
			}
		}
		if misplaced_branch {
			return self.misplaced_branch(start);
		}
		if let Some((construct, word)) = misplaced {
			self.at = whole;
			self.keyword = word;
			self.tag(construct, start)?;
			let at = self.at;
			return self.fail(
				start,
				at,
				Code::Placement,
				Some(&format!("{} in content", construct.display())),
			);
		}
		self.fail(start, start + 1, Code::UnexpectedToken, None)
	}

	/// Whether the open form of `construct` can start at `at`: a declaration starts with its keyword.
	fn claims(&mut self, construct: Construct, at: u32) -> bool {
		if construct != Construct::Declaration {
			return true;
		}
		let back = self.at;
		self.at = at;
		self.space();
		let claims = ["let", "const", "type", "var", "interface", "enum"]
			.into_iter()
			.any(|word| self.word(word));
		self.at = back;
		claims
	}

	/// A marker stopped further than every whole one: the pieces that read that far say what was
	/// expected there, or what does not stand there.
	#[cold]
	fn stopped(&mut self, start: u32, reached: u32) -> Result<()> {
		let inner = self.innermost_block();
		let mut names: Vec<&'static str> = Vec::new();
		let (mut close, mut branch) = (false, false);
		let end = start as usize + 1;
		let after = self.trivia(end);
		for piece in PIECES.iter().chain(std::iter::once(&SPREAD)) {
			if self.marker_after(end, after, piece) != Err(reached) {
				continue;
			}
			let allowed = match piece.which {
				Which::Open => piece.construct.in_content(),
				Which::Branch(_) => {
					branch = true;
					inner.is_some() && piece.construct.block() == inner
				}
				Which::Close => {
					close = true;
					false
				}
			};
			if allowed && !names.contains(&piece.display) {
				names.push(piece.display);
			}
		}
		if !names.is_empty() {
			self.report_at(reached, reached, Code::Expected, Some(&names.join(" or ")))?;
			self.skip_to("}");
			return Ok(());
		}
		if close {
			let src = self.src;
			let word = src[..reached as usize].trim_end_matches(is_id_continue).len() as u32;
			match inner {
				Some(block) => self.report_at(word, word, Code::Expected, Some(block.close()))?,
				None => {
					let rest = &src[word as usize..];
					let name = &rest[..rest.find(|c: char| !is_id_continue(c)).unwrap_or(rest.len())];
					self.report_at(start, start + 1, Code::UnexpectedClose, Some(name))?;
				}
			}
			self.skip_to("}");
			return Ok(());
		}
		if branch {
			return self.misplaced_branch(start);
		}
		self.fail(start, start + 1, Code::Placement, Some("A block or tag in content"))
	}

	/// A branch where its block is not the one open.
	fn misplaced_branch(&mut self, start: u32) -> Result<()> {
		match self.innermost_block() {
			None => {
				self.report_at(start, start + 1, Code::Placement, Some("A branch outside its block"))?;
				self.skip_to("}");
				Ok(())
			}
			Some(block @ (Block::Key | Block::Snippet)) => self.fail(
				start,
				start + 1,
				Code::Placement,
				Some(&format!("A branch in {}", block.construct().display())),
			),
			Some(block) => {
				let names = match block {
					Block::If => "{:else if} or {:else}",
					Block::Each => "{:else}",
					_ => "{:then} or {:catch}",
				};
				self.fail(start, start + 1, Code::Expected, Some(names))
			}
		}
	}

	// ── tags

	/// A tag after its marker: its node.
	pub(super) fn tag(&mut self, construct: Construct, start: u32) -> Result<NodeId> {
		if !matches!(construct, Construct::Debug | Construct::Spread | Construct::Expression) {
			self.require_space()?;
		}
		self.space();
		let kind = match construct {
			Construct::Debug => {
				let mut ids = self.nodes.take();
				loop {
					self.space();
					if self.at >= self.len() || self.matches("}") {
						break;
					}
					ids.push(self.identifier()?);
					self.space();
					if !self.eat(",") {
						break;
					}
					self.space();
					if self.at >= self.len() || self.matches("}") {
						let at = self.at;
						return self.fail(at, at, Code::Expected, Some("an identifier"));
					}
				}
				let identifiers = self.list(&ids);
				self.nodes.give(ids);
				NodeKind::DebugTag { identifiers }
			}
			Construct::Const => NodeKind::ConstTag {
				declaration: self.constant()?,
			},
			_ => {
				let expression = self.expression_of("}")?;
				match construct {
					Construct::Html => NodeKind::HtmlTag { expression },
					Construct::Render => NodeKind::RenderTag { expression },
					Construct::Attach => NodeKind::AttachTag { expression },
					Construct::Spread => NodeKind::SpreadAttribute { expression },
					_ => NodeKind::ExpressionTag { expression },
				}
			}
		};
		self.space();
		self.expect("}")?;
		let end = self.at;
		Ok(self.ast().add(kind, start, end))
	}

	/// `pattern = expression` of a `{@const}`, as the declaration it spells without its keyword.
	fn constant(&mut self) -> Result<NodeId> {
		let id = self.pattern("=")?;
		let id_start = self.tree().node(id).start;
		self.space();
		self.expect("=")?;
		self.space();
		let init_at = self.at;
		let init = self.expression_of("")?;
		let declarator_end = self.at;
		let init_node = *self.tree().node(init);
		if matches!(init_node.kind, NodeKind::SequenceExpression { .. })
			&& !self.src[init_at as usize..init_node.start as usize].contains('(')
		{
			return self.fail(
				init_node.start,
				init_node.end,
				Code::Expected,
				Some("a single declaration"),
			);
		}
		self.space();
		let end = self.at;
		let declarator = self.ast().add(
			NodeKind::VariableDeclarator { id, init: Some(init) },
			id_start,
			declarator_end,
		);
		let declarations = self.list(&[declarator]);
		let keyword = self.keyword;
		Ok(self.ast().add(
			NodeKind::VariableDeclaration {
				declarations,
				kind: VariableKind::Const,
			},
			keyword,
			end,
		))
	}

	/// A declaration after its `{`: a `let` or `const` declaration, or, when `type` reads as an
	/// expression, nothing, for the expression tag.
	fn declaration_tag(&mut self, start: u32) -> Result<bool> {
		self.space();
		if let Some(word) = ["var", "interface", "enum"].into_iter().find(|w| self.word(w)) {
			let at = self.at;
			return self.fail(
				at,
				at + word.len() as u32,
				Code::Placement,
				Some("A declaration of that kind"),
			);
		}
		let at = self.at;
		let comments = self.tree().comments.len();
		let statement = self.js(JsEntry::Statement, "}")?;
		let statement = self.first(statement);
		let kind = self.tree().node(statement).kind;
		match kind {
			NodeKind::VariableDeclaration {
				kind: VariableKind::Let | VariableKind::Const,
				..
			} => {
				self.space();
				self.expect("}")?;
				let end = self.at;
				let node = self
					.ast()
					.add(NodeKind::DeclarationTag { declaration: statement }, start, end);
				self.append(node);
				Ok(true)
			}
			// `{type}` is an expression after all
			NodeKind::ExpressionStatement { .. } => {
				self.at = at;
				self.ast().comments.truncate(comments);
				Ok(false)
			}
			_ => {
				let node = *self.tree().node(statement);
				self.fail(
					node.start,
					node.end,
					Code::Placement,
					Some("A declaration of that kind"),
				)
			}
		}
	}

	// ── blocks

	/// A construct's open after its marker: a tag, or a block, whose body opens. False when the
	/// construct declined what stands there.
	fn open(&mut self, construct: Construct, start: u32) -> Result<bool> {
		let Some(block) = construct.block() else {
			if construct == Construct::Declaration {
				return self.declaration_tag(start);
			}
			let node = self.tag(construct, start)?;
			self.append(node);
			return Ok(true);
		};
		self.require_space()?;
		let mut open = Open {
			start,
			block,
			elseif: false,
			slots: [None; 6],
			body: NONE,
			nodes: Vec::new(),
			groups: Vec::new(),
			outside: Vec::new(),
		};
		let mut group = Group {
			body: NONE,
			fields: 0,
			inside: self.nodes.take(),
		};
		self.space();
		match block {
			Block::If => {
				let test = self.expression_of("}")?;
				open.set(TEST, Value::Node(test));
				self.space();
				self.expect("}")?;
				group.body = CONSEQUENT;
			}
			Block::Each => {
				let roots = match self.trial("as", |walker| walker.stopped) {
					Some(roots) => roots,
					None => match self.trial("as ,", |walker| walker.stopped) {
						Some(roots) => roots,
						None => self.js(JsEntry::Expression, "as , }")?,
					},
				};
				let expression = self.first(roots);
				open.set(EXPRESSION, Value::Node(expression));
				self.space();
				if self.word("as") {
					self.space();
					self.at += 2;
					self.space();
					let context = self.pattern(", ( }")?;
					open.set(CONTEXT, Value::Node(context));
					group.fields |= 1 << CONTEXT;
					group.inside.push(context);
				}
				self.space();
				if self.matches(",") {
					self.space();
					self.at += 1;
					self.space();
					let index = self.identifier()?;
					open.set(INDEX, Value::Node(index));
					group.fields |= 1 << INDEX;
					group.inside.push(index);
				}
				self.space();
				if self.matches("(") {
					self.space();
					self.at += 1;
					self.space();
					let key = self.expression_of(")")?;
					open.set(KEY, Value::Node(key));
					if group.fields != 0 {
						group.fields |= 1 << KEY;
					}
					self.space();
					if !self.matches(")") {
						let at = self.at;
						return self.fail(at, at, Code::Expected, Some(")"));
					}
					self.at += 1;
				}
				self.space();
				self.expect("}")?;
				group.body = BODY;
			}
			Block::Await => {
				let expression = self.expression_of("then catch }")?;
				open.set(EXPRESSION, Value::Node(expression));
				self.space();
				let (slot, body) = if self.word("then") {
					(VALUE, THEN)
				} else if self.word("catch") {
					(ERROR, CATCH)
				} else {
					(NONE, PENDING)
				};
				if slot != NONE {
					self.space();
					self.at += if slot == VALUE { 4 } else { 5 };
					self.space();
					if self.char().is_some_and(|c| is_id_start(c) || c == '[' || c == '{') {
						self.space();
						let pattern = self.pattern("}")?;
						open.set(slot, Value::Node(pattern));
						group.fields |= 1 << slot;
						group.inside.push(pattern);
					}
				}
				self.space();
				self.expect("}")?;
				group.body = body;
			}
			Block::Key => {
				let expression = self.expression_of("}")?;
				open.set(EXPRESSION, Value::Node(expression));
				self.space();
				self.expect("}")?;
				group.body = FRAGMENT_OF_KEY;
			}
			Block::Snippet => {
				let name = self.identifier()?;
				open.set(EXPRESSION, Value::Node(name));
				open.outside.push(name);
				self.space();
				if self.matches("<") {
					self.space();
					let roots = self.js(JsEntry::TypeParameters, "")?;
					let node = *self.tree().node(self.first(roots));
					// under recovery the list can end before its `>`, or hold nothing
					let start = node.start + 1;
					let closed = node.end > start && self.src.as_bytes()[node.end as usize - 1] == b'>';
					open.set(
						TYPE_PARAMS,
						Value::Slice(start, if closed { node.end - 1 } else { node.end.max(start) }),
					);
				}
				self.space();
				let parameters = self.js(JsEntry::Params, "}")?;
				open.set(PARAMETERS, Value::Nodes(parameters));
				group.fields |= 1 << PARAMETERS;
				let ast = self.ast.as_deref().unwrap();
				group.inside.extend(ast.list(parameters).iter().flatten());
				self.space();
				self.expect("}")?;
				group.body = FRAGMENT_OF_SNIPPET;
			}
		}
		self.within_depth(start)?;
		open.body = group.body;
		open.nodes = self.nodes.take();
		open.groups = self.groups.take();
		open.groups.push(group);
		self.frames.push(Frame::Block(open));
		Ok(true)
	}

	fn branch(&mut self, block: Block, i: u8, start: u32, word: u32) -> Result<()> {
		let piece = PIECES
			.iter()
			.find(|piece| piece.construct == block.construct() && piece.which == Which::Branch(i))
			.unwrap();
		let elseif = block == Block::If && i == 0;
		// the block's frame, under an `{:else if}` node when one is open
		let listed = matches!(self.frames.last(), Some(Frame::Block(Open { elseif: true, .. })));
		let Some(Frame::Block(frame)) = self.frames.get(self.frames.len() - 1 - listed as usize) else {
			unreachable!()
		};
		let (slot, body) = match (block, i) {
			(Block::If, 0) => (TEST, CONSEQUENT),
			(Block::If, _) => (NONE, ALTERNATE),
			(Block::Each, _) => (NONE, FALLBACK),
			(Block::Await, 0) => (VALUE, THEN),
			_ => (ERROR, CATCH),
		};
		// a branch read once adds a scope to its block's: `{:else if}` after `{:else}`
		let duplicate = if elseif {
			frame.groups.len() > 1
		} else {
			frame.body == body || frame.slots[body].is_some()
		};
		if duplicate {
			return self.fail(start, start + 1, Code::Duplicate, Some(piece.display));
		}
		// the head is read, and a node too deep refused, before anything closes: a branch that
		// cannot open leaves what follows it in the body still open
		if piece.space {
			self.require_space()?;
		}
		self.keyword = word;
		let mut read = None;
		if elseif {
			self.space();
			read = Some(self.expression_of("}")?);
		} else if slot != NONE {
			self.space();
			if self.char().is_some_and(|c| is_id_start(c) || c == '[' || c == '{') {
				self.space();
				read = Some(self.pattern("}")?);
			}
		}
		self.space();
		self.expect("}")?;
		if elseif && !listed {
			self.within_depth(start)?;
		}
		if listed {
			self.pop_block(start);
		}
		let mut group = Group {
			body,
			fields: 0,
			inside: self.nodes.take(),
		};
		if elseif {
			if !listed {
				self.finish_body(BRANCHES);
			}
			let mut open = Open {
				start,
				block,
				elseif: true,
				slots: [None; 6],
				body,
				nodes: self.nodes.take(),
				groups: self.groups.take(),
				outside: Vec::new(),
			};
			open.set(TEST, Value::Node(read.unwrap()));
			open.groups.push(group);
			self.frames.push(Frame::Block(open));
			return Ok(());
		}
		self.finish_body(body);
		if let Some(pattern) = read {
			group.fields |= 1 << slot;
			group.inside.push(pattern);
		}
		let Some(Frame::Block(frame)) = self.frames.last_mut() else {
			unreachable!()
		};
		if let Some(pattern) = read {
			frame.set(slot, Value::Node(pattern));
		}
		frame.groups.push(group);
		Ok(())
	}

	/// Files the open body of the block on top, a fragment or the list of `branches`, and starts `next`.
	fn finish_body(&mut self, next: usize) {
		let empty = self.nodes.take();
		let Some(Frame::Block(open)) = self.frames.last_mut() else {
			unreachable!()
		};
		let listing = open.listing();
		let field = std::mem::replace(&mut open.body, next);
		let nodes = std::mem::replace(&mut open.nodes, empty);
		let children = if listing {
			let list = self.list(&nodes);
			self.nodes.give(nodes);
			Value::Nodes(list)
		} else {
			Value::Node(self.fragment(nodes))
		};
		let Some(Frame::Block(open)) = self.frames.last_mut() else {
			unreachable!()
		};
		open.set(field, children);
	}

	fn close_block(&mut self, block: Block, start: u32, word: u32) -> Result<()> {
		self.space();
		self.expect("}")?;
		let end = self.at;
		let ours = |frame: &Frame| matches!(frame, Frame::Block(open) if open.block == block);
		if !self.recovering() || !self.frames.iter().any(ours) {
			match self.frames.last() {
				Some(frame) if ours(frame) => {}
				// the block that is open wanted its own close here
				Some(Frame::Block(open)) => {
					let wanted = open.block.close();
					return self.report_at(word, word, Code::Expected, Some(wanted));
				}
				_ => return self.report_at(start, start + 1, Code::UnexpectedClose, Some(block.close())),
			}
		}
		while !self.frames.last().is_some_and(ours) {
			self.pop_unclosed(start)?;
		}
		// an `{:else if}` node ends where the close starts, its block after it
		if matches!(self.frames.last(), Some(Frame::Block(Open { elseif: true, .. }))) {
			self.pop_block(start);
		}
		self.pop_block(end);
		Ok(())
	}

	/// Closes the block on top at `end`: an `{:else if}` node goes into its block's `branches`,
	/// a block's node into the content around it.
	pub(super) fn pop_block(&mut self, end: u32) {
		self.finish_body(NONE);
		let Some(Frame::Block(mut open)) = self.frames.pop() else {
			unreachable!()
		};
		let shape = open.shape();
		let mut fields = [(n::name, Value::Null); 6];
		let mut position = [0usize; 6];
		let mut count = 0;
		for (i, &(name, absent)) in shape.iter().enumerate() {
			position[i] = count;
			if let Some(value) = open.slots[i].or(absent) {
				fields[count] = (name, value);
				count += 1;
			}
		}
		let base = self.ast().host_keys.len() as u32;
		let groups_at = self.ast().host_groups.len() as u32;
		for group in &open.groups {
			let until = position[group.body];
			// the body's scope opens at the first of its fields, which stand before it
			let from = if group.fields == 0 {
				until
			} else {
				position[group.fields.trailing_zeros() as usize]
			};
			let inside = self.list(&group.inside);
			let node = match fields[until].1 {
				Value::Node(fragment) => Some(fragment),
				_ => None,
			};
			self.ast().host_groups.push(HostGroup {
				inside,
				from: base + from as u32,
				until: base + until as u32 + 1,
				node,
			});
		}
		let outside = self.list(&open.outside);
		let scope = Some(Opens {
			outside,
			groups: (groups_at, open.groups.len() as u32),
		});
		let ty = if open.elseif { n::ElseIf } else { open.block.ty() };
		let node = self.opening(ty, open.start, end, &fields[..count], scope);
		self.nodes.give(std::mem::take(&mut open.nodes));
		for group in open.groups.drain(..) {
			self.nodes.give(group.inside);
		}
		self.groups.give(open.groups);
		self.append(node);
	}
}
