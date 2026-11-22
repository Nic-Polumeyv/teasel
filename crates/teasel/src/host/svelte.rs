//! Svelte: its tags, blocks and directives as plain matches over `core`, the host's rows named
//! by the numbers of `NAMES`.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};

use super::core::{Attribute, Core, Name, Pool, Shapes, closes, closing_tag, own, reserved, tag_carries, valid_name};
use crate::ast::{Ast, HostGroup, List, NodeId, NodeKind, Opens, Value, VariableKind};
use crate::error::Code;
use crate::interner::StrId;
use crate::lexer::scan::{find, is_space};
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry as JsEntry, Options, Result};

macro_rules! named {
	($($name:ident)*) => {
		/// The names of Svelte's types and fields, numbered after `own`.
		#[allow(non_upper_case_globals, dead_code)]
		pub(crate) mod n {
			use super::{Name, own};
			#[allow(non_camel_case_types)]
			#[repr(u32)]
			enum Number {
				$($name),*
			}
			$(pub(crate) const $name: Name = Name(own::NAMES.len() as u32 + Number::$name as u32);)*
			pub(super) const OWN: &[&str] = &[$(stringify!($name)),*];
		}
	};
}

named! {
	tag SvelteElement SvelteComponent SvelteSelf SvelteWindow SvelteDocument SvelteBody SvelteHead
	SvelteOptions SvelteFragment SvelteBoundary TitleElement SlotElement RegularElement Component
	BindDirective OnDirective UseDirective ClassDirective StyleDirective intro outro TransitionDirective
	AnimateDirective LetDirective test consequent ElseIf alternate IfBlock fallback index key body EachBlock
	then error r#catch pending AwaitBlock KeyBlock typeParams parameters SnippetBlock HtmlTag identifiers
	DebugTag declaration ConstTag RenderTag AttachTag SpreadAttribute DeclarationTag ExpressionTag css js
	options module instance Root
}

const COUNT: usize = own::NAMES.len() + n::OWN.len();

/// Every name by its number: `own`'s, then Svelte's.
pub(crate) const NAMES: [&str; COUNT] = {
	let mut out = [""; COUNT];
	let mut i = 0;
	while i < own::NAMES.len() {
		out[i] = own::NAMES[i];
		i += 1;
	}
	let mut j = 0;
	while j < n::OWN.len() {
		let name = n::OWN[j];
		out[i + j] = match name.as_bytes() {
			[b'r', b'#', ..] => name.split_at(2).1,
			_ => name,
		};
		j += 1;
	}
	out
};

/// Every type of node a document holds, each with the fields that hold nodes or lists of them:
/// what a walk over a document follows.
pub(crate) fn children() -> Vec<(&'static str, Vec<&'static str>)> {
	let element = ["attributes", "fragment"];
	let mut out: Vec<(&'static str, Vec<&'static str>)> = vec![
		("Root", vec!["css", "js", "module", "instance", "fragment"]),
		("Fragment", vec!["nodes"]),
		("Text", vec![]),
		("Comment", vec![]),
		("SvelteElement", vec!["attributes", "fragment", "tag"]),
		("SvelteComponent", vec!["attributes", "fragment", "expression"]),
	];
	for ty in [
		"SvelteSelf",
		"SvelteWindow",
		"SvelteDocument",
		"SvelteBody",
		"SvelteHead",
		"SvelteOptions",
		"SvelteFragment",
		"SvelteBoundary",
		"TitleElement",
		"SlotElement",
		"RegularElement",
		"Component",
	] {
		out.push((ty, element.to_vec()));
	}
	out.push(("Attribute", vec!["value"]));
	out.push(("Script", vec!["content", "attributes"]));
	for ty in [
		"BindDirective",
		"OnDirective",
		"UseDirective",
		"ClassDirective",
		"StyleDirective",
		"TransitionDirective",
		"AnimateDirective",
		"LetDirective",
	] {
		out.push((ty, vec![if ty == "StyleDirective" { "value" } else { "expression" }]));
	}
	out.extend([
		("ElseIf", vec!["test", "consequent"]),
		("IfBlock", vec!["test", "consequent", "branches", "alternate"]),
		("EachBlock", vec!["expression", "context", "index", "key", "body", "fallback"]),
		("AwaitBlock", vec!["expression", "value", "then", "error", "catch", "pending"]),
		("KeyBlock", vec!["expression", "fragment"]),
		("SnippetBlock", vec!["expression", "parameters", "fragment"]),
		("HtmlTag", vec!["expression"]),
		("DebugTag", vec!["identifiers"]),
		("ConstTag", vec!["declaration"]),
		("RenderTag", vec!["expression"]),
		("AttachTag", vec!["expression"]),
		("SpreadAttribute", vec!["expression"]),
		("DeclarationTag", vec!["declaration"]),
		("ExpressionTag", vec!["expression"]),
	]);
	out.extend(super::css::children(true));
	out
}

/// Whether the document is TypeScript: a script tag says `lang="ts"`.
pub(crate) fn typescript(src: &str) -> bool {
	tag_carries(src, "script", &[("lang", Some("ts"))])
}

