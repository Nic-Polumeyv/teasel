//! Scope analysis over a parsed tree: the scopes, the bindings each declares, and every
//! identifier resolved to the binding it names. Names are declared first, in the environment each
//! belongs to, and every reference is resolved after, so hoisting and merging need nothing special.

use crate::ast::{Ast, List, NodeId, NodeKind, VariableKind, Walk};
use crate::error::{Code, SyntaxError};
use crate::interner::{FastMap, StrId};
use crate::parser::Entry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
	Module,
	Script,
	/// A function, its parameters and its body.
	Function,
	/// The name of a function expression, visible only inside it.
	FunctionName,
	/// A class body, where the class name is bound again, immutably.
	Class,
	Block,
	Catch,
	/// The head of a loop declaring with `let`, `const` or `using`, and its body.
	For,
	Switch,
	StaticBlock,
	With,
	/// A TypeScript namespace body.
	Namespace,
	/// A TypeScript enum body, where the members are names.
	Enum,
	/// What a host parses at an offset: an expression, a statement, a pattern.
	Fragment,
}

impl ScopeKind {
	pub fn name(self) -> &'static str {
		match self {
			ScopeKind::Module => "module",
			ScopeKind::Script => "script",
			ScopeKind::Function => "function",
			ScopeKind::FunctionName => "function-name",
			ScopeKind::Class => "class",
			ScopeKind::Block => "block",
			ScopeKind::Catch => "catch",
			ScopeKind::For => "for",
			ScopeKind::Switch => "switch",
			ScopeKind::StaticBlock => "static-block",
			ScopeKind::With => "with",
			ScopeKind::Namespace => "namespace",
			ScopeKind::Enum => "enum",
			ScopeKind::Fragment => "fragment",
		}
	}

	/// Whether `var` declarations stop here.
	fn holds_var(self) -> bool {
		matches!(
			self,
			ScopeKind::Module
				| ScopeKind::Script
				| ScopeKind::Function
				| ScopeKind::StaticBlock
				| ScopeKind::Namespace
				| ScopeKind::Fragment
		)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
	Var,
	Let,
	Const,
	Using,
	AwaitUsing,
	Function,
	Class,
	Param,
	CatchParam,
	Import,
	/// The name of a function expression, seen from inside it.
	FunctionName,
	/// The name of a class expression, seen from inside its body.
	ClassName,
	/// `arguments` in a function that reads it.
	Arguments,
	Enum,
	EnumMember,
	Namespace,
	/// What a pattern parsed on its own declares; the host says what kind of binding it is.
	Pattern,
}

impl BindingKind {
	pub fn name(self) -> &'static str {
		match self {
			BindingKind::Var => "var",
			BindingKind::Let => "let",
			BindingKind::Const => "const",
			BindingKind::Using => "using",
			BindingKind::AwaitUsing => "await using",
			BindingKind::Function => "function",
			BindingKind::Class => "class",
			BindingKind::Param => "param",
			BindingKind::CatchParam => "catch",
			BindingKind::Import => "import",
			BindingKind::FunctionName => "function-name",
			BindingKind::ClassName => "class-name",
			BindingKind::Arguments => "arguments",
			BindingKind::Enum => "enum",
			BindingKind::EnumMember => "enum-member",
			BindingKind::Namespace => "namespace",
			BindingKind::Pattern => "pattern",
		}
	}

	/// Only `var` hoists; a function declaration belongs to the block it is in.
	fn is_var(self) -> bool {
		self == BindingKind::Var
	}
}

pub type ScopeId = u32;
pub type BindingId = u32;
pub type ReferenceId = u32;

#[derive(Debug)]
pub struct Scope {
	pub kind: ScopeKind,
	/// The node that opens the scope: the program, function, class, block, clause or statement;
	/// none for the scope around a parameter list parsed on its own.
	pub node: Option<NodeId>,
	pub parent: Option<ScopeId>,
	/// How many function scopes enclose this one, itself included when it is one.
	pub function_depth: u32,
	/// An `await` or `for await` runs directly in this scope, no function around it; only a
	/// program or fragment scope can say so.
	pub top_level_await: bool,
}

#[derive(Debug)]
pub struct Binding {
	pub name: StrId,
	pub kind: BindingKind,
	pub scope: ScopeId,
	/// The identifier that declares it; `arguments` has none.
	pub node: Option<NodeId>,
	/// What declares it: the declarator, function, class, import specifier, catch clause or
	/// enum, as eslint-scope's definition node; none for `arguments` and for a pattern or parameter
	/// list parsed on its own.
	pub declaration: Option<NodeId>,
}

#[derive(Debug)]
pub struct Reference {
	pub node: NodeId,
	pub scope: ScopeId,
	/// None when no scope declares the name: a global.
	pub binding: Option<BindingId>,
	/// The identifier is assigned to, updated or bound by a destructuring assignment.
	pub write: bool,
	/// A member of the identifier's value is assigned to, updated or deleted.
	pub mutate: bool,
	/// The identifier's value is read: every reference but a plain assignment's target or a
	/// destructuring one's; a compound assignment or an update reads and writes.
	pub read: bool,
	/// What a write assigns: the right side of the assignment or the iterated expression of a
	/// `for-in` or `for-of`, as eslint-scope's `writeExpr`; none for an update.
	pub write_expr: Option<NodeId>,
}

/// What an identifier is in the analysis.
#[derive(Clone, Copy, Debug)]
pub enum Role {
	Declares(BindingId),
	Reference(ReferenceId),
}

/// A table by node, dense: nodes are numbered, and a hash of the number cost more than the room.
/// Zero is empty so the room comes zeroed from the allocator.
#[derive(Debug)]
pub struct NodeTable<T>(Vec<u32>, std::marker::PhantomData<T>);

impl<T> Default for NodeTable<T> {
	fn default() -> Self {
		NodeTable(Vec::new(), std::marker::PhantomData)
	}
}

pub trait Packed: Copy {
	fn pack(self) -> u32;
	fn unpack(word: u32) -> Self;
}

impl Packed for ScopeId {
	fn pack(self) -> u32 {
		self
	}
	fn unpack(word: u32) -> Self {
		word
	}
}

// ids stay below 2^31: a node count bounds them, and the source is u32 long
impl Packed for Role {
	fn pack(self) -> u32 {
		match self {
			Role::Declares(binding) => binding << 1,
			Role::Reference(reference) => reference << 1 | 1,
		}
	}
	fn unpack(word: u32) -> Self {
		if word & 1 == 0 {
			Role::Declares(word >> 1)
		} else {
			Role::Reference(word >> 1)
		}
	}
}

impl<T: Packed> NodeTable<T> {
	fn sized(nodes: usize) -> Self {
		NodeTable(vec![0; nodes], std::marker::PhantomData)
	}

	fn reset(&mut self, nodes: usize) {
		self.0.clear();
		self.0.resize(nodes, 0);
	}

	pub fn get(&self, id: NodeId) -> Option<T> {
		match self.0.get(id.0 as usize) {
			Some(&word) if word != 0 => Some(T::unpack(word - 1)),
			_ => None,
		}
	}

	fn insert(&mut self, id: NodeId, value: T) {
		debug_assert!((id.0 as usize) < self.0.len(), "nodes are numbered before analysis");
		self.0[id.0 as usize] = value.pack() + 1;
	}

	fn insert_new(&mut self, id: NodeId, value: T) {
		let slot = &mut self.0[id.0 as usize];
		if *slot == 0 {
			*slot = value.pack() + 1;
		}
	}

	pub fn iter(&self) -> impl Iterator<Item = (NodeId, T)> + '_ {
		self.0
			.iter()
			.enumerate()
			.filter(|(_, w)| **w != 0)
			.map(|(i, w)| (NodeId(i as u32), T::unpack(w - 1)))
	}
}

/// A piece of JavaScript a host read on its own, and what the tables hold for it: the scope it
/// sits in, and the scopes opened, the bindings declared and the references made inside it, each
/// a range of its table since a piece is visited in one go.
#[derive(Debug)]
pub struct Root {
	pub node: NodeId,
	pub scope: ScopeId,
	pub scopes: (u32, u32),
	pub bindings: (u32, u32),
	pub references: (u32, u32),
}

