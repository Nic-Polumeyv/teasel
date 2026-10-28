use crate::handed::{Handed, Views};
use crate::interner::{Interner, StrId, Surrogate};
use crate::layout::{Field, Ty};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Comment {
	pub kind: CommentKind,
	pub start: u32,
	pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CommentKind {
	Line,
	Block,
	/// A block comment the input ends inside, kept under recovery.
	Unclosed,
	Hashbang,
	HtmlOpen,
	HtmlClose,
}

impl Comment {
	pub fn is_block(&self) -> bool {
		matches!(self.kind, CommentKind::Block | CommentKind::Unclosed)
	}

	/// Byte range of the comment text, without its delimiters.
	pub fn text_range(&self) -> std::ops::Range<usize> {
		let (prefix, suffix) = match self.kind {
			CommentKind::Line | CommentKind::Hashbang | CommentKind::Unclosed => (2, 0),
			CommentKind::Block => (2, 2),
			CommentKind::HtmlOpen => (4, 0),
			CommentKind::HtmlClose => (3, 0),
		};
		self.start as usize + prefix..self.end as usize - suffix
	}
}

/// Index of a node in `Ast::nodes`, kept plus one so an `Option<NodeId>` is the same four bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(std::num::NonZero<u32>);

impl NodeId {
	pub fn at(index: u32) -> Self {
		NodeId(std::num::NonZero::new(index + 1).unwrap())
	}

	pub fn index(self) -> u32 {
		self.0.get() - 1
	}
}

/// A bit per node.
#[derive(Debug, Default)]
pub struct NodeSet(Handed<u64>);

impl NodeSet {
	pub fn insert(&mut self, id: NodeId) {
		let index = id.index() as usize;
		if self.0.len() <= index / 64 {
			self.0.resize(index / 64 + 1, 0);
		}
		self.0[index / 64] |= 1 << (index % 64);
	}

	pub fn words(&mut self) -> &mut Handed<u64> {
		&mut self.0
	}

	pub fn contains(&self, id: NodeId) -> bool {
		let index = id.index() as usize;
		self.0
			.get(index / 64)
			.is_some_and(|word| word & (1 << (index % 64)) != 0)
	}

	pub fn clear(&mut self) {
		self.0.clear();
	}

	/// Empty, with a bit for each of `nodes` nodes.
	pub fn reset(&mut self, nodes: usize) {
		self.0.clear();
		self.0.resize(nodes.div_ceil(64), 0);
	}

	pub fn union(&mut self, other: &NodeSet) {
		for (word, &more) in self.0.iter_mut().zip(other.0.iter()) {
			*word |= more;
		}
	}

	/// Keeps the bits of the first `nodes` nodes.
	pub fn truncate(&mut self, nodes: usize) {
		self.0.truncate(nodes.div_ceil(64));
		if let Some(last) = self.0.get_mut(nodes / 64) {
			*last &= (1 << (nodes % 64)) - 1;
		}
	}
}

/// A few nodes' records, found through a slot per node: zero for none, else the record's index
/// plus one.
#[derive(Debug, Default)]
pub struct Slots<T: Copy> {
	slots: Handed<u32>,
	list: Handed<T>,
	/// The node of each record, to unhook when the record is forgotten.
	owners: Vec<NodeId>,
}

impl<T: Copy + 'static> Slots<T> {
	pub fn get(&self, id: NodeId) -> Option<&T> {
		match self.slots.get(id.index() as usize) {
			Some(&slot) if slot != 0 => Some(&self.list[slot as usize - 1]),
			_ => None,
		}
	}

	pub fn entry(&mut self, id: NodeId) -> &mut T
	where
		T: Default,
	{
		let index = id.index() as usize;
		if index >= self.slots.len() {
			self.slots.resize(index + 1, 0);
		}
		if self.slots[index] == 0 {
			self.list.push(T::default());
			self.owners.push(id);
			self.slots[index] = self.list.len() as u32;
		}
		&mut self.list[self.slots[index] as usize - 1]
	}

	pub fn clear(&mut self) {
		self.slots.clear();
		self.list.clear();
		self.owners.clear();
	}

	pub fn len(&self) -> usize {
		self.list.len()
	}

	/// The node of each record.
	pub fn owners(&self) -> &[NodeId] {
		&self.owners
	}

	pub fn is_empty(&self) -> bool {
		self.list.is_empty()
	}

	/// Forgets the records past the first `len`.
	pub fn truncate(&mut self, len: usize) {
		for &owner in &self.owners[len..] {
			self.slots[owner.index() as usize] = 0;
		}
		self.list.truncate(len);
		self.owners.truncate(len);
	}

	pub fn views(&mut self, name: &'static str, records: &'static str, out: &mut Views<'_>) {
		out.push(name, &mut self.slots);
		out.push(records, &mut self.list);
	}
}

impl std::fmt::Debug for NodeId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "NodeId({})", self.index())
	}
}

/// A contiguous run of node ids in `Ast::lists`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct List {
	pub start: u32,
	pub len: u32,
}

impl List {
	pub const EMPTY: List = List { start: 0, len: 0 };
}

// decode.ts reads a span as its start and the word after, in a node and in `Ast::spans` alike
const _: () = assert!(
	std::mem::offset_of!(Node, end) == std::mem::offset_of!(Node, start) + 4
		&& std::mem::offset_of!(Span, end) == std::mem::offset_of!(Span, start) + 4
);

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Node {
	pub kind: NodeKind,
	pub start: u32,
	pub end: u32,
}

