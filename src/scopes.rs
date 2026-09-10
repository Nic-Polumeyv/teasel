//! Scope analysis over a parsed tree: the scopes, the bindings each declares, and every
//! identifier resolved to the binding it names. References are resolved when their scope closes,
//! after every declaration in it is known, so hoisting needs no second pass.

use crate::ast::{Ast, List, NodeId, NodeKind, VariableKind, Walk};
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

#[derive(Debug, Default)]
pub struct Scopes {
	pub scopes: Vec<Scope>,
	pub bindings: Vec<Binding>,
	pub references: Vec<Reference>,
	pub of_node: NodeTable<ScopeId>,
	pub of_identifier: NodeTable<Role>,
	/// The bindings each declaring node declares, and the write references each expression is
	/// assigned by: `Binding::declaration` and `Reference::write_expr` from the node's side.
	pub declared_by: FastMap<NodeId, Vec<BindingId>>,
	pub writes_of: FastMap<NodeId, Vec<ReferenceId>>,
}

impl Scopes {
	/// Empties the tables for a tree of `nodes` nodes, keeping the room.
	pub fn clear(&mut self, nodes: usize) {
		self.scopes.clear();
		self.bindings.clear();
		self.references.clear();
		self.of_node.reset(nodes);
		self.of_identifier.reset(nodes);
		self.declared_by.clear();
		self.writes_of.clear();
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

/// How an extension's nodes join the analysis: which of their children are values, and what
/// they declare.
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
	let mut out = binder.out;
	for (id, binding) in out.bindings.iter().enumerate() {
		if let Some(declaration) = binding.declaration {
			out.declared_by.entry(declaration).or_default().push(id as BindingId);
		}
	}
	for (id, reference) in out.references.iter().enumerate() {
		if let Some(expression) = reference.write_expr {
			out.writes_of.entry(expression).or_default().push(id as ReferenceId);
		}
	}
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

/// What the analysis keeps about a scope while it runs, parallel to `Scopes::scopes`.
#[derive(Default)]
struct Open {
	/// An arrow function has no `arguments` of its own.
	arrow: bool,
	/// Where a function's body starts: a parameter default cannot see what the body declares.
	body_start: u32,
	names: FastMap<StrId, BindingId>,
}

pub struct Binder<'a, X> {
	ast: &'a Ast<X>,
	out: Scopes,
	stack: Vec<ScopeId>,
	open: Vec<Open>,
	/// The patterns the open host scopes declare, which their fields do not reference.
	host_declared: Vec<List>,
	/// The node whose pattern is being declared, for `Binding::declaration`.
	declaring: Option<NodeId>,
	/// What the target being visited is assigned, for `Reference::write_expr`.
	writing: Option<NodeId>,
	/// The target being visited is read as well: a compound assignment or an update.
	compound: bool,
	/// The references not yet resolved, those of each open scope after `pending_from`'s entry for it.
	pending: Vec<ReferenceId>,
	pending_from: Vec<usize>,
	/// The scope a binding owns, for declarations that merge: namespaces and enums.
	owned: FastMap<BindingId, ScopeId>,
	arguments: Option<StrId>,
	this_name: Option<StrId>,
}

impl<'a, X: Bind> Binder<'a, X> {
	fn new(ast: &'a Ast<X>, reused: Option<Scopes>) -> Self {
		let out = match reused {
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
		Binder {
			ast,
			out,
			stack: Vec::new(),
			open: Vec::new(),
			host_declared: Vec::new(),
			declaring: None,
			writing: None,
			compound: false,
			pending: Vec::new(),
			pending_from: Vec::new(),
			owned: FastMap::default(),
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

	fn current(&self) -> ScopeId {
		*self.stack.last().unwrap()
	}

	pub fn enter(&mut self, kind: ScopeKind, node: Option<NodeId>, arrow: bool) {
		let parent = self.stack.last().copied();
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
		self.open.push(Open {
			arrow,
			..Open::default()
		});
		if let Some(node) = node {
			self.out.of_node.insert(node, id);
		}
		self.stack.push(id);
		self.pending_from.push(self.pending.len());
	}

	/// Opens the scope a binding owns, or reopens it when the binding declared one before, as
	/// the blocks of one namespace share their names.
	pub fn enter_owned(&mut self, kind: ScopeKind, node: NodeId, binding: Option<BindingId>) {
		if let Some(&scope) = binding.and_then(|b| self.owned.get(&b)) {
			self.out.of_node.insert(node, scope);
			self.stack.push(scope);
			self.pending_from.push(self.pending.len());
			return;
		}
		self.enter(kind, Some(node), false);
		if let Some(binding) = binding {
			self.owned.insert(binding, self.current());
		}
	}

	/// The binding an identifier declares, once declared.
	pub fn declared_by(&self, node: NodeId) -> Option<BindingId> {
		match self.out.of_identifier.get(node) {
			Some(Role::Declares(binding)) => Some(binding),
			_ => None,
		}
	}

	/// Closes the scope: its references resolve here or stay pending for the scope around it.
	pub fn exit(&mut self) {
		let scope = self.stack.pop().unwrap();
		let from = self.pending_from.pop().unwrap();
		let mut kept = from;
		for i in from..self.pending.len() {
			let reference = self.pending[i];
			let NodeKind::Identifier { name } = self.kind(self.out.references[reference as usize].node) else {
				unreachable!()
			};
			let mut found = self.open[scope as usize].names.get(&name).copied();
			if let Some(binding) = found
				&& self.declared_in_body(scope, reference, binding)
			{
				found = None;
			}
			if found.is_none()
				&& self.out.scopes[scope as usize].kind == ScopeKind::Function
				&& !self.open[scope as usize].arrow
				&& Some(name) == self.arguments
			{
				found = Some(self.declare_in(scope, name, BindingKind::Arguments, None));
			}
			match found {
				Some(binding) => self.resolve(reference, binding),
				None => {
					self.pending[kept] = reference;
					kept += 1;
				}
			}
		}
		self.pending.truncate(kept);
		if self.pending_from.is_empty() {
			self.pending.clear();
		}
	}

	/// A reference in a function's parameters to a name the body declares: the body's binding is
	/// not the one, the parameters see past the function.
	fn declared_in_body(&self, scope: ScopeId, reference: ReferenceId, binding: BindingId) -> bool {
		let body_start = self.open[scope as usize].body_start;
		body_start > 0
			&& self.ast.node(self.out.references[reference as usize].node).start < body_start
			&& self.out.bindings[binding as usize]
				.node
				.is_some_and(|node| self.ast.node(node).start >= body_start)
	}

	fn resolve(&mut self, reference: ReferenceId, binding: BindingId) {
		self.out.references[reference as usize].binding = Some(binding);
	}

	fn declare_in(&mut self, scope: ScopeId, name: StrId, kind: BindingKind, node: Option<NodeId>) -> BindingId {
		let id = self.out.bindings.len() as BindingId;
		self.out.bindings.push(Binding {
			name,
			kind,
			scope,
			node,
			declaration: node.and(self.declaring),
		});
		self.open[scope as usize].names.insert(name, id);
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

	fn declare(&mut self, node: NodeId, kind: BindingKind) {
		let NodeKind::Identifier { name } = self.kind(node) else {
			return;
		};
		if self.ast.str(name).is_empty() {
			return;
		}
		let mut scope = self.current();
		if kind.is_var() {
			while !self.out.scopes[scope as usize].kind.holds_var() {
				scope = self.out.scopes[scope as usize].parent.unwrap();
			}
		}
		if let Some(&existing) = self.open[scope as usize].names.get(&name) {
			self.out.of_identifier.insert_new(node, Role::Declares(existing));
			return;
		}
		self.declare_in(scope, name, kind, Some(node));
	}

	pub fn reference(&mut self, node: NodeId, write: bool, mutate: bool) {
		if let NodeKind::Identifier { name } = self.kind(node)
			&& self.ast.str(name).is_empty()
		{
			return;
		}
		let id = self.out.references.len() as ReferenceId;
		let scope = self.current();
		self.out.references.push(Reference {
			node,
			scope,
			binding: None,
			write,
			mutate,
			read: !write || self.compound,
			write_expr: if write { self.writing } else { None },
		});
		self.out.of_identifier.insert(node, Role::Reference(id));
		self.pending.push(id);
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
		let scope = self.current();
		self.open[scope as usize].body_start = self.ast.node(body).start;
		// a TypeScript `this` parameter, first in the list, is a type position and binds nothing
		for (i, &param) in self.ast.list(params).iter().flatten().enumerate() {
			if i == 0 && matches!(self.kind(param), NodeKind::Identifier { name } if Some(name) == self.this_name) {
				continue;
			}
			self.declaring(id, |b| b.visit_with(param, Mode::Declare(BindingKind::Param), false));
		}
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
			self.visit(pattern, Mode::Declare(BindingKind::Let));
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
				self.enter(ScopeKind::Block, Some(group.node.unwrap_or(id)), false);
				for &pattern in self.ast.list(group.inside).iter().flatten() {
					self.visit(pattern, Mode::Declare(BindingKind::Let));
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
			crate::ast::Value::Node(child) if !declared(self, child) => self.visit(child, Mode::Expression),
			crate::ast::Value::Nodes(children) => {
				for &child in self.ast.list(children).iter().flatten() {
					if !declared(self, child) {
						self.visit(child, Mode::Expression);
					}
				}
			}
			_ => {}
		}
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
				self.maybe(declaration, Mode::Expression);
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

	/// An `await` here: top-level when no function encloses it.
	fn awaits(&mut self) {
		if self.out.scopes[self.current() as usize].function_depth == 0 {
			self.out.scopes[0].top_level_await = true;
		}
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
		let ast = analyzed(crate::parse_at(
			src,
			0,
			None,
			Entry::Program,
			Options {
				module,
				..Options::default()
			},
			"",
		));
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
	}

	#[test]
	fn patterns_and_loops() {
		assert_eq!(
			facts("let {a, b: [c = d], ...e} = o; [a, c.x] = p; for (const i of a) i; for (a in o);"),
			"a@5 declares let in module\nc@12 declares let in module\nd@16 -> global\ne@23 declares let in module\no@28 -> global\na@32 -> @5 write\nc@35 -> @12 mutate\np@42 -> global\ni@56 declares const in for\na@61 -> @5\ni@64 -> @56\na@72 -> @5 write\no@77 -> global"
		);
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

	#[test]
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

	#[test]
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
}