thread_local! {
	/// The shapes of Svelte's rows, numbered for good as the walker meets them.
	pub(super) static SHAPES: RefCell<Shapes> = RefCell::default();
}

/// Parses a Svelte document: the host's tree with the JavaScript inside it, positions of the
/// whole source. Returns the tree and its root.
pub(crate) fn parse_document(src: &str, options: Options, reused: Option<Box<Ast>>) -> (Box<Ast>, Result<NodeId>) {
	// a component's scripts are modules
	let options = options.with(Options::MODULE);
	SHAPES.with(|shapes| {
		let full = src.len() as u32;
		let cut = src.trim_end_matches(is_space);
		let mut walker = Walker {
			core: Core::new(cut, full, shapes.borrow_mut(), options, reused),
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
		};
		let root = walker.run();
		(walker.core.ast.take().unwrap(), root)
	})
}

/// A construct that starts with `{`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Construct {
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
	fn block(self) -> Option<Block> {
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
	fn in_content(self) -> bool {
		!matches!(self, Construct::Attach | Construct::Spread)
	}

	fn ty(self) -> Name {
		match self {
			Construct::Html => n::HtmlTag,
			Construct::Debug => n::DebugTag,
			Construct::Const => n::ConstTag,
			Construct::Render => n::RenderTag,
			Construct::Attach => n::AttachTag,
			Construct::Spread => n::SpreadAttribute,
			Construct::Declaration => n::DeclarationTag,
			Construct::Expression => n::ExpressionTag,
			_ => unreachable!(),
		}
	}

	/// The marker and its end word, as messages name the open.
	fn display(self) -> &'static str {
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
enum Which {
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
struct Mark {
	construct: Construct,
	which: Which,
	word: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Block {
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

type Shape = &'static [(Name, Option<Value>)];

const NULL: Option<Value> = Some(Value::Null);

static IF: Shape = &[
	(n::test, NULL),
	(n::consequent, NULL),
	(own::branches, Some(Value::Nodes(List::EMPTY))),
	(n::alternate, NULL),
];
static ELSE_IF: Shape = &[(n::test, NULL), (n::consequent, NULL)];
static EACH: Shape = &[
	(own::expression, NULL),
	(own::context, NULL),
	(n::index, None),
	(n::key, None),
	(n::body, NULL),
	(n::fallback, None),
];
static AWAIT: Shape = &[
	(own::expression, NULL),
	(own::value, NULL),
	(n::then, NULL),
	(n::error, NULL),
	(n::r#catch, NULL),
	(n::pending, NULL),
];
static KEY_BLOCK: Shape = &[(own::expression, NULL), (own::fragment, NULL)];
static SNIPPET: Shape = &[
	(own::expression, NULL),
	(n::typeParams, None),
	(n::parameters, NULL),
	(own::fragment, NULL),
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

	fn shape(self) -> Shape {
		match self {
			Block::If => IF,
			Block::Each => EACH,
			Block::Await => AWAIT,
			Block::Key => KEY_BLOCK,
			Block::Snippet => SNIPPET,
		}
	}

	fn construct(self) -> Construct {
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
struct Group {
	body: usize,
	fields: u8,
	inside: Vec<NodeId>,
}

/// A block being read, or the `{:else if}` node being read under its if block.
struct Open {
	start: u32,
	block: Block,
	elseif: bool,
	slots: [Option<Value>; 6],
	/// The slot `nodes` will fill: a body, or `branches` while `{:else if}` nodes come.
	body: usize,
	nodes: Vec<NodeId>,
	groups: Vec<Group>,
	outside: Vec<NodeId>,
}

impl Open {
	fn shape(&self) -> Shape {
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

/// What the walker is inside: the document, an element, or a block.
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

/// A directive by the name before its `:`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Directive {
	Bind,
	On,
	Use,
	Class,
	Style,
	Transition,
	In,
	Out,
	Animate,
	Let,
}

impl Directive {
	fn of(name: &str) -> Option<Directive> {
		Some(match name {
			"bind" => Directive::Bind,
			"on" => Directive::On,
			"use" => Directive::Use,
			"class" => Directive::Class,
			"style" => Directive::Style,
			"transition" => Directive::Transition,
			"in" => Directive::In,
			"out" => Directive::Out,
			"animate" => Directive::Animate,
			"let" => Directive::Let,
			_ => return None,
		})
	}

	fn ty(self) -> Name {
		match self {
			Directive::Bind => n::BindDirective,
			Directive::On => n::OnDirective,
			Directive::Use => n::UseDirective,
			Directive::Class => n::ClassDirective,
			Directive::Style => n::StyleDirective,
			Directive::Transition | Directive::In | Directive::Out => n::TransitionDirective,
			Directive::Animate => n::AnimateDirective,
			Directive::Let => n::LetDirective,
		}
	}

	/// Without a value, the argument names the expression: `bind:value`.
	fn named(self) -> bool {
		matches!(self, Directive::Bind | Directive::Class | Directive::Let)
	}

	fn flags(self) -> Option<(bool, bool)> {
		match self {
			Directive::Transition => Some((true, true)),
			Directive::In => Some((true, false)),
			Directive::Out => Some((false, true)),
			_ => None,
		}
	}
}

/// An element by its name: its type, and what its rule says of it.
struct Rule {
	ty: Name,
	/// The field a `this` attribute fills, and whether text is taken there as a string.
	this: Option<(Name, bool)>,
	/// Only at the root, and once.
	root: bool,
	/// A `title` is its own type only inside `svelte:head`.
	title: bool,
	/// A `slot` is its own type only outside an element with `shadowrootmode`.
	slot: bool,
	raw: bool,
	rcdata: bool,
}

const fn rule(ty: Name) -> Rule {
	Rule {
		ty,
		this: None,
		root: false,
		title: false,
		slot: false,
		raw: false,
		rcdata: false,
	}
}

/// The rule of a tag's own name.
fn own_rule(name: &str) -> Option<Rule> {
	Some(match name {
		"svelte:element" => Rule {
			this: Some((n::tag, true)),
			..rule(n::SvelteElement)
		},
		"svelte:component" => Rule {
			this: Some((own::expression, false)),
			..rule(n::SvelteComponent)
		},
		"svelte:self" => rule(n::SvelteSelf),
		"svelte:window" => Rule {
			root: true,
			..rule(n::SvelteWindow)
		},
		"svelte:document" => Rule {
			root: true,
			..rule(n::SvelteDocument)
		},
		"svelte:body" => Rule {
			root: true,
			..rule(n::SvelteBody)
		},
		"svelte:head" => Rule {
			root: true,
			..rule(n::SvelteHead)
		},
		"svelte:options" => Rule {
			root: true,
			..rule(n::SvelteOptions)
		},
		"svelte:fragment" => rule(n::SvelteFragment),
		"svelte:boundary" => rule(n::SvelteBoundary),
		"title" => Rule {
			title: true,
			..rule(n::TitleElement)
		},
		"slot" => Rule {
			slot: true,
			..rule(n::SlotElement)
		},
		"textarea" => Rule {
			rcdata: true,
			..rule(n::RegularElement)
		},
		"script" | "style" => Rule {
			raw: true,
			..rule(n::RegularElement)
		},
		_ => return None,
	})
}

/// A component name: capitalized, or a dotted path of identifiers.
fn component_name(name: &str) -> bool {
	let mut chars = name.chars();
	let Some(first) = chars.next() else { return false };
	if first.is_uppercase() {
		return chars.all(|c| is_id_continue(c) || c == '.');
	}
	if !is_id_start(first) || !name.contains('.') {
		return false;
	}
	let mut parts = name.split('.');
	parts.next().is_some_and(|part| part.chars().all(is_id_continue))
		&& parts.all(|part| !part.is_empty() && part.chars().all(is_id_continue))
}

fn is_void(name: &str) -> bool {
	name.starts_with('!')
		|| matches!(
			name,
			"area"
				| "base" | "br"
				| "col" | "command"
				| "embed" | "hr"
				| "img" | "input"
				| "keygen" | "link"
				| "meta" | "param"
				| "source" | "track"
				| "wbr"
		)
}

struct Walker<'a> {
	core: Core<'a>,
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

impl<'a> Deref for Walker<'a> {
	type Target = Core<'a>;

	fn deref(&self) -> &Core<'a> {
		&self.core
	}
}

impl<'a> DerefMut for Walker<'a> {
	fn deref_mut(&mut self) -> &mut Core<'a> {
		&mut self.core
	}
}

impl<'a> Walker<'a> {
	// the core's, called with arguments read off the walker: a call through `DerefMut` borrows it first
	fn fail<T>(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<T> {
		self.core.fail(pos, end, code, arg)
	}

	fn report_at(&mut self, pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<()> {
		self.core.report_at(pos, end, code, arg)
	}

	fn host(&mut self, ty: Name, start: u32, end: u32, fields: &[(Name, Value)]) -> NodeId {
		self.core.host(ty, start, end, fields)
	}

	fn intern(&mut self, s: &str) -> StrId {
		self.core.intern(s)
	}

	fn text_chunk(&mut self, start: u32, end: u32, attribute: bool) -> NodeId {
		self.core.text_chunk(start, end, attribute)
	}

	fn within_depth(&mut self, at: u32) -> Result<()> {
		if self.frames.len() as u32 >= crate::parser::MAX_DEPTH {
			return self.fail(at, at + 1, Code::NestingDepth, None);
		}
		Ok(())
	}

	fn give_declared(&mut self, declared: Vec<NodeId>) {
		self.nodes.give(declared);
	}

	fn fragment(&mut self, nodes: Vec<NodeId>) -> NodeId {
		let fragment = self.core.fragment(&nodes);
		self.nodes.give(nodes);
		fragment
	}

	fn append(&mut self, node: NodeId) {
		match self.frames.last_mut().unwrap() {
			Frame::Root { nodes, .. } | Frame::Element { nodes, .. } | Frame::Block(Open { nodes, .. }) => {
				nodes.push(node)
			}
		}
	}

	/// The expression one reads in `{expression}`.
	fn expression_of(&mut self, stop: &str) -> Result<NodeId> {
		let roots = self.js(JsEntry::Expression, stop)?;
		Ok(self.first(roots))
	}

	fn pattern(&mut self, stop: &str) -> Result<NodeId> {
		let roots = self.js(JsEntry::Pattern, stop)?;
		Ok(self.first(roots))
	}

	// ── the document

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
				self.core.report(error)?;
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
		let mut fields = [(own::fragment, Value::Null); 7];
		let mut count = 4;
		fields[..4].copy_from_slice(&[
			(n::css, css.map_or(Value::Null, Value::Node)),
			(n::js, Value::Nodes(List::EMPTY)),
			(n::options, Value::Null),
			(own::comments, Value::Comments),
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
		fields[count] = (own::fragment, Value::Node(fragment));
		count += 1;
		// the scope belongs to the instance script's program, or to the fragment
		let node = match instance {
			Some(script) => match self.field_of(script, own::content) {
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
		Ok(self.core.opening(n::Root, 0, full, &fields[..count], scope))
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

	// ── elements

	/// Whether the nearest element around the cursor, past blocks and meta elements, is `svelte:head`.
	fn inside_head(&self) -> bool {
		for frame in self.frames.iter().rev() {
			if let Frame::Element { name: span, ty, .. } = frame {
				if &self.src[span.0 as usize..span.1 as usize] == "svelte:head" {
					return true;
				}
				if *ty == n::RegularElement || *ty == n::Component {
					return false;
				}
			}
		}
		false
	}

	fn element(&mut self) -> Result<()> {
		let start = self.at;
		self.at += 1;
		if self.eat("!--") {
			let Some(len) = self.rest().find("-->") else {
				let len = self.len();
				return self.fail(len, len, Code::Expected, Some("-->"));
			};
			let data_start = self.at;
			self.at += len as u32 + 3;
			let end = self.at;
			let node = self.host(own::Comment, start, end, &[(own::data, Value::Slice(data_start, end - 3))]);
			self.append(node);
			return Ok(());
		}
		if self.eat("/") {
			let name = self.tag_name(false)?;
			self.space();
			self.expect(">")?;
			// a void element is never open
			let open = match self.frames.last() {
				Some(Frame::Element { name: span, .. }) => &self.src[span.0 as usize..span.1 as usize] == name,
				_ => false,
			};
			if !open && is_void(name) {
				return self.report_at(
					start,
					start + 1,
					Code::Placement,
					Some("A closing tag of a void element"),
				);
			}
			return self.close_element(start, name);
		}
		let name = self.tag_name(false)?;
		let name_span = (start + 1, self.at);
		let own = own_rule(name);
		if own.is_none() && name.starts_with("svelte:") {
			return self.fail(name_span.0, name_span.1, Code::InvalidName, Some(name));
		}
		let typing = self.recovering() && name.ends_with('.') && component_name(&format!("{name}_"));
		let rule = match own {
			Some(rule) => rule,
			None if component_name(name) => rule(n::Component),
			None if valid_name(name) => rule(n::RegularElement),
			None if typing => rule(n::Component),
			None => return self.fail(name_span.0, name_span.1, Code::InvalidName, Some(name)),
		};
		if rule.root && self.frames.len() > 1 {
			self.report_at(start, start + 1, Code::Placement, Some(name))?;
		}
		if rule.root {
			if self.once.contains(&rule.ty) {
				self.report_at(start, start + 1, Code::Duplicate, Some(name))?;
			} else {
				self.once.push(rule.ty);
			}
		}
		let plain = n::RegularElement;
		let mut ty = rule.ty;
		if rule.title && !self.inside_head() {
			ty = plain;
		}
		if rule.slot && self.frames.iter().any(|frame| matches!(frame, Frame::Element { shadow: true, .. })) {
			ty = plain;
		}
		self.space();
		// the browser closes the open element when this one cannot sit inside it
		if let Some(Frame::Element {
			name: parent,
			ty: parent_ty,
			..
		}) = self.frames.last()
		{
			let src = self.src;
			let parent_name = &src[parent.0 as usize..parent.1 as usize];
			if *parent_ty == plain && closes(parent_name, name) {
				self.close_top(start);
				self.autoclosed = Some((parent_name, name, self.frames.len()));
			}
		}
		let at_root = self.frames.len() == 1;
		let script = name == "script" && at_root;
		let style = name == "style" && at_root;
		let mut attributes = self.nodes.take();
		let mut seen = std::mem::take(&mut self.seen);
		seen.clear();
		let mut shadow = false;
		self.declared.clear();
		loop {
			let attribute = if script || style {
				self.static_attribute()?
			} else {
				self.attribute()?
			};
			let Some((node, key)) = attribute else { break };
			if let Some((kind, key)) = key {
				let text = self.tree().strings.get(key);
				if kind == own::Attribute && ty == plain && text == "shadowrootmode" {
					shadow = true;
				}
				let this = text == "this";
				if seen.contains(&(kind, key)) {
					let (at, end) = (self.tree().node(node).start, self.tree().node(node).end);
					let text = text.to_string();
					self.report_at(at, end, Code::Duplicate, Some(&text))?;
				}
				if !this {
					seen.push((kind, key));
				}
			}
			attributes.push(node);
			self.space();
		}
		self.seen = seen;
		let declared = std::mem::take(&mut self.declared);
		let mut this = None;
		if let Some((field, text)) = rule.this {
			let position = attributes.iter().position(|&id| self.attribute_named(id, "this"));
			let value = match position {
				None => {
					self.report_at(start, start + 1, Code::Expected, Some("a this attribute"))?;
					self.placeholder(name_span.1, name_span.1)
				}
				Some(position) => {
					let id = attributes.remove(position);
					match self.attribute_chunk(id).and_then(|chunk| self.chunk_expression(chunk)) {
						Some(expression) => expression,
						None => {
							let chunk = self
								.attribute_chunk(id)
								.filter(|&chunk| text && self.host_type(chunk) == Some(own::Text));
							match chunk {
								Some(chunk) => {
									let node = *self.tree().node(chunk);
									let value = match self.field_of(chunk, own::data) {
										Some(Value::Str(s)) => s,
										Some(Value::Slice(a, b)) => {
											let src = self.src;
											self.intern(&src[a as usize..b as usize])
										}
										_ => unreachable!(),
									};
									self.ast().add(NodeKind::StringLiteral { value }, node.start, node.end)
								}
								None => {
									let node = *self.tree().node(id);
									self.report_at(
										node.start,
										node.end,
										Code::Expected,
										Some("an expression as this"),
									)?;
									self.placeholder(node.start, node.end)
								}
							}
						}
					}
				}
			};
			this = Some((field, Value::Node(value)));
		}
		if script {
			return self.script(start, name, attributes);
		}
		if style {
			self.expect(">")?;
			let node = self.style_sheet(start, name, attributes)?;
			let Some(Frame::Root { css, .. }) = self.frames.first_mut() else {
				unreachable!()
			};
			if css.is_none() {
				*css = Some(node);
				return Ok(());
			}
			return self.report_at(start, start + 1, Code::Duplicate, Some(name));
		}
		let self_closing = self.eat("/") || is_void(name);
		let unclosed = !self.eat(">");
		if unclosed {
			let at = self.at;
			self.report_at(at, at, Code::Expected, Some(">"))?;
		}
		let name_id = self.intern(name);
		if self_closing || unclosed {
			let end = self.at;
			let nodes = self.nodes.take();
			self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
			return Ok(());
		}
		if rule.rcdata {
			let nodes = self.sequence(Done::Closing(name), &format!("<{name}>"), JsEntry::Expression)?;
			if let Some(len) = closing_tag(self.rest(), name) {
				self.at += len as u32;
			}
			let end = self.at;
			self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
			return Ok(());
		}
		if rule.raw {
			let content_start = self.at;
			let (close, end) = self.closing(name);
			self.at = end;
			let text = Value::Slice(content_start, close);
			let node = self.host(own::Text, content_start, close, &[(own::raw, text), (own::data, text)]);
			if close == self.len() {
				let len = self.len();
				self.report_at(len, len, Code::Unclosed, Some(name))?;
			}
			let mut nodes = self.nodes.take();
			nodes.push(node);
			self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
			return Ok(());
		}
		let nodes = self.nodes.take();
		self.within_depth(start)?;
		self.frames.push(Frame::Element {
			start,
			name: name_span,
			name_id,
			ty,
			attributes,
			this,
			nodes,
			shadow,
			declared,
		});
		Ok(())
	}

	/// A script at the root, after its attributes: the instance's or the module's.
	fn script(&mut self, start: u32, name: &str, attributes: Vec<NodeId>) -> Result<()> {
		self.expect(">")?;
		let content_start = self.at;
		let (close, end) = self.closing(name);
		if close == self.len() {
			let len = self.len();
			self.report_at(len, len, Code::Unclosed, Some(name))?;
		}
		let program = self.program(content_start, close)?;
		self.at = end;
		let mut module = false;
		for &id in &attributes {
			if self.attribute_named(id, "context") {
				if self.attribute_text(id) != Some("module") {
					let node = *self.tree().node(id);
					self.report_at(node.start, node.end, Code::Expected, Some("context to be \"module\""))?;
				} else {
					module = true;
				}
			}
			if self.attribute_named(id, "module") {
				if !matches!(self.field_of(id, own::value), Some(Value::Bool(true))) {
					let node = *self.tree().node(id);
					self.report_at(node.start, node.end, Code::Expected, Some("module without a value"))?;
				} else {
					module = true;
				}
			}
		}
		let context = self.intern(if module { "module" } else { "default" });
		let list = self.list(&attributes);
		self.nodes.give(attributes);
		let end = self.at;
		let node = self.host(
			own::Script,
			start,
			end,
			&[
				(own::context, Value::Str(context)),
				(own::content, Value::Node(program)),
				(own::attributes, Value::Nodes(list)),
			],
		);
		let Some(Frame::Root {
			instance,
			module: module_slot,
			..
		}) = self.frames.first_mut()
		else {
			unreachable!()
		};
		let slot = if module { module_slot } else { instance };
		if slot.is_none() {
			*slot = Some(node);
			return Ok(());
		}
		self.report_at(start, start + 1, Code::Duplicate, Some(name))
	}

	#[allow(clippy::too_many_arguments)]
	fn element_node(
		&mut self,
		ty: Name,
		(start, end): (u32, u32),
		name: StrId,
		this: Option<(Name, Value)>,
		attributes: Vec<NodeId>,
		nodes: Vec<NodeId>,
		declared: &[NodeId],
	) {
		let list = self.list(&attributes);
		self.nodes.give(attributes);
		let fragment = self.fragment(nodes);
		let mut fields = [(own::name, Value::Str(name)); 4];
		let mut count = 1;
		if let Some(this) = this {
			fields[1] = this;
			count = 2;
		}
		let at = count;
		fields[count] = (own::attributes, Value::Nodes(list));
		fields[count + 1] = (own::fragment, Value::Node(fragment));
		let scope = self.element_scope(at, fragment, declared);
		let node = self.core.opening(ty, start, end, &fields[..count + 2], Some(scope));
		self.append(node);
	}

	/// Closes the element on top of the stack at `end`.
	fn close_top(&mut self, end: u32) {
		let Some(Frame::Element {
			start,
			name_id,
			ty,
			attributes,
			this,
			nodes,
			declared,
			..
		}) = self.frames.pop()
		else {
			unreachable!()
		};
		self.element_node(ty, (start, end), name_id, this, attributes, nodes, &declared);
		self.give_declared(declared);
	}

	fn close_element(&mut self, start: u32, name: &str) -> Result<()> {
		if let Some((_, _, depth)) = self.autoclosed
			&& self.frames.len() < depth
		{
			self.autoclosed = None;
		}
		let autoclosed = self.autoclosed;
		let closed = move || match autoclosed {
			Some((closed, by, _)) if closed == name => format!("{name}, closed by {by}"),
			_ => name.to_string(),
		};
		// under recovery a closing tag that closes nothing is skipped, and one that closes an
		// element further out closes what is open inside it
		let opens = |w: &Self| {
			w.frames.iter().any(
				|frame| matches!(frame, Frame::Element { name: span, .. } if &w.src[span.0 as usize..span.1 as usize] == name),
			)
		};
		if self.recovering() && !opens(self) {
			return self.report_at(start, start + 1, Code::UnexpectedClose, Some(&closed()));
		}
		loop {
			match self.frames.last() {
				Some(Frame::Element { name: span, ty, .. }) => {
					let open = &self.src[span.0 as usize..span.1 as usize];
					if open == name {
						let end = self.at;
						self.close_top(end);
						return Ok(());
					}
					if *ty != n::RegularElement {
						self.report_at(start, start + 1, Code::UnexpectedClose, Some(name))?;
					}
					// the browser closes it here
					self.close_top(start);
				}
				Some(Frame::Block(_)) if self.recovering() => self.pop_unclosed(start)?,
				_ => return self.fail(start, start + 1, Code::UnexpectedClose, Some(&closed())),
			}
		}
	}

	// ── attributes

	/// The expression node of a chunk written as one, `{expression}`.
	fn chunk_expression(&self, chunk: NodeId) -> Option<NodeId> {
		if self.host_type(chunk) != Some(n::ExpressionTag) {
			return None;
		}
		match self.field_of(chunk, own::expression)? {
			Value::Node(expression) => Some(expression),
			_ => None,
		}
	}

	/// One attribute: a plain one, a shorthand, a spread, an attachment or a directive.
	fn attribute(&mut self) -> Result<Option<Attribute>> {
		while self.comment_between_attributes() {
			self.space();
		}
		let start = self.at;
		if self.byte() == Some(b'{') {
			return self.among_attributes(start);
		}
		let name = self.tag_name(true)?;
		if name.is_empty() || (self.recovering() && name.starts_with('<')) {
			self.at = start;
			return Ok(None);
		}
		let name_end = self.at;
		// a directive: its kind, its argument's text and start, and its modifiers
		let directive = match name.split_once(':') {
			Some((head, rest)) => match Directive::of(head) {
				Some(kind) => {
					let len = rest.find('|').unwrap_or(rest.len());
					if len == 0 {
						return self.fail(
							start,
							start + name.len() as u32,
							Code::Expected,
							Some("a directive name"),
						);
					}
					let arg_start = start + head.len() as u32 + 1;
					Some((kind, &rest[..len], arg_start, &rest[len..]))
				}
				None => None,
			},
			None => None,
		};
		let mut end = name_end;
		self.space();
		let has_value = self.eat("=");
		if !has_value && self.char().is_some_and(|c| c == '"' || c == '\'') {
			let at = self.at;
			return self.fail(at, at, Code::Expected, Some("="));
		}
		if has_value {
			self.space();
		}
		let Some((kind, arg, arg_start, modifiers)) = directive else {
			let value = if has_value {
				self.plain_value(JsEntry::Expression)?
			} else {
				Value::Bool(true)
			};
			if has_value {
				end = self.at;
			}
			let name_id = self.intern(name);
			let node = self.host(
				own::Attribute,
				start,
				end,
				&[(own::name, Value::Str(name_id)), (own::value, value)],
			);
			return Ok(Some((node, Some((own::Attribute, name_id)))));
		};
		let arg_id = self.intern(arg);
		let mut fields = [(own::name, Value::Str(arg_id)); 5];
		let value = if kind == Directive::Style {
			let value = if has_value {
				self.plain_value(JsEntry::Expression)?
			} else {
				Value::Bool(true)
			};
			if has_value {
				end = self.at;
			}
			(own::value, value)
		} else {
			let entry = if kind == Directive::Let {
				JsEntry::Pattern
			} else {
				JsEntry::Expression
			};
			let value = if has_value {
				self.plain_value(entry)?
			} else {
				Value::Bool(true)
			};
			if has_value {
				end = self.at;
			}
			let expression = match value {
				Value::Node(tag) => self.chunk_expression(tag),
				Value::Nodes(list) => match *self.tree().list(list) {
					[Some(chunk)] if self.chunk_expression(chunk).is_some() => self.chunk_expression(chunk),
					[Some(chunk), ..] => {
						let node = *self.tree().node(chunk);
						return self.fail(node.start, node.end, Code::Expected, Some("an expression, not text"));
					}
					_ => None,
				},
				_ => None,
			};
			let expression = match expression {
				Some(e) => Value::Node(e),
				None if kind.named() => {
					let id = self.intern(arg);
					Value::Node(self.ast().add(NodeKind::Identifier { name: id }, arg_start, end))
				}
				None => Value::Null,
			};
			if let (Directive::Let, Value::Node(pattern)) = (kind, expression) {
				self.declared.push(pattern);
			}
			(own::expression, expression)
		};
		fields[1] = value;
		let from = self.ast().host_strings.len() as u32;
		let mut count = 0;
		for modifier in modifiers.split('|').filter(|m| !m.is_empty()) {
			let id = self.intern(modifier);
			self.ast().host_strings.push(id);
			count += 1;
		}
		fields[2] = (own::modifiers, Value::Strs(from, count));
		let mut len = 3;
		if let Some((intro, outro)) = kind.flags() {
			fields[3] = (n::intro, Value::Bool(intro));
			fields[4] = (n::outro, Value::Bool(outro));
			len = 5;
		}
		let node = self.host(kind.ty(), start, end, &fields[..len]);
		let key = match kind {
			Directive::Class | Directive::Style => Some((kind.ty(), self.intern(arg))),
			Directive::Bind => Some((own::Attribute, self.intern(arg))),
			_ => None,
		};
		Ok(Some((node, key)))
	}

	/// An attribute value: text with expressions, or one expression on its own; a slash right
	/// before `>` is the value, `<a href=/>`.
	fn plain_value(&mut self, entry: JsEntry) -> Result<Value> {
		if self.matches("/>") {
			let slash = self.at;
			self.at += 1;
			let text = self.text_chunk(slash, slash + 1, true);
			let list = self.list(&[text]);
			return Ok(Value::Nodes(list));
		}
		let quote = self.char().filter(|c| matches!(c, '"' | '\''));
		if let Some(q) = quote {
			self.at += 1;
			if self.char() == Some(q) {
				let at = self.at;
				self.at += 1;
				let text = self.text_chunk(at, at, true);
				let list = self.list(&[text]);
				return Ok(Value::Nodes(list));
			}
		}
		let done = match quote {
			Some(q) => Done::Quote(q),
			None => Done::Bare,
		};
		let chunks = self.sequence(done, "an attribute value", entry)?;
		if chunks.is_empty() && quote.is_none() {
			let at = self.at;
			return self.fail(at, at, Code::Expected, Some("an attribute value"));
		}
		if quote.is_some() && self.char() == quote {
			self.at += 1;
		}
		let value = if quote.is_some() || chunks.len() > 1 || self.host_type(chunks[0]) == Some(own::Text) {
			Value::Nodes(self.list(&chunks))
		} else {
			Value::Node(chunks[0])
		};
		self.nodes.give(chunks);
		Ok(value)
	}

	/// Text and expression chunks up to where `done` says.
	fn sequence(&mut self, done: Done, place: &str, entry: JsEntry) -> Result<Vec<NodeId>> {
		let mut chunks = self.nodes.take();
		let mut chunk_start = self.at;
		loop {
			if self.at >= self.len() {
				let len = self.len();
				self.report_at(len, len, Code::UnexpectedEof, None)?;
				let at = self.at;
				self.flush_text(chunk_start, at, &mut chunks);
				return Ok(chunks);
			}
			let finished = match done {
				Done::Quote(q) => self.char() == Some(q),
				Done::Bare => {
					self.matches("/>")
						|| self.char().is_none_or(|c| is_space(c) || matches!(c, '"' | '\'' | '=' | '<' | '>' | '`'))
				}
				Done::Closing(name) => closing_tag(self.rest(), name).is_some(),
			};
			if finished {
				let at = self.at;
				self.flush_text(chunk_start, at, &mut chunks);
				return Ok(chunks);
			}
			if self.byte() == Some(b'{') {
				let start = self.at;
				// one expression here, unless the open of something else reads further
				if self.opens_reach(start) > start + 1 {
					return self.fail(start, start + 1, Code::Placement, Some(&format!("A block or tag in {place}")));
				}
				self.flush_text(chunk_start, start, &mut chunks);
				self.at = start + 1;
				self.space();
				if self.matches("/>") || self.matches(">") {
					let at = self.at;
					return self.fail(at, at, Code::Expected, Some("}"));
				}
				let roots = self.js(entry, "}")?;
				let expression = self.first(roots);
				self.space();
				self.expect("}")?;
				let end = self.at;
				let tag = self.host(n::ExpressionTag, start, end, &[(own::expression, Value::Node(expression))]);
				chunks.push(tag);
				chunk_start = self.at;
				continue;
			}
			self.at += self.char().map_or(1, |c| c.len_utf8() as u32);
		}
	}

	/// How far the open of a construct other than `{expression}` reads at the `{` at `at`.
	fn opens_reach(&self, at: u32) -> u32 {
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

	/// What a `{` among the attributes starts: a tag that stands there, the shorthand `{name}`,
	/// or under recovery the end of the attributes.
	fn among_attributes(&mut self, start: u32) -> Result<Option<Attribute>> {
		let mut marks = Vec::new();
		let (whole, reached) = self.scan(start, &mut marks);
		let shorthand_end = start + 1;
		let mut found: Option<Mark> = None;
		if reached <= whole && whole > shorthand_end {
			for mark in &marks {
				if mark.which == Which::Open
					&& found.is_none_or(|known| !mark.construct.in_content() && known.construct.in_content())
				{
					found = Some(*mark);
				}
			}
		}
		if let Some(mark) = found
			&& mark.construct.block().is_none()
		{
			self.at = whole;
			self.keyword = mark.word;
			let node = self.tag(mark.construct, start)?;
			if mark.construct.in_content() {
				let at = self.at;
				return self.fail(
					start,
					at,
					Code::Placement,
					Some(&format!("{} among attributes", mark.construct.display())),
				);
			}
			return Ok(Some((node, None)));
		}
		if self.recovering() && (whole > shorthand_end || reached > shorthand_end) {
			// under recovery a block among the attributes ends them
			self.at = start;
			return Ok(None);
		}
		self.at += 1;
		self.space();
		if self.matches("/>") || self.matches(">") {
			let at = self.at;
			return self.fail(at, at, Code::Expected, Some("}"));
		}
		let id_start = self.at;
		let id = self.identifier()?;
		let id_end = self.at;
		let name = &self.src[id_start as usize..id_end as usize];
		if reserved(name) {
			return self.fail(id_start, id_end, Code::ReservedWord, Some(name));
		}
		self.space();
		self.expect("}")?;
		let tag = self.host(n::ExpressionTag, id_start, id_end, &[(own::expression, Value::Node(id))]);
		let name_id = self.intern(name);
		let end = self.at;
		let node = self.host(
			own::Attribute,
			start,
			end,
			&[(own::name, Value::Str(name_id)), (own::value, Value::Node(tag))],
		);
		Ok(Some((node, Some((own::Attribute, name_id)))))
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
	fn scan(&self, at: u32, marks: &mut Vec<Mark>) -> (u32, u32) {
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
	fn construct(&mut self) -> Result<()> {
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
	fn tag(&mut self, construct: Construct, start: u32) -> Result<NodeId> {
		if !matches!(construct, Construct::Debug | Construct::Spread | Construct::Expression) {
			self.require_space()?;
		}
		self.space();
		let (field, value) = match construct {
			Construct::Debug => {
				let mut ids = self.nodes.take();
				loop {
					self.space();
					if self.at >= self.limit || self.matches("}") {
						break;
					}
					ids.push(self.identifier()?);
					self.space();
					if !self.eat(",") {
						break;
					}
					self.space();
					if self.at >= self.limit || self.matches("}") {
						let at = self.at;
						return self.fail(at, at, Code::Expected, Some("an identifier"));
					}
				}
				let list = self.list(&ids);
				self.nodes.give(ids);
				(n::identifiers, Value::Nodes(list))
			}
			Construct::Const => (n::declaration, Value::Node(self.constant()?)),
			_ => (own::expression, Value::Node(self.expression_of("}")?)),
		};
		self.space();
		self.expect("}")?;
		let end = self.at;
		Ok(self.host(construct.ty(), start, end, &[(field, value)]))
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
				let node = self.host(n::DeclarationTag, start, end, &[(n::declaration, Value::Node(statement))]);
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
				let roots = match self.trial("as", |core| core.stopped) {
					Some(roots) => roots,
					None => match self.trial("as ,", |core| core.stopped) {
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
				let ast = self.core.ast.as_deref().unwrap();
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

	/// Closes the block on top at `end`: an `{:else if}` node goes into its block's `branches`,
	/// a block's node into the content around it.
	fn pop_block(&mut self, end: u32) {
		self.finish_body(NONE);
		let Some(Frame::Block(mut open)) = self.frames.pop() else {
			unreachable!()
		};
		let shape = open.shape();
		let mut fields = [(own::name, Value::Null); 6];
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
		let node = self.core.opening(ty, open.start, end, &fields[..count], scope);
		self.nodes.give(std::mem::take(&mut open.nodes));
		for group in open.groups.drain(..) {
			self.nodes.give(group.inside);
		}
		self.groups.give(open.groups);
		self.append(node);
	}
}

/// Where a run of text and expressions ends.
#[derive(Clone, Copy)]
enum Done<'n> {
	Quote(char),
	Bare,
	Closing(&'n str),
}