// every node is as wide as JavaScript's widest kind: a TypeScript kind past that moves a field to
// the extras record
const _: () = assert!(size_of::<Node>() == 32);

/// A node of a host's grammar: its type and its fields are the grammar's, held by name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Host {
	pub ty: &'static str,
	/// The node's fields, a run of `Ast::host_fields` in source order.
	pub fields: (u32, u32),
	/// Whether the node has a span; a fragment has none.
	pub span: bool,
	/// The scope the node opens, when it opens one.
	pub scope: Option<Opens>,
}

/// The scopes a host node opens: the patterns it declares around itself, and its groups, a run
/// of `Ast::host_groups`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opens {
	pub outside: List,
	pub groups: (u32, u32),
}

/// One scope a host node opens over a run of its fields: the patterns declared in it, the
/// fields inside it, and the node the scope belongs to when it is not the host node itself, a
/// body's fragment say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostGroup {
	pub inside: List,
	pub from: u32,
	pub until: u32,
	pub node: Option<NodeId>,
}

/// A host node's field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
	Node(NodeId),
	Nodes(List),
	/// An interned string.
	Str(StrId),
	/// A slice of the source.
	Slice(u32, u32),
	/// Interned strings, a run of `Ast::host_strings`.
	Strs(u32, u32),
	Bool(bool),
	Int(u32),
	Null,
	/// Every comment read, as the answer lists them.
	Comments,
}

crate::layout::names! {
	/// What a host field's value is, as `Value` has it.
	#[derive(Clone, Copy, Debug, PartialEq)]
	pub enum HostTag {
		Node = "node",
		Nodes = "nodes",
		Str = "str",
		Slice = "slice",
		Strs = "strs",
		Bool = "bool",
		Int = "int",
		Null = "null",
		Comments = "comments",
	}
}

crate::layout::record! {
	/// A host field's value: two words, as many of them as the tag needs.
	#[derive(Clone, Copy, Debug)]
	pub struct HostValue {
		pub tag: HostTag,
		pub value: [u32; 2],
	}
}

crate::layout::record! {
	#[derive(Clone, Copy, Debug)]
	pub struct HostRow {
		/// The name of its type; none for an object of the host's without one.
		pub ty: Option<u32>,
		/// Its fields, in `Ast::host_keys` and `Ast::host_vals`.
		pub fields: Run,
		/// The number of its shape: its type, whether it has a span, and its fields' keys and tags.
		pub shape: u32,
		pub span: bool,
	}
}

crate::layout::record! {
	/// Where something starts and ends, in UTF-16 offsets.
	#[derive(Clone, Copy, Debug, Default)]
	pub struct Span {
		pub start: u32,
		pub end: u32,
	}
}

crate::layout::record! {
	#[derive(Clone, Copy, Debug, Default)]
	pub struct Position {
		pub line: u32,
		pub column: u32,
	}
}

crate::layout::record! {
	#[derive(Clone, Copy, Debug, Default)]
	pub struct Loc {
		pub start: Position,
		pub end: Position,
	}
}

crate::layout::record! {
	#[derive(Clone, Copy, Debug)]
	pub struct CommentRow {
		pub block: bool,
		/// Its text, without its delimiters.
		pub text: Span,
		pub span: Span,
		/// With `locations`.
		pub loc: Loc,
	}
}

crate::layout::record! {
	#[derive(Clone, Copy, Debug)]
	pub struct ErrorRow {
		/// Its number in `Code`.
		pub code: u32,
		pub first: Option<StrId>,
		pub second: Option<StrId>,
		pub pos: u32,
		pub end: u32,
		/// The line and column of `pos`.
		pub line: u32,
		pub column: u32,
	}
}

/// Every record a front end reads beside the nodes and the tables, by the name of its view.
pub const RECORDS: &[(&str, usize, &[Field])] = &[
	("comments", size_of::<CommentRow>(), CommentRow::FIELDS),
	("attached", size_of::<Attached>(), Attached::FIELDS),
	("errors", size_of::<ErrorRow>(), ErrorRow::FIELDS),
	("hosts", size_of::<HostRow>(), HostRow::FIELDS),
	("host_vals", size_of::<HostValue>(), HostValue::FIELDS),
	("spans", size_of::<Span>(), Span::FIELDS),
	("locs", size_of::<Loc>(), Loc::FIELDS),
	("marks", size_of::<Surrogate>(), Surrogate::FIELDS),
];

