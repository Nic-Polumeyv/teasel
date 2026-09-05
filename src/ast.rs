use crate::interner::{Interner, StrId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comment {
	pub kind: CommentKind,
	pub start: u32,
	pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentKind {
	Line,
	Block,
	Hashbang,
	HtmlOpen,
	HtmlClose,
}

impl Comment {
	pub fn is_block(&self) -> bool {
		self.kind == CommentKind::Block
	}

	/// Byte range of the comment text, without its delimiters.
	pub fn text_range(&self) -> std::ops::Range<usize> {
		let (prefix, suffix) = match self.kind {
			CommentKind::Line | CommentKind::Hashbang => (2, 0),
			CommentKind::Block => (2, 2),
			CommentKind::HtmlOpen => (4, 0),
			CommentKind::HtmlClose => (3, 0),
		};
		self.start as usize + prefix..self.end as usize - suffix
	}
}

/// Index of a node in `Ast::nodes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// A contiguous run of node ids in `Ast::lists`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct List {
	pub start: u32,
	pub len: u32,
}

impl List {
	pub const EMPTY: List = List { start: 0, len: 0 };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
	pub kind: NodeKind,
	pub start: u32,
	pub end: u32,
}

#[derive(Debug, Default)]
pub struct Ast {
	pub nodes: Vec<Node>,
	pub lists: Vec<Option<NodeId>>,
	pub strings: Interner,
	pub comments: Vec<Comment>,
}

impl Ast {
	pub fn node(&self, id: NodeId) -> &Node {
		&self.nodes[id.0 as usize]
	}

	pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
		&mut self.nodes[id.0 as usize]
	}

	pub fn str(&self, id: StrId) -> &str {
		self.strings.get(id)
	}

	pub fn list(&self, list: List) -> &[Option<NodeId>] {
		&self.lists[list.start as usize..(list.start + list.len) as usize]
	}

	pub fn add(&mut self, kind: NodeKind, start: u32, end: u32) -> NodeId {
		self.nodes.push(Node { kind, start, end });
		NodeId(self.nodes.len() as u32 - 1)
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeKind {
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
		value: f64,
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
	ParenthesizedExpression {
		expression: NodeId,
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
	ExportNamedDeclaration {
		declaration: Option<NodeId>,
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Function {
	pub id: Option<NodeId>,
	pub params: List,
	pub body: NodeId,
	pub is_async: bool,
	pub generator: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Class {
	pub id: Option<NodeId>,
	pub super_class: Option<NodeId>,
	pub body: NodeId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
	Init,
	Get,
	Set,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
	Constructor,
	Method,
	Get,
	Set,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableKind {
	Var,
	Let,
	Const,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
	Minus,
	Plus,
	Not,
	BitNot,
	Typeof,
	Void,
	Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOperator {
	Increment,
	Decrement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
	Eq,
	NotEq,
	StrictEq,
	StrictNotEq,
	Lt,
	LtEq,
	Gt,
	GtEq,
	Shl,
	Shr,
	UShr,
	Add,
	Sub,
	Mul,
	Div,
	Mod,
	Exp,
	BitOr,
	BitXor,
	BitAnd,
	In,
	Instanceof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalOperator {
	Or,
	And,
	Nullish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentOperator {
	Assign,
	Add,
	Sub,
	Mul,
	Div,
	Mod,
	Exp,
	Shl,
	Shr,
	UShr,
	BitOr,
	BitXor,
	BitAnd,
	Or,
	And,
	Nullish,
}

impl UnaryOperator {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Minus => "-",
			Self::Plus => "+",
			Self::Not => "!",
			Self::BitNot => "~",
			Self::Typeof => "typeof",
			Self::Void => "void",
			Self::Delete => "delete",
		}
	}
}

impl UpdateOperator {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Increment => "++",
			Self::Decrement => "--",
		}
	}
}

impl BinaryOperator {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Eq => "==",
			Self::NotEq => "!=",
			Self::StrictEq => "===",
			Self::StrictNotEq => "!==",
			Self::Lt => "<",
			Self::LtEq => "<=",
			Self::Gt => ">",
			Self::GtEq => ">=",
			Self::Shl => "<<",
			Self::Shr => ">>",
			Self::UShr => ">>>",
			Self::Add => "+",
			Self::Sub => "-",
			Self::Mul => "*",
			Self::Div => "/",
			Self::Mod => "%",
			Self::Exp => "**",
			Self::BitOr => "|",
			Self::BitXor => "^",
			Self::BitAnd => "&",
			Self::In => "in",
			Self::Instanceof => "instanceof",
		}
	}
}

impl LogicalOperator {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Or => "||",
			Self::And => "&&",
			Self::Nullish => "??",
		}
	}
}

impl AssignmentOperator {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Assign => "=",
			Self::Add => "+=",
			Self::Sub => "-=",
			Self::Mul => "*=",
			Self::Div => "/=",
			Self::Mod => "%=",
			Self::Exp => "**=",
			Self::Shl => "<<=",
			Self::Shr => ">>=",
			Self::UShr => ">>>=",
			Self::BitOr => "|=",
			Self::BitXor => "^=",
			Self::BitAnd => "&=",
			Self::Or => "||=",
			Self::And => "&&=",
			Self::Nullish => "??=",
		}
	}
}

impl VariableKind {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Var => "var",
			Self::Let => "let",
			Self::Const => "const",
		}
	}
}