#[derive(Debug, Default)]
pub struct Scopes {
	pub scopes: Vec<Scope>,
	pub bindings: Vec<Binding>,
	pub references: Vec<Reference>,
	/// The pieces of JavaScript in a host's document, in source order; empty for a plain parse.
	pub roots: Vec<Root>,
	pub root_of: FastMap<NodeId, u32>,
	pub of_node: NodeTable<ScopeId>,
	pub of_identifier: NodeTable<Role>,
	/// The bindings each declaring node declares, and the write references each expression is
	/// assigned by: `Binding::declaration` and `Reference::write_expr` from the node's side.
	pub declared_by: ByNode,
	pub writes_of: ByNode,
	/// A name declared twice in one of the host's scopes, which are lexical like a block's.
	pub errors: Vec<SyntaxError>,
	scratch: Scratch,
}

/// Ids grouped by node: one sorted list, and where each node's run starts in it.
#[derive(Debug, Default)]
pub struct ByNode {
	pairs: Vec<(NodeId, u32)>,
	ids: Vec<u32>,
	starts: NodeTable<u32>,
}

impl ByNode {
	fn finish(&mut self, nodes: usize) {
		self.pairs.sort_unstable();
		self.ids.clear();
		self.ids.extend(self.pairs.iter().map(|&(_, id)| id));
		self.starts.reset(nodes);
		for (i, &(node, _)) in self.pairs.iter().enumerate() {
			if i == 0 || self.pairs[i - 1].0 != node {
				self.starts.insert(node, i as u32);
			}
		}
	}

	pub fn get(&self, node: NodeId) -> &[u32] {
		let Some(from) = self.starts.get(node) else {
			return &[];
		};
		let from = from as usize;
		let to = from + self.pairs[from..].partition_point(|&(n, _)| n == node);
		&self.ids[from..to]
	}

	fn clear(&mut self) {
		self.pairs.clear();
		self.ids.clear();
		self.starts.reset(0);
	}
}

/// The binder's working storage, kept between analyses.
#[derive(Debug, Default)]
struct Scratch {
	envs: Vec<Env>,
	open: Vec<u32>,
	host_declared: Vec<List>,
	env_of: Vec<u32>,
	owned: FastMap<BindingId, u32>,
	/// The name maps of earlier analyses by environment id, emptied: a document of the same shape
	/// finds each environment's map back where it was, so a root's room never lands on a small scope.
	names: Vec<FastMap<StrId, BindingId>>,
}

/// A name map with room for more than this is freed rather than kept.
const POOLED_NAMES: usize = 1024;

/// Where names live while the analysis runs. A scope is one environment, but a function's
/// parameters and its body are two, so a default sees past what the body declares, and a
/// namespace block keeps what it does not export to itself, under the environment its blocks share.
#[derive(Debug, Default)]
struct Env {
	scope: ScopeId,
	parent: Option<u32>,
	names: FastMap<StrId, BindingId>,
	/// Where `var` stops.
	holds_var: bool,
	/// The first environment of its scope: leaving it leaves the scope.
	opens: bool,
	/// A function's parameters, where `arguments` is found unless the function is an arrow.
	parameters: bool,
	arrow: bool,
	/// The function's own `arguments`, once referred to.
	arguments: Option<BindingId>,
	/// The environments of the references named `arguments` this function is the nearest to; when
	/// it closes, each has found a declaration on its way or is the function's own.
	wants_arguments: Vec<u32>,
	/// A function's body, whose names come after its parameters'.
	body: bool,
	/// A namespace block's own names; what it exports goes to the parent.
	local: bool,
}

impl Scopes {
	/// Empties the tables for a tree of `nodes` nodes, keeping the room.
	pub fn clear(&mut self, nodes: usize) {
		self.scopes.clear();
		self.bindings.clear();
		self.references.clear();
		self.roots.clear();
		self.root_of.clear();
		self.of_node.reset(nodes);
		self.of_identifier.reset(nodes);
		self.declared_by.clear();
		self.writes_of.clear();
		self.errors.clear();
		self.scratch.owned.clear();
	}

	pub fn scope(&self, id: ScopeId) -> &Scope {
		&self.scopes[id as usize]
	}

	pub fn binding(&self, id: BindingId) -> &Binding {
		&self.bindings[id as usize]
	}

	pub fn reference(&self, id: ReferenceId) -> &Reference {
		&self.references[id as usize]
	}
}

pub trait Bind: Walk {
	fn bind(&self, binder: &mut Binder<Self>, id: NodeId, mode: Mode);
	/// The value-space parts an extension attaches to a plain node: decorators.
	fn bind_extras(&self, _binder: &mut Binder<Self>, _id: NodeId) {}
	/// The expression an extension node wraps without changing its value: `x as T`, `x!`.
	fn wrapped(&self, _ast: &Ast<Self>, _id: NodeId) -> Option<NodeId> {
		None
	}
	/// Whether an import or export declaration or specifier is type-only, binding no value.
	fn types_only(&self, _ast: &Ast<Self>, _id: NodeId) -> bool {
		false
	}
}

impl Bind for () {
	fn bind(&self, _binder: &mut Binder<Self>, _id: NodeId, _mode: Mode) {
		unreachable!("the JavaScript parser adds no extension nodes")
	}
}

/// What a node is visited as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
	Expression,
	/// A pattern that declares bindings of this kind.
	Declare(BindingKind),
}

/// Runs an analysis under one outermost scope and puts the answer on the tree.
fn analyze_with<X: Bind>(ast: &mut Ast<X>, kind: ScopeKind, root: Option<NodeId>, f: impl FnOnce(&mut Binder<X>)) {
	let reused = ast.scopes.take();
	let mut binder = Binder::new(ast, reused);
	binder.enter(kind, root, false);
	f(&mut binder);
	binder.exit();
	let walked = (
		binder.out.scopes.len(),
		binder.out.bindings.len(),
		binder.out.references.len(),
	);
	binder.resolve_all();
	debug_assert_eq!(
		walked,
		(
			binder.out.scopes.len(),
			binder.out.bindings.len(),
			binder.out.references.len()
		),
		"resolution links what the walk declared, it declares nothing"
	);
	let Binder {
		mut out,
		mut envs,
		open,
		host_declared,
		mut env_of,
		owned,
		..
	} = binder;
	for (id, binding) in out.bindings.iter().enumerate() {
		if let Some(declaration) = binding.declaration {
			out.declared_by.pairs.push((declaration, id as BindingId));
		}
	}
	for (id, reference) in out.references.iter().enumerate() {
		if let Some(expression) = reference.write_expr {
			out.writes_of.pairs.push((expression, id as ReferenceId));
		}
	}
	out.declared_by.finish(ast.nodes.len());
	out.writes_of.finish(ast.nodes.len());
	let mut names = std::mem::take(&mut out.scratch.names);
	if names.len() < envs.len() {
		names.resize_with(envs.len(), FastMap::default);
	}
	for (i, mut env) in envs.drain(..).enumerate() {
		env.names.clear();
		names[i] = if env.names.capacity() > POOLED_NAMES {
			FastMap::default()
		} else {
			env.names
		};
	}
	env_of.clear();
	out.scratch = Scratch {
		envs,
		open,
		host_declared,
		env_of,
		owned,
		names,
	};
	ast.scopes = Some(out);
}

/// Analyzes an answer: a program opens its own scope, a parameter list a function scope whose
/// patterns declare `param` bindings, a pattern a fragment scope whose names are `pattern`
/// bindings, and anything else a fragment scope.
pub fn analyze<X: Bind>(ast: &mut Ast<X>, entry: Entry, roots: List) {
	let root = ast.list(roots).first().copied().flatten();
	match entry {
		Entry::Program => {
			if let NodeKind::Host(_) = ast.node(root.unwrap()).kind {
				return analyze_with(ast, ScopeKind::Module, root, |b| {
					b.visit(root.unwrap(), Mode::Expression)
				});
			}
			let NodeKind::Program { body, module } = ast.node(root.unwrap()).kind else {
				unreachable!()
			};
			let kind = if module { ScopeKind::Module } else { ScopeKind::Script };
			analyze_with(ast, kind, root, |b| b.statements(body));
		}
		Entry::Pattern => analyze_with(ast, ScopeKind::Fragment, root, |b| {
			b.visit(root.unwrap(), Mode::Declare(BindingKind::Pattern))
		}),
		Entry::Params => analyze_with(ast, ScopeKind::Function, None, |b| {
			let ast = b.ast;
			for &param in ast.list(roots).iter().flatten() {
				b.visit(param, Mode::Declare(BindingKind::Param));
			}
		}),
		_ => analyze_with(ast, ScopeKind::Fragment, root, |b| {
			b.visit(root.unwrap(), Mode::Expression)
		}),
	}
}