#[derive(Debug, Default)]
pub struct Ast {
	pub nodes: Handed<Node>,
	/// The values of the number literals, by `NumberLiteral::value`.
	pub numbers: Handed<f64>,
	pub lists: Handed<Option<NodeId>>,
	pub hosts: Vec<Host>,
	pub host_fields: Vec<(&'static str, Value)>,
	pub host_strings: Handed<StrId>,
	pub host_groups: Vec<HostGroup>,
	pub strings: Interner,
	pub comments: Vec<Comment>,
	pub comment_rows: Handed<CommentRow>,
	/// Comments attached to nodes by `comments::attach`, as indices into `comments`.
	pub attached: Slots<Attached>,
	/// Nodes erasure leaves out, when the answer erases TypeScript.
	pub erased: NodeSet,
	/// Nodes with what few have, for a front end that builds the rest in one piece: parentheses,
	/// comments, TypeScript's extras, a binding or reference on what is not an identifier.
	pub rare: NodeSet,
	/// Nodes that declare bindings, are assigned by references or are a root.
	pub late: NodeSet,
	/// Each node's span in UTF-16 offsets, when the source has characters past ASCII; empty
	/// otherwise, the node's own span being right.
	pub spans: Handed<Span>,
	/// Each node's lines and columns, with `locations`.
	pub locs: Handed<Loc>,
	/// Where each interned string starts in UTF-16 units, when the text has characters past ASCII.
	pub units: Handed<u32>,
	pub error_rows: Handed<ErrorRow>,
	pub host_rows: Handed<HostRow>,
	/// The name id of each host field, and its value.
	pub host_keys: Handed<u32>,
	pub host_vals: Handed<HostValue>,
	/// The buffers the last parse worked in, for the next one.
	pub spare: crate::parser::Spare,
	/// The scope analysis, when `scopes::analyze` ran.
	pub scopes: Option<crate::scopes::Scopes>,
	/// What went wrong, in source order, when errors are recovered from instead of thrown.
	pub errors: Vec<crate::SyntaxError>,
	/// Whether a node was written in parentheses, when `Options::parenthesized` asks.
	pub parenthesized: NodeSet,
	/// The keys TypeScript adds to a node, by the node; empty in a JavaScript tree.
	pub extras: Slots<Extras>,
}

crate::layout::record! {
	#[derive(Debug, Default, Clone, Copy)]
	pub struct Attached {
		pub leading: Run,
		pub trailing: Run,
		/// Inside an empty block, program, array or object.
		pub inner: Run,
	}
}

crate::layout::record! {
	/// `len` items from `start` on: a node takes each of its comments one after the other, a host
	/// node its fields.
	#[derive(Debug, Default, Clone, Copy)]
	pub struct Run {
		pub start: u32,
		pub len: u32,
	}
}

impl Run {
	pub fn is_empty(self) -> bool {
		self.len == 0
	}

	pub fn indices(self) -> std::ops::Range<u32> {
		self.start..self.start + self.len
	}

	pub(crate) fn push(&mut self, index: u32) {
		if self.len == 0 {
			self.start = index;
		}
		debug_assert_eq!(self.start + self.len, index);
		self.len += 1;
	}
}

/// Where a tree stood, to forget what was built after.
#[derive(Clone, Copy)]
pub(crate) struct Mark {
	nodes: usize,
	numbers: usize,
	lists: usize,
	extras: usize,
}

impl Ast {
	/// Empties the tree for the next parse; the room stays allocated.
	pub fn clear(&mut self) {
		self.hosts.clear();
		self.host_fields.clear();
		self.host_strings.clear();
		self.host_groups.clear();
		self.nodes.clear();
		self.numbers.clear();
		self.lists.clear();
		self.strings.clear();
		self.comments.clear();
		self.attached.clear();
		if let Some(scopes) = &mut self.scopes {
			scopes.clear(0);
		}
		self.errors.clear();
		self.parenthesized.clear();
		self.erased.clear();
		self.rare.clear();
		self.late.clear();
		self.comment_rows.clear();
		self.error_rows.clear();
		self.spans.clear();
		self.locs.clear();
		self.units.clear();
		self.host_rows.clear();
		self.host_keys.clear();
		self.host_vals.clear();
		self.extras.clear();
	}

	pub(crate) fn mark(&self) -> Mark {
		Mark {
			nodes: self.nodes.len(),
			numbers: self.numbers.len(),
			lists: self.lists.len(),
			extras: self.extras.len(),
		}
	}

	/// Forgets the nodes built since `mark`; their ids are reused, so nothing may still hold one.
	pub(crate) fn truncate(&mut self, mark: Mark) {
		self.nodes.truncate(mark.nodes);
		self.numbers.truncate(mark.numbers);
		self.lists.truncate(mark.lists);
		self.parenthesized.truncate(mark.nodes);
		self.extras.truncate(mark.extras);
	}

	/// Room for the tree of `bytes` of source: about a node per eight bytes, a list per thirty, a
	/// number per 128.
	pub(crate) fn sized(bytes: usize) -> Self {
		Ast {
			nodes: Handed::with_capacity(bytes / 8 + 16, 16),
			lists: Handed::with_capacity(bytes / 30 + 16, 16),
			numbers: Handed::with_capacity(bytes / 128 + 16, 16),
			strings: Interner::sized(bytes),
			..Ast::default()
		}
	}

	/// The tree's buffers a front end reads in place, by the names `layout::json` lists; a
	/// TypeScript tree's tables last.
	pub fn views(&mut self, typescript: bool, out: &mut Views<'_>) {
		out.push("nodes", &mut self.nodes);
		out.push("lists", &mut self.lists);
		out.push("numbers", &mut self.numbers);
		let (text, starts, marks) = self.strings.buffers();
		out.push("text", text);
		out.push("starts", starts);
		out.push("marks", marks);
		out.push("units", &mut self.units);
		out.push("spans", &mut self.spans);
		out.push("locs", &mut self.locs);
		out.push("parenthesized", self.parenthesized.words());
		out.push("erased", self.erased.words());
		out.push("rare", self.rare.words());
		out.push("late", self.late.words());
		out.push("comments", &mut self.comment_rows);
		self.attached.views("attached_slots", "attached", out);
		out.push("errors", &mut self.error_rows);
		out.push("hosts", &mut self.host_rows);
		out.push("host_keys", &mut self.host_keys);
		out.push("host_vals", &mut self.host_vals);
		out.push("host_strings", &mut self.host_strings);
		match &mut self.scopes {
			Some(scopes) => scopes.views(out),
			None => crate::scopes::Scopes::no_views(out),
		}
		if typescript {
			self.extras.views("extras_slots", "extras", out);
		}
	}
}

impl Value {
	pub fn row(self) -> HostValue {
		let (tag, value) = match self {
			Value::Node(id) => (HostTag::Node, [id.index(), 0]),
			Value::Nodes(list) => (HostTag::Nodes, [list.start, list.len]),
			Value::Str(id) => (HostTag::Str, [id.index(), 0]),
			Value::Slice(start, end) => (HostTag::Slice, [start, end]),
			Value::Strs(start, len) => (HostTag::Strs, [start, len]),
			Value::Bool(value) => (HostTag::Bool, [value as u32, 0]),
			Value::Int(value) => (HostTag::Int, [value, 0]),
			Value::Null => (HostTag::Null, [0, 0]),
			Value::Comments => (HostTag::Comments, [0, 0]),
		};
		HostValue { tag, value }
	}
}

impl Ast {
	/// Appends the children of `id` in source order.
	pub fn children(&self, id: NodeId, out: &mut Vec<NodeId>) {
		let from = out.len();
		let kind = &self.node(id).kind;
		match *kind {
			NodeKind::Host(index) => self.host_children(index, out),
			_ => {
				let base = kind as *const NodeKind as *const u8;
				self.held(base, node_layout::VARIANTS[crate::estree::tag(base)].fields, out);
			}
		}
		if let Some(extras) = self.extras.get(id) {
			self.held(extras as *const Extras as *const u8, Extras::FIELDS, out);
		}
		if !out[from..].is_sorted_by_key(|&child| self.node(child).start) {
			out[from..].sort_unstable_by_key(|&child| self.node(child).start);
		}
	}

	/// The nodes the fields of the record at `base` hold.
	fn held(&self, base: *const u8, fields: &[Field], out: &mut Vec<NodeId>) {
		for field in fields {
			let at = unsafe { base.add(field.at) };
			match field.ty {
				Ty::Node => out.push(unsafe { at.cast::<NodeId>().read_unaligned() }),
				Ty::OptNode => out.extend(unsafe { at.cast::<Option<NodeId>>().read_unaligned() }),
				Ty::List => out.extend(
					self.list(unsafe { at.cast::<List>().read_unaligned() })
						.iter()
						.flatten(),
				),
				Ty::OptList => {
					if let Some(list) = unsafe { at.cast::<Option<List>>().read_unaligned() } {
						out.extend(self.list(list).iter().flatten());
					}
				}
				Ty::Struct(inner) => self.held(at, inner, out),
				_ => {}
			}
		}
	}

	/// The nodes under a host node's fields; a child that is a fragment, a node without a span
	/// of its own, is walked through, so a document's nodes come in source order around its scripts.
	fn host_children(&self, index: u32, out: &mut Vec<NodeId>) {
		let host = self.hosts[index as usize];
		for &(_, value) in &self.host_fields[host.fields.0 as usize..(host.fields.0 + host.fields.1) as usize] {
			match value {
				Value::Node(child) => match self.node(child).kind {
					NodeKind::Host(inner) if !self.hosts[inner as usize].span => self.host_children(inner, out),
					_ => out.push(child),
				},
				Value::Nodes(children) => out.extend(self.list(children).iter().flatten()),
				_ => {}
			}
		}
	}

	pub fn node(&self, id: NodeId) -> &Node {
		&self.nodes[id.index() as usize]
	}

	pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
		&mut self.nodes[id.index() as usize]
	}

	pub fn str(&self, id: StrId) -> &str {
		self.strings.get(id)
	}

	pub fn list(&self, list: List) -> &[Option<NodeId>] {
		&self.lists[list.start as usize..(list.start + list.len) as usize]
	}

	/// The `i`th item of a list, for loops that mutate the tree between items.
	pub fn nth(&self, list: List, i: u32) -> Option<NodeId> {
		self.lists[(list.start + i) as usize]
	}

	pub fn add(&mut self, kind: NodeKind, start: u32, end: u32) -> NodeId {
		self.nodes.push(Node { kind, start, end });
		NodeId::at(self.nodes.len() as u32 - 1)
	}

	/// The last node added, which is the root after a whole-program parse.
	pub fn last(&self) -> NodeId {
		NodeId::at(self.nodes.len() as u32 - 1)
	}

	pub fn set_parenthesized(&mut self, id: NodeId) {
		self.parenthesized.insert(id);
	}

	pub fn is_parenthesized(&self, id: NodeId) -> bool {
		self.parenthesized.contains(id)
	}

	/// A number literal's value, kept beside the tree so a node stays four-byte aligned.
	pub fn number(&mut self, value: f64) -> u32 {
		self.numbers.push(value);
		self.numbers.len() as u32 - 1
	}

	pub fn add_list_from(&mut self, items: impl ExactSizeIterator<Item = Option<NodeId>>) -> List {
		let start = self.lists.len() as u32;
		let len = items.len() as u32;
		self.lists.extend(items);
		List { start, len }
	}