pub struct Binder<'a, X> {
	ast: &'a Ast<X>,
	out: Scopes,
	envs: Vec<Env>,
	/// The environments open here, innermost last.
	open: Vec<u32>,
	/// The patterns the open host scopes declare, which their fields do not reference.
	host_declared: Vec<List>,
	/// The node whose pattern is being declared, for `Binding::declaration`.
	declaring: Option<NodeId>,
	/// What the target being visited is assigned, for `Reference::write_expr`.
	writing: Option<NodeId>,
	/// The target being visited is read as well: a compound assignment or an update.
	compound: bool,
	/// The environment an `export` was met in: what it declares there is the namespace's to share.
	exporting: Option<u32>,
	/// The environment of each reference, parallel to `Scopes::references`.
	env_of: Vec<u32>,
	/// The shared environment a binding owns, for declarations that merge: namespaces and enums.
	owned: FastMap<BindingId, u32>,
	arguments: Option<StrId>,
	this_name: Option<StrId>,
}

impl<'a, X: Bind> Binder<'a, X> {
	fn new(ast: &'a Ast<X>, reused: Option<Scopes>) -> Self {
		let mut out = match reused {
			Some(mut scopes) => {
				scopes.clear(ast.nodes.len());
				scopes
			}
			None => Scopes {
				of_node: NodeTable::sized(ast.nodes.len()),
				of_identifier: NodeTable::sized(ast.nodes.len()),
				..Scopes::default()
			},
		};
		let Scratch {
			envs,
			open,
			host_declared,
			env_of,
			owned,
			names,
		} = std::mem::take(&mut out.scratch);
		out.scratch.names = names;
		Binder {
			ast,
			out,
			envs,
			open,
			host_declared,
			declaring: None,
			writing: None,
			compound: false,
			exporting: None,
			env_of,
			owned,
			arguments: ast.strings.find("arguments"),
			this_name: ast.strings.find("this"),
		}
	}

	pub fn ast(&self) -> &'a Ast<X> {
		self.ast
	}

	fn kind(&self, id: NodeId) -> NodeKind {
		self.ast.node(id).kind
	}

	fn env(&self) -> u32 {
		*self.open.last().unwrap()
	}

	fn current(&self) -> ScopeId {
		self.envs[self.env() as usize].scope
	}

	pub fn enter(&mut self, kind: ScopeKind, node: Option<NodeId>, arrow: bool) {
		let parent = self.open.last().map(|&env| self.envs[env as usize].scope);
		let function_depth =
			parent.map_or(0, |p| self.out.scopes[p as usize].function_depth) + u32::from(kind == ScopeKind::Function);
		let id = self.out.scopes.len() as ScopeId;
		self.out.scopes.push(Scope {
			kind,
			node,
			parent,
			function_depth,
			top_level_await: false,
		});
		if let Some(node) = node {
			self.out.of_node.insert(node, id);
		}
		let function = kind == ScopeKind::Function;
		self.open_env(Env {
			scope: id,
			holds_var: kind.holds_var() && !function,
			opens: true,
			parameters: function,
			arrow,
			..Env::default()
		});
	}

	fn open_env(&mut self, mut env: Env) {
		let id = self.envs.len() as u32;
		env.parent = self.open.last().copied();
		env.names = self
			.out
			.scratch
			.names
			.get_mut(id as usize)
			.map(std::mem::take)
			.unwrap_or_default();
		self.envs.push(env);
		self.open.push(id);
	}

	/// A function's body, after its parameters.
	fn open_body(&mut self) {
		let scope = self.current();
		self.open_env(Env {
			scope,
			holds_var: true,
			body: true,
			..Env::default()
		});
	}

	/// A namespace block's own environment, under the one its blocks share, which is open.
	fn open_local(&mut self, shared: u32) {
		let scope = self.envs[shared as usize].scope;
		self.open_env(Env {
			scope,
			holds_var: true,
			local: true,
			..Env::default()
		});
	}

	/// Opens the scope a binding owns, or reopens it when the binding declared one before, as
	/// the blocks of one namespace share what they export.
	pub fn enter_owned(&mut self, kind: ScopeKind, node: NodeId, binding: Option<BindingId>) {
		if let Some(&shared) = binding.and_then(|b| self.owned.get(&b)) {
			self.out.of_node.insert(node, self.envs[shared as usize].scope);
			self.open.push(shared);
			self.open_local(shared);
			return;
		}
		self.enter(kind, Some(node), false);
		let shared = self.env();
		if let Some(binding) = binding {
			self.owned.insert(binding, shared);
		}
		if kind == ScopeKind::Namespace {
			self.open_local(shared);
		}
	}

	/// The binding an identifier declares, once declared.
	pub fn declared_by(&self, node: NodeId) -> Option<BindingId> {
		match self.out.of_identifier.get(node) {
			Some(Role::Declares(binding)) => Some(binding),
			_ => None,
		}
	}

	/// Leaves the scope: its environments close, the one that opened it last.
	pub fn exit(&mut self) {
		while let Some(env) = self.open.pop() {
			if self.envs[env as usize].opens {
				if self.envs[env as usize].parameters && !self.envs[env as usize].arrow {
					self.settle_arguments(env);
				}
				break;
			}
		}
	}

	/// The function at `function` is complete, so are the declarations between it and each
	/// reference to `arguments` it is the nearest function to: the reference that meets none on
	/// its way is the function's own, declared here, while the piece of the tree is still being
	/// walked.
	fn settle_arguments(&mut self, function: u32) {
		let (Some(name), wants) = (
			self.arguments,
			std::mem::take(&mut self.envs[function as usize].wants_arguments),
		) else {
			return;
		};
		for mut env in wants {
			let implicit = loop {
				if self.envs[env as usize].names.contains_key(&name) {
					break false;
				}
				if env == function {
					break true;
				}
				env = self.envs[env as usize].parent.unwrap();
			};
			if implicit {
				self.implicit_arguments(function);
			}
		}
	}

	/// The nearest function around here that has an `arguments` of its own.
	fn nearest_function(&self) -> Option<u32> {
		self.open
			.iter()
			.rev()
			.copied()
			.find(|&env| self.envs[env as usize].parameters && !self.envs[env as usize].arrow)
	}

	/// Every reference resolved, once everything is declared: through its environment and those
	/// around it, to the function's own `arguments` at a parameter boundary, else to nothing.
	/// Nothing is declared here: the walk declared everything, the implicit `arguments` included.
	fn resolve_all(&mut self) {
		for i in 0..self.out.references.len() {
			let NodeKind::Identifier { name } = self.kind(self.out.references[i].node) else {
				unreachable!()
			};
			let mut env = Some(self.env_of[i]);
			let binding = loop {
				let Some(at) = env else { break None };
				let here = &self.envs[at as usize];
				if let Some(&binding) = here.names.get(&name) {
					break Some(binding);
				}
				if here.parameters && !here.arrow && Some(name) == self.arguments {
					debug_assert!(here.arguments.is_some(), "an `arguments` the walk did not settle");
					break here.arguments;
				}
				env = here.parent;
			};
			self.out.references[i].binding = binding;
		}
	}

	fn implicit_arguments(&mut self, env: u32) {
		if self.envs[env as usize].arguments.is_some() {
			return;
		}
		let id = self.out.bindings.len() as BindingId;
		self.out.bindings.push(Binding {
			name: self.arguments.unwrap(),
			kind: BindingKind::Arguments,
			scope: self.envs[env as usize].scope,
			node: None,
			declaration: None,
		});
		self.envs[env as usize].arguments = Some(id);
	}

	fn declare_in(&mut self, env: u32, name: StrId, kind: BindingKind, node: Option<NodeId>) -> BindingId {
		let id = self.out.bindings.len() as BindingId;
		self.out.bindings.push(Binding {
			name,
			kind,
			scope: self.envs[env as usize].scope,
			node,
			declaration: node.and(self.declaring),
		});
		self.envs[env as usize].names.insert(name, id);
		// a class name declares the outer binding; the one inside its body shares the identifier
		if let Some(node) = node {
			self.out.of_identifier.insert_new(node, Role::Declares(id));
		}
		id
	}

	/// Declares `node`, an identifier, as `declaration` does: in the current scope, or for `var` in
	/// the nearest scope that holds it. A name declared twice keeps its first binding.
	pub fn declare_by(&mut self, node: NodeId, kind: BindingKind, declaration: NodeId) {
		let outer = self.declaring.replace(declaration);
		self.declare(node, kind);
		self.declaring = outer;
	}

	/// Runs `f` with `declaration` as what the patterns it visits declare.
	fn declaring<T>(&mut self, declaration: NodeId, f: impl FnOnce(&mut Self) -> T) -> T {
		let outer = self.declaring.replace(declaration);
		let result = f(self);
		self.declaring = outer;
		result
	}

	/// Runs `f` with `expression` as what the targets it visits are assigned, `compound` when
	/// they are read too.
	fn writing<T>(&mut self, expression: Option<NodeId>, compound: bool, f: impl FnOnce(&mut Self) -> T) -> T {
		let outer = (
			std::mem::replace(&mut self.writing, expression),
			std::mem::replace(&mut self.compound, compound),
		);
		let result = f(self);
		(self.writing, self.compound) = outer;
		result
	}

	/// Runs `f` under an `export`: what it declares right here, a namespace block shares.
	fn exported<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
		let outer = self.exporting.replace(self.env());
		let result = f(self);
		self.exporting = outer;
		result
	}

	/// The environment a declaration here goes to.
	fn declaring_env(&self, var: bool) -> u32 {
		let mut env = self.env();
		if self.exporting == Some(env) && self.envs[env as usize].local {
			env = self.envs[env as usize].parent.unwrap();
		}
		if var {
			while !self.envs[env as usize].holds_var {
				match self.envs[env as usize].parent {
					Some(parent) => env = parent,
					None => break,
				}
			}
		}
		env
	}

	fn declare(&mut self, node: NodeId, kind: BindingKind) {
		let NodeKind::Identifier { name } = self.kind(node) else {
			return;
		};
		if self.ast.str(name).is_empty() {
			return;
		}
		let env = self.declaring_env(kind.is_var());
		let here = &self.envs[env as usize];
		// a body that declares a parameter's name again keeps the parameter's binding
		let existing = here.names.get(&name).copied().or_else(|| {
			here.body
				.then(|| self.envs[here.parent.unwrap() as usize].names.get(&name).copied())
				.flatten()
		});
		if let Some(existing) = existing {
			let scope = here.scope;
			let opened_by_host = match self.out.scopes[scope as usize] {
				Scope {
					kind: ScopeKind::Fragment,
					..
				} => true,
				Scope {
					kind: ScopeKind::Block,
					node: Some(n),
					..
				} => matches!(self.kind(n), NodeKind::Host(_)),
				_ => false,
			};
			if opened_by_host && !kind.is_var() && !self.out.bindings[existing as usize].kind.is_var() {
				let n = *self.ast.node(node);
				let message: std::borrow::Cow<'static, str> = Code::Redeclaration.with(self.ast.str(name)).into();
				self.out
					.errors
					.push(SyntaxError::with(n.start, Code::Redeclaration, message).to(n.end));
			}
			self.out.of_identifier.insert_new(node, Role::Declares(existing));
			return;
		}
		self.declare_in(env, name, kind, Some(node));
	}

	pub fn reference(&mut self, node: NodeId, write: bool, mutate: bool) {
		if let NodeKind::Identifier { name } = self.kind(node)
			&& self.ast.str(name).is_empty()
		{
			return;
		}
		let id = self.out.references.len() as ReferenceId;
		self.out.references.push(Reference {
			node,
			scope: self.current(),
			binding: None,
			write,
			mutate,
			read: !write || self.compound,
			write_expr: if write { self.writing } else { None },
		});
		self.env_of.push(self.env());
		if let NodeKind::Identifier { name } = self.kind(node)
			&& Some(name) == self.arguments
			&& let Some(function) = self.nearest_function()
		{
			let env = self.env();
			self.envs[function as usize].wants_arguments.push(env);
		}
		self.out.of_identifier.insert(node, Role::Reference(id));
	}

	pub fn statements(&mut self, list: List) {
		self.list(list, Mode::Expression);
	}

	fn list(&mut self, list: List, mode: Mode) {
		for &id in self.ast.list(list).iter().flatten() {
			self.visit(id, mode);
		}
	}

	fn maybe(&mut self, id: Option<NodeId>, mode: Mode) {
		if let Some(id) = id {
			self.visit(id, mode);
		}
	}

	/// The identifier at the root of a member chain, whose value a write to the chain mutates.
	fn mutated_root(&self, mut id: NodeId) -> Option<NodeId> {
		loop {
			match self.kind(id) {
				NodeKind::Identifier { .. } => return Some(id),
				NodeKind::MemberExpression { object, .. } => id = object,
				NodeKind::ChainExpression { expression } => id = expression,
				NodeKind::Extension(_) => {
					id = self.ast.extension.wrapped(self.ast, id)?;
				}
				_ => return None,
			}
		}
	}

	/// Whether an assignment target is a member expression, through parens and wrappers.
	fn is_member(&self, mut id: NodeId) -> bool {
		loop {
			match self.kind(id) {
				NodeKind::MemberExpression { .. } => return true,
				NodeKind::ChainExpression { expression } => id = expression,
				NodeKind::Extension(_) => match self.ast.extension.wrapped(self.ast, id) {
					Some(inner) => id = inner,
					None => return false,
				},
				_ => return false,
			}
		}
	}

	/// A member expression as an assignment target: its parts are read, its root mutated.
	fn assign_member(&mut self, id: NodeId) {
		let root = self.mutated_root(id);
		self.visit_member_target(id, root);
	}

	fn visit_member_target(&mut self, id: NodeId, root: Option<NodeId>) {
		match self.kind(id) {
			NodeKind::Identifier { .. } if Some(id) == root => self.reference(id, false, true),
			NodeKind::MemberExpression {
				object,
				property,
				computed,
				..
			} => {
				self.visit_member_target(object, root);
				if computed {
					self.visit(property, Mode::Expression);
				}
			}
			NodeKind::ChainExpression { expression } => self.visit_member_target(expression, root),
			NodeKind::Extension(_) => match self.ast.extension.wrapped(self.ast, id) {
				Some(inner) => self.visit_member_target(inner, root),
				None => self.visit(id, Mode::Expression),
			},
			_ => self.visit(id, Mode::Expression),
		}
	}

	fn function(&mut self, id: NodeId, params: List, body: NodeId, arrow: bool) {
		// a parameter's decorators are evaluated where the function is defined
		for &param in self.ast.list(params).iter().flatten() {
			self.ast.extension.bind_extras(self, param);
		}
		self.enter(ScopeKind::Function, Some(id), arrow);
		// a TypeScript `this` parameter, first in the list, is a type position and binds nothing
		for (i, &param) in self.ast.list(params).iter().flatten().enumerate() {
			if i == 0 && matches!(self.kind(param), NodeKind::Identifier { name } if Some(name) == self.this_name) {
				continue;
			}
			self.declaring(id, |b| b.visit_with(param, Mode::Declare(BindingKind::Param), false));
		}
		self.open_body();
		match self.kind(body) {
			NodeKind::BlockStatement { body } => self.statements(body),
			_ => self.visit(body, Mode::Expression),
		}
		self.exit();
	}

	/// A class body: a class expression binds its own name inside, a declaration's body sees the
	/// declaration's binding.
	fn class(&mut self, id: NodeId, class: crate::ast::Class, expression: bool) {
		self.enter(ScopeKind::Class, Some(id), false);
		if let (true, Some(name)) = (expression, class.id) {
			self.declare_by(name, BindingKind::ClassName, id);
		}
		self.maybe(class.super_class, Mode::Expression);
		self.visit(class.body, Mode::Expression);
		self.exit();
	}

	fn declaration_kind(kind: VariableKind) -> BindingKind {
		match kind {
			VariableKind::Var => BindingKind::Var,
			VariableKind::Let => BindingKind::Let,
			VariableKind::Const => BindingKind::Const,
			VariableKind::Using => BindingKind::Using,
			VariableKind::AwaitUsing => BindingKind::AwaitUsing,
		}
	}

	fn for_head_declares(&self, head: Option<NodeId>) -> bool {
		head.is_some_and(
			|id| matches!(self.kind(id), NodeKind::VariableDeclaration { kind, .. } if kind != VariableKind::Var),
		)
	}

	pub fn visit(&mut self, id: NodeId, mode: Mode) {
		self.visit_with(id, mode, true);
	}

	/// A host node: its fields in order, inside the scope it opens from `Opens::from` on.
	fn host(&mut self, id: NodeId, index: u32) {
		let host = self.ast.hosts[index as usize];
		let (from, len) = host.fields;
		let Some(opens) = host.scope else {
			for i in from..from + len {
				self.host_field(i);
			}
			return;
		};
		for &pattern in self.ast.list(opens.outside).iter().flatten() {
			self.root(pattern, |b| b.visit(pattern, Mode::Declare(BindingKind::Pattern)));
		}
		let depth = self.host_declared.len();
		self.host_declared.push(opens.outside);
		// groups nest: the outer one opens first at a field and closes last
		let mut groups =
			self.ast.host_groups[opens.groups.0 as usize..(opens.groups.0 + opens.groups.1) as usize].to_vec();
		groups.sort_by_key(|group| (group.from, std::cmp::Reverse(group.until)));
		let mut next = 0;
		let mut open: Vec<usize> = Vec::new();
		for i in from..from + len {
			while next < groups.len() && groups[next].from == i {
				let group = groups[next];
				// a script's program hoists `var` like a script; a fragment is where a template's expressions sit
				let node = group.node.unwrap_or(id);
				let kind = match self.ast.node(node).kind {
					NodeKind::Program { .. } => ScopeKind::Script,
					NodeKind::Host(inner) if !self.ast.hosts[inner as usize].span => ScopeKind::Fragment,
					_ => ScopeKind::Block,
				};
				self.enter(kind, Some(node), false);
				for &pattern in self.ast.list(group.inside).iter().flatten() {
					self.root(pattern, |b| b.visit(pattern, Mode::Declare(BindingKind::Pattern)));
				}
				self.host_declared.push(group.inside);
				open.push(next);
				next += 1;
			}
			self.host_field(i);
			while let Some(&g) = open.last() {
				if groups[g].until != i + 1 {
					break;
				}
				self.exit();
				open.pop();
				self.host_declared.pop();
			}
		}
		self.host_declared.truncate(depth);
	}

	/// A host node's field as an expression, the patterns the open host scopes declare left to them.
	fn host_field(&mut self, i: u32) {
		let declared = |b: &Self, child: NodeId| {
			b.host_declared
				.iter()
				.any(|list| b.ast.list(*list).contains(&Some(child)))
		};
		match self.ast.host_fields[i as usize].1 {
			crate::ast::Value::Node(child) if !declared(self, child) => self.field_value(child),
			crate::ast::Value::Nodes(children) => {
				for &child in self.ast.list(children).iter().flatten() {
					if !declared(self, child) {
						self.field_value(child);
					}
				}
			}
			_ => {}
		}
	}

	/// A host field's value: a host node visited as such, JavaScript as a root of its own.
	fn field_value(&mut self, child: NodeId) {
		if matches!(self.kind(child), NodeKind::Host(_)) {
			self.visit(child, Mode::Expression);
		} else {
			self.root(child, |b| b.visit(child, Mode::Expression));
		}
	}

	/// Visits a piece of JavaScript the host read on its own, recording what the tables gain for it.
	fn root(&mut self, node: NodeId, f: impl FnOnce(&mut Self)) {
		let index = self.out.roots.len() as u32;
		let from = (
			self.out.scopes.len() as u32,
			self.out.bindings.len() as u32,
			self.out.references.len() as u32,
		);
		self.out.roots.push(Root {
			node,
			scope: self.current(),
			scopes: (from.0, from.0),
			bindings: (from.1, from.1),
			references: (from.2, from.2),
		});
		self.out.root_of.insert(node, index);
		f(self);
		let root = &mut self.out.roots[index as usize];
		root.scopes.1 = self.out.scopes.len() as u32;
		root.bindings.1 = self.out.bindings.len() as u32;
		root.references.1 = self.out.references.len() as u32;
	}

	fn visit_with(&mut self, id: NodeId, mode: Mode, extras: bool) {
		use NodeKind::*;
		if extras {
			self.ast.extension.bind_extras(self, id);
		}
		match self.kind(id) {
			Host(index) => self.host(id, index),
			Identifier { .. } => match mode {
				Mode::Expression => self.reference(id, false, false),
				Mode::Declare(kind) => self.declare(id, kind),
			},
			Extension(_) => self.ast.extension.bind(self, id, mode),
			Program { body, .. } => self.statements(body),
			PrivateIdentifier { .. }
			| NumberLiteral { .. }
			| BigIntLiteral
			| StringLiteral { .. }
			| BooleanLiteral { .. }
			| NullLiteral
			| RegExpLiteral { .. }
			| TemplateElement { .. }
			| ThisExpression
			| Super
			| EmptyStatement
			| DebuggerStatement
			| MetaProperty { .. }
			| BreakStatement { .. }
			| ContinueStatement { .. }
			| ImportAttribute { .. }
			| ExportAllDeclaration { .. } => {}
			TemplateLiteral { expressions, .. } => self.list(expressions, Mode::Expression),
			TaggedTemplateExpression { tag, quasi } => {
				self.visit(tag, Mode::Expression);
				self.visit(quasi, Mode::Expression);
			}
			ArrayExpression { elements } => self.list(elements, Mode::Expression),
			ObjectExpression { properties } => self.list(properties, Mode::Expression),
			Property {
				key, value, computed, ..
			} => {
				if computed {
					self.visit(key, Mode::Expression);
				}
				self.visit(value, mode);
			}
			SpreadElement { argument } => self.visit(argument, mode),
			UnaryExpression { argument, operator } => {
				if operator == crate::ast::UnaryOperator::Delete && self.is_member(argument) {
					self.assign_member(argument);
				} else {
					self.visit(argument, Mode::Expression);
				}
			}
			UpdateExpression { argument, .. } => self.writing(None, true, |b| b.target(argument)),
			BinaryExpression { left, right, .. } | LogicalExpression { left, right, .. } => {
				self.visit(left, Mode::Expression);
				self.visit(right, Mode::Expression);
			}
			AssignmentExpression { left, right, operator } => {
				let compound = operator != crate::ast::AssignmentOperator::Assign;
				self.writing(Some(right), compound, |b| b.target(left));
				self.visit(right, Mode::Expression);
			}
			ConditionalExpression {
				test,
				consequent,
				alternate,
			} => {
				self.visit(test, Mode::Expression);
				self.visit(consequent, Mode::Expression);
				self.visit(alternate, Mode::Expression);
			}
			MemberExpression {
				object,
				property,
				computed,
				..
			} => {
				self.visit(object, Mode::Expression);
				if computed {
					self.visit(property, Mode::Expression);
				}
			}
			CallExpression { callee, arguments, .. } | NewExpression { callee, arguments } => {
				self.visit(callee, Mode::Expression);
				self.list(arguments, Mode::Expression);
			}
			ChainExpression { expression } => self.visit(expression, mode),
			SequenceExpression { expressions } => self.list(expressions, Mode::Expression),
			ArrowFunctionExpression { params, body, .. } => self.function(id, params, body, true),
			FunctionExpression { function } => match function.id {
				Some(name) => {
					self.enter(ScopeKind::FunctionName, None, false);
					self.declare_by(name, BindingKind::FunctionName, id);
					self.function(id, function.params, function.body, false);
					self.exit();
				}
				None => self.function(id, function.params, function.body, false),
			},
			FunctionDeclaration { function } => {
				if let Some(name) = function.id {
					self.declare_by(name, BindingKind::Function, id);
				}
				self.function(id, function.params, function.body, false);
			}
			ClassExpression { class } => self.class(id, class, true),
			ClassDeclaration { class } => {
				if let Some(name) = class.id {
					self.declare_by(name, BindingKind::Class, id);
				}
				self.class(id, class, false);
			}
			ClassBody { body } => self.statements(body),
			MethodDefinition {
				key, value, computed, ..
			} => {
				if computed {
					self.visit(key, Mode::Expression);
				}
				self.visit(value, Mode::Expression);
			}
			PropertyDefinition {
				key, value, computed, ..
			} => {
				if computed {
					self.visit(key, Mode::Expression);
				}
				self.maybe(value, Mode::Expression);
			}
			StaticBlock { body } => {
				self.enter(ScopeKind::StaticBlock, Some(id), false);
				self.statements(body);
				self.exit();
			}
			YieldExpression { argument, .. } => self.maybe(argument, Mode::Expression),
			AwaitExpression { argument } => {
				self.awaits();
				self.visit(argument, Mode::Expression)
			}
			ImportExpression { source, options } => {
				self.visit(source, Mode::Expression);
				self.maybe(options, Mode::Expression);
			}
			ObjectPattern { properties } | ArrayPattern { elements: properties } => self.list(properties, mode),
			RestElement { argument } => self.visit(argument, mode),
			AssignmentPattern { left, right } => {
				self.visit(left, mode);
				self.visit(right, Mode::Expression);
			}
			ExpressionStatement { expression, .. } => self.visit(expression, Mode::Expression),
			BlockStatement { body } => {
				self.enter(ScopeKind::Block, Some(id), false);
				self.statements(body);
				self.exit();
			}
			WithStatement { object, body } => {
				self.visit(object, Mode::Expression);
				self.enter(ScopeKind::With, Some(id), false);
				self.visit(body, Mode::Expression);
				self.exit();
			}
			ReturnStatement { argument } => self.maybe(argument, Mode::Expression),
			LabeledStatement { body, .. } => self.visit(body, Mode::Expression),
			IfStatement {
				test,
				consequent,
				alternate,
			} => {
				self.visit(test, Mode::Expression);
				self.visit(consequent, Mode::Expression);
				self.maybe(alternate, Mode::Expression);
			}
			SwitchStatement { discriminant, cases } => {
				self.visit(discriminant, Mode::Expression);
				self.enter(ScopeKind::Switch, Some(id), false);
				self.list(cases, Mode::Expression);
				self.exit();
			}
			SwitchCase { test, consequent } => {
				self.maybe(test, Mode::Expression);
				self.statements(consequent);
			}
			ThrowStatement { argument } => self.visit(argument, Mode::Expression),
			TryStatement {
				block,
				handler,
				finalizer,
			} => {
				self.visit(block, Mode::Expression);
				self.maybe(handler, Mode::Expression);
				self.maybe(finalizer, Mode::Expression);
			}
			CatchClause { param, body } => {
				self.enter(ScopeKind::Catch, Some(id), false);
				self.declaring(id, |b| b.maybe(param, Mode::Declare(BindingKind::CatchParam)));
				self.visit(body, Mode::Expression);
				self.exit();
			}
			WhileStatement { test, body } | DoWhileStatement { body, test } => {
				self.visit(test, Mode::Expression);
				self.visit(body, Mode::Expression);
			}
			ForStatement {
				init,
				test,
				update,
				body,
			} => {
				let scoped = self.for_head_declares(init);
				if scoped {
					self.enter(ScopeKind::For, Some(id), false);
				}
				self.maybe(init, Mode::Expression);
				self.maybe(test, Mode::Expression);
				self.maybe(update, Mode::Expression);
				self.visit(body, Mode::Expression);
				if scoped {
					self.exit();
				}
			}
			ForInStatement { left, right, body } | ForOfStatement { left, right, body, .. } => {
				if matches!(self.kind(id), ForOfStatement { is_await: true, .. }) {
					self.awaits();
				}
				let scoped = self.for_head_declares(Some(left));
				if scoped {
					self.enter(ScopeKind::For, Some(id), false);
				}
				match self.kind(left) {
					VariableDeclaration { .. } => self.visit(left, Mode::Expression),
					_ => self.writing(Some(right), false, |b| b.target(left)),
				}
				self.visit(right, Mode::Expression);
				self.visit(body, Mode::Expression);
				if scoped {
					self.exit();
				}
			}
			VariableDeclaration { declarations, kind } => {
				if kind == VariableKind::AwaitUsing {
					self.awaits();
				}
				let kind = Self::declaration_kind(kind);
				for &declarator in self.ast.list(declarations).iter().flatten() {
					let VariableDeclarator { id: pattern, init } = self.kind(declarator) else {
						continue;
					};
					self.declaring(declarator, |b| b.visit(pattern, Mode::Declare(kind)));
					self.maybe(init, Mode::Expression);
				}
			}
			VariableDeclarator { id: pattern, init } => {
				self.declaring(id, |b| b.visit(pattern, mode));
				self.maybe(init, Mode::Expression);
			}
			ImportDeclaration { specifiers, .. } => {
				if !self.ast.extension.types_only(self.ast, id) {
					self.list(specifiers, Mode::Expression);
				}
			}
			ImportSpecifier { local, .. } | ImportDefaultSpecifier { local } | ImportNamespaceSpecifier { local } => {
				if !self.ast.extension.types_only(self.ast, id) {
					self.declare_by(local, BindingKind::Import, id);
				}
			}
			ExportNamedDeclaration {
				declaration,
				specifiers,
				source,
				..
			} => {
				self.exported(|b| b.maybe(declaration, Mode::Expression));
				if source.is_none() && !self.ast.extension.types_only(self.ast, id) {
					self.list(specifiers, Mode::Expression);
				}
			}
			ExportSpecifier { local, .. } => {
				if let Identifier { .. } = self.kind(local)
					&& !self.ast.extension.types_only(self.ast, id)
				{
					self.reference(local, false, false);
				}
			}
			ExportDefaultDeclaration { declaration } => self.visit(declaration, Mode::Expression),
		}
	}

	/// An assignment or update target: an identifier is written, a member chain mutates its root,
	/// a pattern does both to what it names.
	fn target(&mut self, id: NodeId) {
		match self.kind(id) {
			NodeKind::Identifier { .. } => self.reference(id, true, false),
			NodeKind::MemberExpression { .. } => self.assign_member(id),
			NodeKind::ObjectPattern { properties: list } | NodeKind::ArrayPattern { elements: list } => {
				self.targets(list)
			}
			NodeKind::AssignmentPattern { left, right } => {
				self.target(left);
				self.visit(right, Mode::Expression);
			}
			NodeKind::RestElement { argument } => self.target(argument),
			NodeKind::Property {
				key, value, computed, ..
			} => {
				if computed {
					self.visit(key, Mode::Expression);
				}
				self.target(value);
			}
			NodeKind::Extension(_) => match self.ast.extension.wrapped(self.ast, id) {
				Some(inner) => self.target(inner),
				None => self.visit(id, Mode::Expression),
			},
			_ => self.visit(id, Mode::Expression),
		}
	}

	fn targets(&mut self, list: List) {
		for &id in self.ast.list(list).iter().flatten() {
			self.target(id);
		}
	}

	/// An `await` here: top-level when no function encloses it, for the program or fragment it runs in.
	fn awaits(&mut self) {
		let mut scope = self.current();
		if self.out.scopes[scope as usize].function_depth != 0 {
			return;
		}
		while !matches!(
			self.out.scopes[scope as usize].kind,
			ScopeKind::Module | ScopeKind::Script | ScopeKind::Fragment
		) {
			let Some(parent) = self.out.scopes[scope as usize].parent else {
				break;
			};
			scope = parent;
		}
		self.out.scopes[scope as usize].top_level_await = true;
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn node_tables() {
		use super::{NodeTable, Role};
		use crate::ast::NodeId;
		let mut table: NodeTable<Role> = NodeTable::sized(3);
		assert_eq!(table.get(NodeId(7)).is_none(), true);
		table.insert_new(NodeId(1), Role::Declares(5));
		table.insert_new(NodeId(1), Role::Reference(6));
		assert!(matches!(table.get(NodeId(1)), Some(Role::Declares(5))));
		table.insert(NodeId(2), Role::Reference(6));
		assert!(matches!(table.get(NodeId(2)), Some(Role::Reference(6))));
		assert_eq!(table.iter().count(), 2);
		assert!(table.get(NodeId(0)).is_none());
	}

	#[test]
	fn ids_by_node() {
		let mut by_node = ByNode::default();
		for (node, id) in [(5, 0), (2, 1), (5, 2), (9, 3)] {
			by_node.pairs.push((NodeId(node), id));
		}
		by_node.finish(10);
		assert_eq!(by_node.get(NodeId(5)), [0, 2]);
		assert_eq!(by_node.get(NodeId(2)), [1]);
		assert_eq!(by_node.get(NodeId(9)), [3]);
		assert!(by_node.get(NodeId(0)).is_empty());
		assert!(by_node.get(NodeId(3)).is_empty());
	}

	use super::*;
	use crate::SyntaxError;
	use crate::parser::Options;

	fn analyzed<X: Bind>(result: Result<(Ast<X>, List, u32), SyntaxError>) -> Ast<X> {
		let (mut ast, roots, _) = result.unwrap();
		analyze(&mut ast, Entry::Program, roots);
		ast
	}

	/// Every identifier as `name@start` with what it declares or refers to.
	fn facts(src: &str) -> String {
		facts_in(src, true)
	}

	fn facts_in(src: &str, module: bool) -> String {
		facts_of(&analyzed(crate::parse_at(
			src,
			0,
			None,
			Entry::Program,
			Options {
				module,
				..Options::default()
			},
			"",
		)))
	}

	#[cfg(feature = "typescript")]
	fn ts_facts(src: &str) -> String {
		facts_of(&analyzed(crate::typescript::parse_at(
			src,
			0,
			None,
			Entry::Program,
			Options {
				module: true,
				..Options::default()
			},
			"",
		)))
	}

	fn facts_of<X: Bind>(ast: &Ast<X>) -> String {
		let scopes = ast.scopes.as_ref().unwrap();
		let mut ids: Vec<_> = scopes.of_identifier.iter().collect();
		ids.sort_by_key(|&(id, _)| ast.node(id).start);
		let mut out = Vec::new();
		for (id, role) in ids {
			let node = ast.node(id);
			let NodeKind::Identifier { name } = node.kind else {
				unreachable!()
			};
			let mut line = format!("{}@{} ", ast.str(name), node.start);
			match role {
				Role::Declares(b) => {
					let binding = scopes.binding(b);
					line += &format!(
						"declares {} in {}",
						binding.kind.name(),
						scopes.scope(binding.scope).kind.name()
					);
				}
				Role::Reference(r) => {
					let reference = scopes.reference(r);
					line += &match reference.binding {
						Some(b) => match scopes.binding(b).node {
							Some(node) => format!("-> @{}", ast.node(node).start),
							None => format!("-> {}", scopes.binding(b).kind.name()),
						},
						None => "-> global".into(),
					};
					if reference.write {
						line += " write";
					}
					if reference.mutate {
						line += " mutate";
					}
				}
			}
			out.push(line);
		}
		out.join("\n")
	}

	#[test]
	fn declarations_and_references() {
		assert_eq!(
			facts("let x = 1; x = 2; y.z = x; delete y.w; k++;"),
			"x@4 declares let in module\nx@11 -> @4 write\ny@18 -> global mutate\nx@24 -> @4\ny@34 -> global mutate\nk@39 -> global write"
		);
	}

	#[test]
	fn hoisting_and_blocks() {
		assert_eq!(
			facts("f(); { let a = v; var v; } function f() { return a; }"),
			"f@0 -> @36\na@11 declares let in block\nv@15 -> @22\nv@22 declares var in module\nf@36 declares function in module\na@49 -> global"
		);
	}

	#[test]
	fn functions_and_classes() {
		assert_eq!(
			facts("const f = function g(a = b) { return g(arguments, a); }; class C { m() { return C; } }"),
			"f@6 declares const in module\ng@19 declares function-name in function-name\na@21 declares param in function\nb@25 -> global\ng@37 -> @19\narguments@39 -> arguments\na@50 -> @21\nC@63 declares class in module\nC@80 -> @63"
		);
		assert_eq!(
			facts("const A = class B { static { B; } }; () => arguments;"),
			"A@6 declares const in module\nB@16 declares class-name in class\nB@29 -> @16\narguments@43 -> global"
		);
		// a parameter default sees the function's own `arguments`; the body sees what it declares
		assert_eq!(
			facts_in(
				"function f(a = arguments) { let arguments = 2; return arguments; }",
				false
			),
			"f@9 declares function in script\na@11 declares param in function\narguments@15 -> arguments\narguments@32 declares let in function\narguments@54 -> @32"
		);
	}

	#[test]
	fn patterns_and_loops() {
		assert_eq!(
			facts("let {a, b: [c = d], ...e} = o; [a, c.x] = p; for (const i of a) i; for (a in o);"),
			"a@5 declares let in module\nc@12 declares let in module\nd@16 -> global\ne@23 declares let in module\no@28 -> global\na@32 -> @5 write\nc@35 -> @12 mutate\np@42 -> global\ni@56 declares const in for\na@61 -> @5\ni@64 -> @56\na@72 -> @5 write\no@77 -> global"
		);
	}

	/// The walk declares a function's own `arguments` when the function closes, so a host's root
	/// lists it with the rest of the piece, and resolution declares nothing.
	#[test]
	fn arguments_is_declared_by_the_walk() {
		// nothing to settle: sources without the word, a parameter list on its own
		assert_eq!(
			facts_in("function f(a) { return a; }", false),
			"f@9 declares function in script\na@11 declares param in function\na@25 -> @11"
		);
		let (mut ast, roots, _) =
			crate::parse_at("(a, b = a)", 0, None, Entry::Params, Options::default(), "").unwrap();
		analyze(&mut ast, Entry::Params, roots);
		assert_eq!(ast.scopes.as_ref().unwrap().bindings.len(), 2);
		// a declaration on the way, even a later one, is what a reference means
		for (src, expected) in [
			(
				"function f() { arguments; let arguments; }",
				"f@9 declares function in script\narguments@15 -> @30\narguments@30 declares let in function",
			),
			(
				"function f() { arguments; var arguments; }",
				"f@9 declares function in script\narguments@15 -> @30\narguments@30 declares var in function",
			),
			(
				"function f() { { let arguments; arguments; } arguments; }",
				"f@9 declares function in script\narguments@21 declares let in block\narguments@32 -> @21\narguments@45 -> arguments",
			),
			(
				"function f() { try {} catch (arguments) { arguments } arguments }",
				"f@9 declares function in script\narguments@29 declares catch in catch\narguments@42 -> @29\narguments@54 -> arguments",
			),
			(
				"function f(arguments) { arguments }",
				"f@9 declares function in script\narguments@11 declares param in function\narguments@24 -> @11",
			),
			(
				"function f() { return () => { function g() { return () => arguments } return arguments } }",
				"f@9 declares function in script\ng@44 declares function in block\narguments@61 -> arguments\narguments@80 -> arguments",
			),
			(
				"(function arguments() { arguments })",
				"arguments@10 declares function-name in function-name\narguments@24 -> arguments",
			),
		] {
			assert_eq!(facts_in(src, false), expected, "{src}");
		}
		// a class field or static block cannot say `arguments` at all
		for src in ["class C { x = arguments }", "class C { static { arguments } }"] {
			assert!(
				crate::parse_at(src, 0, None, Entry::Program, Options::default(), "").is_err(),
				"{src}"
			);
		}
		// a host document: each piece's root lists the `arguments` of the functions inside it
		let grammar = crate::host::grammar::Grammar::read(
			&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/hosts/svelte.grammar")).unwrap(),
		)
		.unwrap();
		let src = "<script>let n = 1; function f() { return arguments; }</script>\n{(function () { return arguments + n; })()}";
		let (mut ast, root) = crate::host::parse_document::<()>(src, &grammar, Options::default(), None);
		let roots = ast.add_list(&[Some(root.unwrap())]);
		analyze(&mut ast, Entry::Program, roots);
		let scopes = ast.scopes.as_ref().unwrap();
		let mut implicit = 0;
		for (i, binding) in scopes.bindings.iter().enumerate() {
			if binding.kind != BindingKind::Arguments {
				continue;
			}
			implicit += 1;
			let function = ast.node(scopes.scope(binding.scope).node.unwrap());
			let root = scopes
				.roots
				.iter()
				.find(|r| (r.bindings.0 as usize..r.bindings.1 as usize).contains(&i))
				.expect("in a root");
			let piece = ast.node(root.node);
			assert!(
				piece.start <= function.start && function.end <= piece.end,
				"{i} listed by the wrong piece"
			);
		}
		assert_eq!(implicit, 2);
		assert!(scopes.references.iter().all(|r| r.binding.is_some()));
	}

	#[test]
	fn parameters_see_past_the_body() {
		assert_eq!(
			facts("let v = 1; function f(o = v) { const v = 2; return o + v; }"),
			"v@4 declares let in module\nf@20 declares function in module\no@22 declares param in function\nv@26 -> @4\nv@37 declares const in function\no@51 -> @22\nv@55 -> @37"
		);
	}

	#[test]
	fn delete_and_wrappers() {
		assert_eq!(
			facts_in("delete x; delete y.z; delete (w).v;", false),
			"x@7 -> global\ny@17 -> global mutate\nw@30 -> global mutate"
		);
		#[cfg(feature = "typescript")]
		{
			let src = "(a as any).b = 1; (c!).d = 2; (e as any) = 3;";
			let ast = analyzed(crate::typescript::parse_at(
				src,
				0,
				None,
				Entry::Program,
				Options {
					module: true,
					..Options::default()
				},
				"",
			));
			let scopes = ast.scopes.as_ref().unwrap();
			let flags: Vec<_> = scopes.references.iter().map(|r| (r.write, r.mutate)).collect();
			assert_eq!(flags, [(false, true), (false, true), (true, false)]);
		}
	}

	#[test]
	#[cfg(feature = "typescript")]
	fn typescript_declarations() {
		let src = "import type { X } from 'm'; import { type Y, Z } from 'm'; export { type X }; enum E { A = 1, B = A } namespace N { export const n = 1; } namespace N { n; } export function f(): void; class C { m(@dec p) {} } function dec() {} function g(this: T, a) {} enum M { this = 1 }";
		let ast = analyzed(crate::typescript::parse_at(
			src,
			0,
			None,
			Entry::Program,
			Options {
				module: true,
				..Options::default()
			},
			"",
		));
		let scopes = ast.scopes.as_ref().unwrap();
		let names: Vec<_> = scopes
			.bindings
			.iter()
			.map(|b| format!("{}:{}", ast.str(b.name), b.kind.name()))
			.collect();
		assert_eq!(
			names,
			[
				"Z:import",
				"E:enum",
				"A:enum-member",
				"B:enum-member",
				"N:namespace",
				"n:const",
				"f:function",
				"C:class",
				"p:param",
				"dec:function",
				"g:function",
				"a:param",
				"M:enum",
				"this:enum-member",
			]
		);
		let unresolved = scopes.references.iter().filter(|r| r.binding.is_none()).count();
		assert_eq!(unresolved, 0);
		let dec = scopes.bindings.iter().position(|b| ast.str(b.name) == "dec").unwrap() as BindingId;
		let dec_reference = scopes.references.iter().find(|r| r.binding == Some(dec)).unwrap();
		assert_eq!(scopes.scope(dec_reference.scope).kind, ScopeKind::Class);
	}

	/// A namespace's blocks share what they export and keep the rest to themselves; a type-only
	/// import-equals binds nothing.
	#[test]
	#[cfg(feature = "typescript")]
	fn namespaces_merge() {
		let cases = [
			"const x = 0; namespace N { const x = 1; } namespace N { export const y = x; }",
			"const x = 0; namespace N { export const read = () => x; } namespace N { export const x = 1; }",
			"namespace N { const x = 1; x; } namespace N { const x = 2; x; }",
			"import type X = require('m'); X;",
		];
		let facts: Vec<String> = cases.iter().map(|src| ts_facts(src)).collect();
		assert_eq!(
			facts.join("\n\n"),
			"x@6 declares const in module\nN@23 declares namespace in module\nx@33 declares const in namespace\nN@52 declares namespace in module\ny@69 declares const in namespace\nx@73 -> @6\n\nx@6 declares const in module\nN@23 declares namespace in module\nread@40 declares const in namespace\nx@53 -> @85\nN@68 declares namespace in module\nx@85 declares const in namespace\n\nN@10 declares namespace in module\nx@20 declares const in namespace\nx@27 -> @20\nN@42 declares namespace in module\nx@52 declares const in namespace\nx@59 -> @52\n\nX@30 -> global"
		);
	}

	#[test]
	#[cfg(feature = "typescript")]
	fn typescript_values_and_types() {
		let src = "enum E { A = x } namespace N { export const n: T = y as T; } let t: T; @d class C { constructor(public p: P) {} }";
		let ast = analyzed(crate::typescript::parse_at(
			src,
			0,
			None,
			Entry::Program,
			Options {
				module: true,
				..Options::default()
			},
			"",
		));
		let scopes = ast.scopes.as_ref().unwrap();
		let names: Vec<_> = scopes
			.bindings
			.iter()
			.map(|b| format!("{}:{}", ast.str(b.name), b.kind.name()))
			.collect();
		assert_eq!(
			names,
			[
				"E:enum",
				"A:enum-member",
				"N:namespace",
				"n:const",
				"t:let",
				"C:class",
				"p:param"
			]
		);
		let globals: Vec<_> = scopes
			.references
			.iter()
			.filter(|r| r.binding.is_none())
			.map(|r| match ast.node(r.node).kind {
				NodeKind::Identifier { name } => ast.str(name).to_string(),
				_ => unreachable!(),
			})
			.collect();
		assert_eq!(globals, ["x", "y", "d"]);
	}

	#[test]
	fn declarations_writes_and_top_level_await() {
		let src = "let [a = 1, b] = c; a = b + 1; a++; for (b of c) {} function f(p) { g = p; } await c;";
		let ast = analyzed(crate::parse_at(
			src,
			0,
			None,
			Entry::Program,
			Options {
				module: true,
				..Default::default()
			},
			"",
		));
		let scopes = ast.scopes.as_ref().unwrap();
		let start = |id: Option<NodeId>| id.map(|id| ast.node(id).start);
		let declarations: Vec<_> = scopes
			.bindings
			.iter()
			.map(|b| (ast.str(b.name), start(b.declaration)))
			.collect();
		assert_eq!(
			declarations,
			[("a", Some(4)), ("b", Some(4)), ("f", Some(52)), ("p", Some(52))]
		);
		let writes: Vec<_> = scopes
			.references
			.iter()
			.filter(|r| r.write)
			.map(|r| (ast.node(r.node).start, start(r.write_expr)))
			.collect();
		// `a = b + 1`, `a++`, `for (b of c)`, `g = p`
		assert_eq!(writes, [(20, Some(24)), (31, None), (41, Some(46)), (68, Some(72))]);
		let reads: Vec<_> = scopes
			.references
			.iter()
			.map(|r| (ast.node(r.node).start, r.read))
			.collect();
		// only the update reads what it writes; the parameter read after `g =` is a plain read
		assert_eq!(
			&reads[..5],
			[(17, true), (20, false), (24, true), (31, true), (41, false)]
		);
		assert!(scopes.scopes[0].top_level_await);
		assert!(!scopes.scopes[1].top_level_await);
	}

	#[test]
	fn imports_and_exports() {
		let ast = analyzed(crate::parse_at(
			"import { a } from 'a'; export { a as b }; function f() { return () => a; }",
			0,
			None,
			Entry::Program,
			Options {
				module: true,
				..Options::default()
			},
			"",
		));
		let scopes = ast.scopes.as_ref().unwrap();
		assert_eq!(scopes.scopes.len(), 3);
		let a = &scopes.bindings[0];
		let references = scopes.references.iter().filter(|r| r.binding == Some(0)).count();
		assert_eq!((ast.str(a.name), a.kind, references), ("a", BindingKind::Import, 2));
		assert_eq!(scopes.scopes[2].function_depth, 2);
	}

	#[test]
	fn pooled_maps_stay_where_they_were() {
		let document = |functions: usize| {
			let mut src: String = (0..300).map(|i| format!("let v{i};")).collect();
			for f in 0..functions {
				src.push_str(&format!("function f{f}(x) {{ return x; }}"));
			}
			src
		};
		let mut pool = None;
		let mut room = Vec::new();
		for round in 0..12 {
			let (mut ast, roots, _) = crate::parse_at(
				&document(5 + round % 2),
				0,
				None,
				Entry::Program,
				Options::default(),
				"",
			)
			.unwrap();
			ast.scopes = pool.take();
			analyze(&mut ast, Entry::Program, roots);
			let scopes = ast.scopes.take().unwrap();
			room.push(scopes.scratch.names.iter().map(|m| m.capacity()).sum::<usize>());
			pool = Some(scopes);
		}
		assert_eq!(room[2..].iter().max(), room[2..].iter().min(), "{room:?}");
	}
}