	pub fn add_list(&mut self, items: &[Option<NodeId>]) -> List {
		let start = self.lists.len() as u32;
		self.lists.extend_from_slice(items);
		List {
			start,
			len: items.len() as u32,
		}
	}
}

crate::layout::kinds! {
	#[derive(Clone, Copy, Debug, PartialEq)]
	pub enum NodeKind in node_layout {
		Program {
			body: List,
			module: bool,
		},

		Identifier {
			name: StrId,
		},
		PrivateIdentifier {
			name: StrId,
		},
		NumberLiteral {
			/// Into `Ast::numbers`.
			value: u32,
		},
		BigIntLiteral,
		StringLiteral {
			value: StrId,
		},
		BooleanLiteral {
			value: bool,
		},
		NullLiteral,
		RegExpLiteral {
			pattern: StrId,
			flags: StrId,
		},
		TemplateLiteral {
			quasis: List,
			expressions: List,
		},
		TemplateElement {
			cooked: Option<StrId>,
			raw: StrId,
			tail: bool,
		},
		TaggedTemplateExpression {
			tag: NodeId,
			quasi: NodeId,
		},
		ThisExpression,
		Super,
		ArrayExpression {
			elements: List,
		},
		ObjectExpression {
			properties: List,
		},
		Property {
			key: NodeId,
			value: NodeId,
			kind: PropertyKind,
			computed: bool,
			method: bool,
			shorthand: bool,
		},
		SpreadElement {
			argument: NodeId,
		},
		UnaryExpression {
			operator: UnaryOperator,
			argument: NodeId,
		},
		UpdateExpression {
			operator: UpdateOperator,
			prefix: bool,
			argument: NodeId,
		},
		BinaryExpression {
			operator: BinaryOperator,
			left: NodeId,
			right: NodeId,
		},
		LogicalExpression {
			operator: LogicalOperator,
			left: NodeId,
			right: NodeId,
		},
		AssignmentExpression {
			operator: AssignmentOperator,
			left: NodeId,
			right: NodeId,
		},
		ConditionalExpression {
			test: NodeId,
			consequent: NodeId,
			alternate: NodeId,
		},
		MemberExpression {
			object: NodeId,
			property: NodeId,
			computed: bool,
			optional: bool,
		},
		CallExpression {
			callee: NodeId,
			arguments: List,
			optional: bool,
		},
		ChainExpression {
			expression: NodeId,
		},
		NewExpression {
			callee: NodeId,
			arguments: List,
		},
		SequenceExpression {
			expressions: List,
		},
		ArrowFunctionExpression {
			params: List,
			body: NodeId,
			expression: bool,
			is_async: bool,
		},
		FunctionExpression {
			function: Function,
		},
		FunctionDeclaration {
			function: Function,
		},
		ClassExpression {
			class: Class,
		},
		ClassDeclaration {
			class: Class,
		},
		ClassBody {
			body: List,
		},
		MethodDefinition {
			key: NodeId,
			value: NodeId,
			kind: MethodKind,
			computed: bool,
			is_static: bool,
		},
		PropertyDefinition {
			key: NodeId,
			value: Option<NodeId>,
			computed: bool,
			is_static: bool,
			kind: FieldKind,
		},
		StaticBlock {
			body: List,
		},
		YieldExpression {
			argument: Option<NodeId>,
			delegate: bool,
		},
		AwaitExpression {
			argument: NodeId,
		},
		MetaProperty {
			meta: NodeId,
			property: NodeId,
		},
		ImportExpression {
			source: NodeId,
			options: Option<NodeId>,
		},

		ObjectPattern {
			properties: List,
		},
		ArrayPattern {
			elements: List,
		},
		RestElement {
			argument: NodeId,
		},
		AssignmentPattern {
			left: NodeId,
			right: NodeId,
		},

		ExpressionStatement {
			expression: NodeId,
			directive: Option<StrId>,
		},
		BlockStatement {
			body: List,
		},
		EmptyStatement,
		DebuggerStatement,
		WithStatement {
			object: NodeId,
			body: NodeId,
		},
		ReturnStatement {
			argument: Option<NodeId>,
		},
		LabeledStatement {
			label: NodeId,
			body: NodeId,
		},
		BreakStatement {
			label: Option<NodeId>,
		},
		ContinueStatement {
			label: Option<NodeId>,
		},
		IfStatement {
			test: NodeId,
			consequent: NodeId,
			alternate: Option<NodeId>,
		},
		SwitchStatement {
			discriminant: NodeId,
			cases: List,
		},
		SwitchCase {
			test: Option<NodeId>,
			consequent: List,
		},
		ThrowStatement {
			argument: NodeId,
		},
		TryStatement {
			block: NodeId,
			handler: Option<NodeId>,
			finalizer: Option<NodeId>,
		},
		CatchClause {
			param: Option<NodeId>,
			body: NodeId,
		},
		WhileStatement {
			test: NodeId,
			body: NodeId,
		},
		DoWhileStatement {
			body: NodeId,
			test: NodeId,
		},
		ForStatement {
			init: Option<NodeId>,
			test: Option<NodeId>,
			update: Option<NodeId>,
			body: NodeId,
		},
		ForInStatement {
			left: NodeId,
			right: NodeId,
			body: NodeId,
		},
		ForOfStatement {
			left: NodeId,
			right: NodeId,
			body: NodeId,
			is_await: bool,
		},
		VariableDeclaration {
			declarations: List,
			kind: VariableKind,
		},
		VariableDeclarator {
			id: NodeId,
			init: Option<NodeId>,
		},

		ImportDeclaration {
			specifiers: List,
			source: NodeId,
			attributes: List,
		},
		ImportSpecifier {
			imported: NodeId,
			local: NodeId,
		},
		ImportDefaultSpecifier {
			local: NodeId,
		},
		ImportNamespaceSpecifier {
			local: NodeId,
		},
		ImportAttribute {
			key: NodeId,
			value: NodeId,
		},
		/// `export declaration`, an ExportNamedDeclaration whose only child is what it declares.
		ExportDeclaration {
			declaration: NodeId,
		},
		/// `export { specifiers } from source with { attributes }`.
		ExportNamedDeclaration {
			specifiers: List,
			source: Option<NodeId>,
			attributes: List,
		},
		ExportSpecifier {
			local: NodeId,
			exported: NodeId,
		},
		ExportDefaultDeclaration {
			declaration: NodeId,
		},
		ExportAllDeclaration {
			exported: Option<NodeId>,
			source: NodeId,
			attributes: List,
		},
		/// A node of the host's grammar, indexed into `Ast::hosts`.
		Host(u32),

		// Types
		TypeAnnotation {
			type_annotation: NodeId,
		},
		/// `any`, `string`, ... and `void`, `null`, `intrinsic`; the name is the ESTree type.
		Keyword(Keyword),
		ThisType,
		TypePredicate {
			parameter_name: NodeId,
			type_annotation: Option<NodeId>,
			asserts: bool,
		},
		TypeReference {
			type_name: NodeId,
			type_arguments: Option<NodeId>,
		},
		QualifiedName {
			left: NodeId,
			right: NodeId,
		},
		TypeParameterInstantiation {
			params: List,
		},
		TypeParameterDeclaration {
			params: List,
		},
		TypeParameter {
			name: StrId,
			constraint: Option<NodeId>,
			default: Option<NodeId>,
			is_in: bool,
			is_out: bool,
			is_const: bool,
		},
		FunctionType {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: NodeId,
		},
		ConstructorType {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: NodeId,
			is_abstract: bool,
		},
		UnionType {
			types: List,
		},
		IntersectionType {
			types: List,
		},
		TypeOperator {
			operator: StrId,
			type_annotation: NodeId,
		},
		InferType {
			type_parameter: NodeId,
		},
		LiteralType {
			literal: NodeId,
		},
		ImportType {
			argument: NodeId,
			qualifier: Option<NodeId>,
			type_arguments: Option<NodeId>,
		},
		TypeQuery {
			expr_name: NodeId,
			type_arguments: Option<NodeId>,
		},
		MappedType {
			readonly: Option<Modifier>,
			type_parameter: NodeId,
			name_type: Option<NodeId>,
			optional: Option<Modifier>,
			type_annotation: Option<NodeId>,
		},
		TypeLiteral {
			members: List,
		},
		NamedTupleMember {
			label: NodeId,
			optional: bool,
			element_type: NodeId,
		},
		OptionalType {
			type_annotation: NodeId,
		},
		RestType {
			type_annotation: NodeId,
		},
		TupleType {
			element_types: List,
		},
		ParenthesizedType {
			type_annotation: NodeId,
		},
		ArrayType {
			element_type: NodeId,
		},
		IndexedAccessType {
			object_type: NodeId,
			index_type: NodeId,
		},
		ConditionalType {
			check_type: NodeId,
			extends_type: NodeId,
			true_type: NodeId,
			false_type: NodeId,
		},

		// Type members
		IndexSignature {
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		CallSignatureDeclaration {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		ConstructSignatureDeclaration {
			type_parameters: Option<NodeId>,
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		/// Its type parameters are in the extras record: with them, the node would outgrow
		/// JavaScript's widest kind and widen every node.
		MethodSignature {
			key: NodeId,
			computed: bool,
			optional: bool,
			kind: SignatureKind,
			parameters: List,
			type_annotation: Option<NodeId>,
		},
		PropertySignature {
			key: NodeId,
			computed: bool,
			optional: bool,
			readonly: bool,
			type_annotation: Option<NodeId>,
		},

		// Declarations
		InterfaceDeclaration {
			id: NodeId,
			type_parameters: Option<NodeId>,
			extends: List,
			body: NodeId,
		},
		InterfaceBody {
			body: List,
		},
		Heritage {
			expression: NodeId,
			type_arguments: Option<NodeId>,
			clause: Clause,
		},
		EnumDeclaration {
			id: NodeId,
			members: List,
			is_const: bool,
		},
		EnumMember {
			id: NodeId,
			initializer: Option<NodeId>,
		},
		ModuleDeclaration {
			id: NodeId,
			body: Option<NodeId>,
			global: bool,
		},
		ModuleBlock {
			body: List,
		},
		TypeAliasDeclaration {
			id: NodeId,
			type_parameters: Option<NodeId>,
			type_annotation: NodeId,
		},
		ImportEqualsDeclaration {
			id: NodeId,
			module_reference: NodeId,
			is_export: bool,
			import_kind: Kind,
		},
		ExternalModuleReference {
			expression: NodeId,
		},
		ExportAssignment {
			expression: NodeId,
		},
		NamespaceExportDeclaration {
			id: NodeId,
		},
		/// A function declaration without a body: an overload or an ambient declaration.
		DeclareFunction {
			id: Option<NodeId>,
			params: List,
			is_async: bool,
			generator: bool,
		},
		/// A method without a body, the `value` of its `MethodDefinition`.
		DeclareMethod {
			params: List,
			is_async: bool,
			generator: bool,
		},

		// Expressions
		AsExpression {
			expression: NodeId,
			type_annotation: NodeId,
		},
		SatisfiesExpression {
			expression: NodeId,
			type_annotation: NodeId,
		},
		NonNullExpression {
			expression: NodeId,
		},
		TypeAssertion {
			type_annotation: NodeId,
			expression: NodeId,
		},
		/// An item of a list, or its rest, with `?`, `: type` or both, before the list is known to be parameters.
		TypeCastExpression {
			expression: NodeId,
			type_annotation: Option<NodeId>,
			optional: bool,
		},
		InstantiationExpression {
			expression: NodeId,
			type_arguments: NodeId,
		},
		ParameterProperty {
			parameter: NodeId,
		},
		Decorator {
			expression: NodeId,
		},
	}
}

crate::layout::record! {
	#[derive(Clone, Copy, Debug, PartialEq)]
	pub struct Function {
		pub id: Option<NodeId>,
		pub params: List,
		pub body: NodeId,
		pub is_async: bool,
		pub generator: bool,
	}
}

crate::layout::record! {
	#[derive(Clone, Copy, Debug, PartialEq)]
	pub struct Class {
		pub id: Option<NodeId>,
		pub super_class: Option<NodeId>,
		pub body: NodeId,
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum PropertyKind {
		Init = "init",
		Get = "get",
		Set = "set",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum MethodKind {
		Constructor = "constructor",
		Method = "method",
		Get = "get",
		Set = "set",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum FieldKind {
		Property = "PropertyDefinition",
		Accessor = "AccessorProperty",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum VariableKind {
		Var = "var",
		Let = "let",
		Const = "const",
		Using = "using",
		AwaitUsing = "await using",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum UnaryOperator {
		Minus = "-",
		Plus = "+",
		Not = "!",
		BitNot = "~",
		Typeof = "typeof",
		Void = "void",
		Delete = "delete",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum UpdateOperator {
		Increment = "++",
		Decrement = "--",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum BinaryOperator {
		Eq = "==",
		NotEq = "!=",
		StrictEq = "===",
		StrictNotEq = "!==",
		Lt = "<",
		LtEq = "<=",
		Gt = ">",
		GtEq = ">=",
		Shl = "<<",
		Shr = ">>",
		UShr = ">>>",
		Add = "+",
		Sub = "-",
		Mul = "*",
		Div = "/",
		Mod = "%",
		Exp = "**",
		BitOr = "|",
		BitXor = "^",
		BitAnd = "&",
		In = "in",
		Instanceof = "instanceof",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum LogicalOperator {
		Or = "||",
		And = "&&",
		Nullish = "??",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum AssignmentOperator {
		Assign = "=",
		Add = "+=",
		Sub = "-=",
		Mul = "*=",
		Div = "/=",
		Mod = "%=",
		Exp = "**=",
		Shl = "<<=",
		Shr = ">>=",
		UShr = ">>>=",
		BitOr = "|=",
		BitXor = "^=",
		BitAnd = "&=",
		Or = "||=",
		And = "&&=",
		Nullish = "??=",
	}
}

impl VariableKind {
	pub(crate) fn is_using(self) -> bool {
		matches!(self, Self::Using | Self::AwaitUsing)
	}
}

impl Ast {
	pub(crate) fn extras_of(&self, id: NodeId) -> Extras {
		self.extras.get(id).copied().unwrap_or_default()
	}

	/// The nodes erasing still adds keys to: those with decorators.
	pub(crate) fn decorated(&self, set: &mut NodeSet) {
		for &owner in self.extras.owners() {
			if self.extras.get(owner).is_some_and(|extras| extras.decorators.is_some()) {
				set.insert(owner);
			}
		}
	}

	/// Whether every statement of a list erases to nothing.
	fn all_erased(&self, list: List) -> bool {
		self.list(list).iter().all(|item| item.is_none_or(|id| self.erased(id)))
	}

	pub(crate) fn erased(&self, id: NodeId) -> bool {
		use NodeKind::*;
		let extras = self.extras_of(id);
		if extras.declare {
			return true;
		}
		match self.node(id).kind {
			MethodDefinition { .. } | PropertyDefinition { .. } if extras.is_abstract => true,
			// an overload signature: a method without a body
			MethodDefinition { value, .. } => {
				matches!(self.node(value).kind, DeclareMethod { .. })
			}
			InterfaceDeclaration { .. }
			| TypeAliasDeclaration { .. }
			| DeclareFunction { .. }
			| IndexSignature { .. }
			| NamespaceExportDeclaration { .. } => true,
			ImportEqualsDeclaration { import_kind, .. } => import_kind == Kind::Type,
			ModuleDeclaration { body: None, .. } => true,
			ModuleDeclaration { body: Some(block), .. } => match self.node(block).kind {
				ModuleBlock { body } => self.all_erased(body),
				_ => false,
			},
			ImportDeclaration { specifiers, .. } => {
				extras.import_kind == Some(Kind::Type) || (specifiers.len > 0 && self.all_erased(specifiers))
			}
			ImportSpecifier { .. } => extras.import_kind == Some(Kind::Type),
			ExportSpecifier { .. } | ExportAllDeclaration { .. } => extras.export_kind == Some(Kind::Type),
			// `export { type A }` keeps an `export {}`, as tsc keeps the file a module
			ExportDeclaration { declaration } => extras.export_kind == Some(Kind::Type) || self.erased(declaration),
			ExportNamedDeclaration { .. } => extras.export_kind == Some(Kind::Type),
			ExportDefaultDeclaration { declaration } => {
				extras.export_kind == Some(Kind::Type) || self.erased(declaration)
			}
			_ => false,
		}
	}
}

crate::layout::names! {
	/// Whether an import or export carries values or only types.
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Kind {
		Value = "value",
		Type = "type",
	}
}

crate::layout::names! {
	/// An interface's `extends` or a class's `implements`.
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Clause {
		Extends = "TSInterfaceHeritage",
		Implements = "TSClassImplements",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Accessibility {
		Public = "public",
		Private = "private",
		Protected = "protected",
	}
}

crate::layout::names! {
	/// A `readonly` or `?` modifier of a mapped type: bare, or with a `+`/`-` prefix.
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Modifier {
		Plus = "+",
		Minus = "-",
		True = "true",
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum SignatureKind {
		Method = "method",
		Get = "get",
		Set = "set",
	}
}

crate::layout::record! {
	/// Keys TypeScript adds to a JavaScript node, or modifiers on one of its own.
	#[derive(Clone, Copy, Debug, Default, PartialEq)]
	pub struct Extras {
		pub type_annotation: Option<NodeId>,
		pub return_type: Option<NodeId>,
		pub type_parameters: Option<NodeId>,
		pub type_arguments: Option<NodeId>,
		pub super_type_arguments: Option<NodeId>,
		pub implements: Option<List>,
		pub decorators: Option<List>,
		pub accessibility: Option<Accessibility>,
		pub import_kind: Option<Kind>,
		pub export_kind: Option<Kind>,
		pub optional: bool,
		pub definite: bool,
		pub declare: bool,
		pub is_abstract: bool,
		pub readonly: bool,
		pub is_override: bool,
		pub is_static: bool,
	}
}

impl Extras {
	pub(crate) fn set(&self) -> impl Iterator<Item = &'static str> {
		let base = self as *const Extras as *const u8;
		Extras::FIELDS
			.iter()
			.filter(move |field| unsafe {
				let at = base.add(field.at);
				match field.ty {
					Ty::Bool => at.cast::<bool>().read(),
					Ty::OptNode => at.cast::<Option<NodeId>>().read().is_some(),
					Ty::OptList => at.cast::<Option<List>>().read().is_some(),
					Ty::OptEnum(names) => usize::from(at.read()) < names.len(),
					_ => unreachable!(),
				}
			})
			.map(|field| field.name)
	}
}

crate::layout::names! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Keyword {
		Any = "TSAnyKeyword",
		Boolean = "TSBooleanKeyword",
		BigInt = "TSBigIntKeyword",
		Never = "TSNeverKeyword",
		Number = "TSNumberKeyword",
		Object = "TSObjectKeyword",
		String = "TSStringKeyword",
		Symbol = "TSSymbolKeyword",
		Undefined = "TSUndefinedKeyword",
		Unknown = "TSUnknownKeyword",
		Void = "TSVoidKeyword",
		Null = "TSNullKeyword",
		Intrinsic = "TSIntrinsicKeyword",
	}
}

impl Keyword {
	pub(crate) fn from_name(name: &str) -> Option<Keyword> {
		Some(match name {
			"any" => Keyword::Any,
			"boolean" => Keyword::Boolean,
			"bigint" => Keyword::BigInt,
			"never" => Keyword::Never,
			"number" => Keyword::Number,
			"object" => Keyword::Object,
			"string" => Keyword::String,
			"symbol" => Keyword::Symbol,
			"undefined" => Keyword::Undefined,
			"unknown" => Keyword::Unknown,
			_ => return None,
		})
	}
}

impl Ast {
	/// Whether an import or export only names types, so it binds and refers to no value.
	pub(crate) fn types_only(&self, id: NodeId) -> bool {
		self.extras
			.get(id)
			.is_some_and(|e| e.import_kind == Some(Kind::Type) || e.export_kind == Some(Kind::Type))
	}

	/// The expression a TypeScript node wraps without changing its value: `x as T`, `x!`.
	pub(crate) fn wrapped(&self, id: NodeId) -> Option<NodeId> {
		use NodeKind::*;
		match self.node(id).kind {
			AsExpression { expression, .. }
			| SatisfiesExpression { expression, .. }
			| NonNullExpression { expression }
			| TypeAssertion { expression, .. }
			| TypeCastExpression { expression, .. }
			| InstantiationExpression { expression, .. } => Some(expression),
			_ => None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_set_truncated_past_its_last_word_keeps_the_word() {
		let mut set = NodeSet::default();
		set.insert(NodeId::at(63));
		set.truncate(65);
		assert!(set.contains(NodeId::at(63)));
		set.truncate(63);
		assert!(!set.contains(NodeId::at(63)));
	}
}
