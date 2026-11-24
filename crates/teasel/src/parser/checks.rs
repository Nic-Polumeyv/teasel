//! What the grammar alone does not say. The scopes a parse opens and the names declared in
//! them, labels, private names, reserved words, assignment and binding targets, and the facts of
//! a literal that may yet be a pattern.

use super::parse::Parser;
use crate::ast::{AssignmentOperator, List, NodeId, NodeKind, PropertyKind};
use crate::error::Code;
use crate::interner::{FastMap, StrId};
use crate::parser::word;

pub(super) const SCOPE_TOP: u32 = 1;
pub(super) const SCOPE_FUNCTION: u32 = 2;
pub(super) const SCOPE_ASYNC: u32 = 4;
pub(super) const SCOPE_GENERATOR: u32 = 8;
pub(super) const SCOPE_ARROW: u32 = 16;
pub(super) const SCOPE_SIMPLE_CATCH: u32 = 32;
pub(super) const SCOPE_SUPER: u32 = 64;
pub(super) const SCOPE_DIRECT_SUPER: u32 = 128;
pub(super) const SCOPE_CLASS_STATIC_BLOCK: u32 = 256;
pub(super) const SCOPE_CLASS_FIELD_INIT: u32 = 512;
pub(super) const SCOPE_TS_MODULE: u32 = 1024;
pub(super) const SCOPE_VAR: u32 = SCOPE_TOP | SCOPE_FUNCTION | SCOPE_CLASS_STATIC_BLOCK | SCOPE_TS_MODULE;
const SCOPE_VAR_LIKE: u32 = SCOPE_VAR | SCOPE_CLASS_FIELD_INIT;

pub(super) fn function_flags(is_async: bool, generator: bool) -> u32 {
	SCOPE_FUNCTION | if is_async { SCOPE_ASYNC } else { 0 } | if generator { SCOPE_GENERATOR } else { 0 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Binding {
	None,
	Var,
	Lexical,
	Function,
	SimpleCatch,
	Outside,
}

const VAR: u8 = 1;
const LEXICAL: u8 = 2;
const FUNCTION: u8 = 4;
const TYPE: u8 = 8;
const EXPORT_ONLY: u8 = 16;

pub(super) struct Scope {
	pub flags: u32,
}

#[derive(Clone, Copy)]
struct Name {
	name: StrId,
	kind: u8,
	/// What the index held for the name before.
	before: u8,
	/// The mark it was added after.
	serial: u32,
}

/// Names in the order they were added, and by name once a scan costs more than a lookup: the
/// index is there from `INDEXED` names on.
#[derive(Default)]
pub(super) struct Names {
	list: Vec<Name>,
	index: FastMap<StrId, u8>,
}

const INDEXED: usize = 64;

impl Names {
	pub(super) fn len(&self) -> usize {
		self.list.len()
	}

	fn first(&self) -> Option<StrId> {
		self.list.first().map(|n| n.name)
	}

	#[inline(always)]
	pub(super) fn has(&self, name: StrId, kinds: u8) -> bool {
		if self.list.len() >= INDEXED {
			return self.indexed_has(name, kinds);
		}
		self.list.iter().any(|n| n.name == name && n.kind & kinds != 0)
	}

	#[cold]
	#[inline(never)]
	fn indexed_has(&self, name: StrId, kinds: u8) -> bool {
		self.index.get(&name).is_some_and(|kind| kind & kinds != 0)
	}

	#[inline(always)]
	pub(super) fn push(&mut self, name: StrId, kind: u8, serial: u32) {
		debug_assert!(self.list.last().is_none_or(|n| n.serial <= serial));
		if self.list.len() >= INDEXED - 1 {
			return self.indexed_push(name, kind, serial);
		}
		self.list.push(Name {
			name,
			kind,
			before: 0,
			serial,
		});
	}

	#[cold]
	#[inline(never)]
	fn indexed_push(&mut self, name: StrId, kind: u8, serial: u32) {
		if self.list.len() < INDEXED {
			for n in &self.list {
				*self.index.entry(n.name).or_default() |= n.kind;
			}
		}
		let held = self.index.entry(name).or_default();
		let before = std::mem::replace(held, *held | kind);
		self.list.push(Name {
			name,
			kind,
			before,
			serial,
		});
	}

	#[inline(always)]
	pub(super) fn truncate(&mut self, len: usize) {
		if self.list.len() >= INDEXED {
			return self.indexed_truncate(len);
		}
		self.list.truncate(len);
	}

	#[cold]
	#[inline(never)]
	fn indexed_truncate(&mut self, len: usize) {
		if len >= self.list.len() {
			return;
		}
		if len < INDEXED {
			self.index.clear();
		} else {
			// rebuilding the index instead was quadratic in a long scope's skipped statements
			for n in self.list[len..].iter().rev() {
				if n.before == 0 {
					self.index.remove(&n.name);
				} else {
					self.index.insert(n.name, n.before);
				}
			}
		}
		self.list.truncate(len);
	}

	#[inline(always)]
	fn forget(&mut self, serial: u32) {
		if self.list.last().is_some_and(|n| n.serial >= serial) {
			let kept = self.list.iter().rposition(|n| n.serial < serial).map_or(0, |i| i + 1);
			self.truncate(kept);
		}
	}

	#[inline(always)]
	fn clear(&mut self) {
		self.truncate(0);
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LabelKind {
	None,
	Loop,
	Switch,
}

pub(super) struct Label {
	pub name: Option<StrId>,
	pub kind: LabelKind,
	pub statement_start: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PrivateKind {
	Other,
	Get,
	Set,
	StaticGet,
	StaticSet,
}

#[derive(Default)]
pub(super) struct PrivateScope {
	pub declared: Vec<(StrId, PrivateKind)>,
	pub used: Vec<(StrId, u32)>,
}

/// Errors that only become real once an expression turns out to be a pattern, or the reverse.
#[derive(Clone, Copy, Default)]
pub(super) struct Facts {
	pub shorthand_assign: Option<u32>,
	pub trailing_comma: Option<u32>,
	pub parenthesized_assign: Option<u32>,
	pub double_proto: Option<u32>,
	pub owner: Option<NodeId>,
	pub operand: Option<(u32, Code)>,
}

fn first_pending(pending: [Option<(u32, Code)>; 3]) -> Option<(u32, Code)> {
	pending.into_iter().flatten().min_by_key(|&(pos, _)| pos)
}

impl Facts {
	pub(super) fn merge(&mut self, element: Facts) {
		self.shorthand_assign = self.shorthand_assign.or(element.shorthand_assign);
		self.trailing_comma = self.trailing_comma.or(element.trailing_comma);
		self.parenthesized_assign = self.parenthesized_assign.or(element.parenthesized_assign);
		self.double_proto = self.double_proto.or(element.double_proto);
		self.operand = self.operand.or(element.operand);
	}

	pub(super) fn settle_as_operand(&mut self) {
		if self.operand.is_none() && self.shorthand_assign.is_none() && self.double_proto.is_none() {
			*self = Facts::default();
			return;
		}
		*self = Facts {
			operand: first_pending([
				self.operand,
				self.shorthand_assign.map(|pos| (pos, Code::ShorthandAssignment)),
				self.double_proto.map(|pos| (pos, Code::DuplicateProto)),
			]),
			..Facts::default()
		};
	}

	/// Whether an error of an expression is pending.
	#[inline(always)]
	pub(super) fn pending(&self) -> bool {
		self.shorthand_assign.is_some() || self.double_proto.is_some() || self.operand.is_some()
	}
}

/// What the checks keep between the files of a run: their buffers.
#[derive(Default)]
pub(crate) struct Checks {
	pub(super) scopes: Vec<Scope>,
	/// The names of each open scope, by its depth: a list past the open scopes is empty.
	names: Vec<Names>,
	pub(super) labels: Vec<Label>,
	pub(super) private_names: Vec<PrivateScope>,
	pub(super) param_names: Vec<StrId>,
	pub(super) undeclared_exports: Vec<(StrId, u32)>,
	pub(super) exports: Names,
	/// How many marks were taken: a name added after one holds its count.
	serial: u32,
}

impl Checks {
	#[inline(always)]
	fn here(&mut self) -> &mut Names {
		let open = self.scopes.len();
		&mut self.names[open - 1]
	}

	/// A mark for a rewind: the names added from here on are the ones `unwind` forgets.
	pub(super) fn mark_names(&mut self) -> u32 {
		self.serial += 1;
		self.serial
	}

	/// The scopes as they stood when the mark `names` was taken.
	pub(super) fn unwind(&mut self, scopes: usize, names: u32) {
		while self.scopes.len() > scopes {
			self.scopes.pop();
			self.names[self.scopes.len()].clear();
		}
		let open = self.scopes.len();
		for list in &mut self.names[..open] {
			list.forget(names);
		}
	}

	pub(super) fn current_has_type(&self, name: StrId) -> bool {
		self.names[self.scopes.len() - 1].has(name, TYPE)
	}

	pub(super) fn declare_here_type(&mut self, name: StrId) {
		self.names[self.scopes.len() - 1].push(name, TYPE, self.serial);
	}

	pub(super) fn declare_here_export_only(&mut self, name: StrId) {
		self.names[self.scopes.len() - 1].push(name, EXPORT_ONLY, self.serial);
	}

	pub(super) fn declare_global(&mut self, name: StrId) {
		self.names[0].push(name, EXPORT_ONLY, self.serial);
	}

	pub(super) fn current_declared(&self) -> Vec<StrId> {
		self.names[self.scopes.len() - 1].list.iter().map(|n| n.name).collect()
	}

	pub(super) fn declares_only_types(&self, name: StrId) -> bool {
		let top = &self.names[0];
		top.has(name, TYPE | EXPORT_ONLY) && !top.has(name, VAR | LEXICAL | FUNCTION)
	}

	pub(super) fn clear(&mut self) {
		if self.scopes.is_empty() && self.labels.is_empty() && self.private_names.is_empty() {
			self.param_names.clear();
			self.undeclared_exports.clear();
			self.exports.clear();
			self.serial = 0;
			return;
		}
		let open = self.scopes.len();
		for list in &mut self.names[..open] {
			list.clear();
		}
		self.scopes.clear();
		self.labels.clear();
		self.private_names.clear();
		self.param_names.clear();
		self.undeclared_exports.clear();
		self.exports.clear();
		self.serial = 0;
	}
}

impl<'a, const TS: bool> Parser<'a, TS> {
	pub(super) fn enter_scope(&mut self, flags: u32) {
		let checks = &mut self.scratch.checks;
		if checks.names.len() == checks.scopes.len() {
			checks.names.push(Names::default());
		}
		checks.scopes.push(Scope { flags });
	}

	pub(super) fn exit_scope(&mut self) {
		let checks = &mut self.scratch.checks;
		if checks.scopes.pop().is_some() {
			checks.names[checks.scopes.len()].clear();
		}
	}

	fn current_var_flags(&self) -> u32 {
		self.scratch
			.checks
			.scopes
			.iter()
			.rev()
			.find(|s| s.flags & SCOPE_VAR_LIKE != 0)
			.map_or(SCOPE_TOP, |s| s.flags)
	}

	fn current_this_flags(&self) -> u32 {
		self.scratch
			.checks
			.scopes
			.iter()
			.rev()
			.find(|s| s.flags & SCOPE_VAR_LIKE != 0 && s.flags & SCOPE_ARROW == 0)
			.map_or(SCOPE_TOP, |s| s.flags)
	}

	fn treat_functions_as_var_in(&self, flags: u32) -> bool {
		flags & SCOPE_FUNCTION != 0 || (!self.module && flags & SCOPE_TOP != 0)
	}

	pub(super) fn treat_functions_as_var(&self) -> bool {
		let flags = self.scratch.checks.scopes.last().map_or(SCOPE_TOP, |s| s.flags);
		self.treat_functions_as_var_in(flags)
	}

	/// Whether the nearest scope that holds `var`s is the top one.
	pub(super) fn at_top_scope(&self) -> bool {
		self.current_var_flags() & SCOPE_TOP != 0
	}

	pub(super) fn in_function(&self) -> bool {
		self.current_var_flags() & SCOPE_FUNCTION != 0
	}

	pub(super) fn in_generator(&self) -> bool {
		self.current_var_flags() & SCOPE_GENERATOR != 0
	}

	pub(super) fn in_async(&self) -> bool {
		self.current_var_flags() & SCOPE_ASYNC != 0
	}

	pub(super) fn can_await_here(&self) -> bool {
		for scope in self.scratch.checks.scopes.iter().rev() {
			if scope.flags & (SCOPE_CLASS_STATIC_BLOCK | SCOPE_CLASS_FIELD_INIT) != 0 {
				return false;
			}
			if scope.flags & SCOPE_FUNCTION != 0 {
				return scope.flags & SCOPE_ASYNC != 0;
			}
		}
		self.module || self.options.has(crate::parser::Options::ALLOW_AWAIT_OUTSIDE_FUNCTION)
	}

	pub(super) fn allow_super(&self) -> bool {
		self.current_this_flags() & SCOPE_SUPER != 0
			|| self.options.has(crate::parser::Options::ALLOW_SUPER_OUTSIDE_METHOD)
	}

	pub(super) fn allow_direct_super(&self) -> bool {
		self.current_this_flags() & SCOPE_DIRECT_SUPER != 0
	}

	pub(super) fn allow_new_target(&self) -> bool {
		for scope in self.scratch.checks.scopes.iter().rev() {
			let flags = scope.flags;
			if flags & (SCOPE_CLASS_STATIC_BLOCK | SCOPE_CLASS_FIELD_INIT) != 0
				|| (flags & SCOPE_FUNCTION != 0 && flags & SCOPE_ARROW == 0)
			{
				return true;
			}
		}
		false
	}

	fn in_class_static_block(&self) -> bool {
		self.current_var_flags() & SCOPE_CLASS_STATIC_BLOCK != 0
	}

	fn in_class_field_init(&self) -> bool {
		self.current_this_flags() & SCOPE_VAR == 0
	}

	/// The name declared where the cursor stands in the scopes; twice in one is an error.
	pub(super) fn declare_name(&mut self, name: StrId, binding: Binding, pos: u32) {
		let mut redeclared = false;
		let serial = self.scratch.checks.serial;
		match binding {
			Binding::Lexical => {
				let names = self.scratch.checks.here();
				redeclared = names.has(name, LEXICAL | FUNCTION | VAR);
				names.push(name, LEXICAL, serial);
			}
			Binding::SimpleCatch => self.scratch.checks.here().push(name, LEXICAL, serial),
			Binding::Function => {
				let as_var = self.treat_functions_as_var();
				let names = self.scratch.checks.here();
				redeclared = names.has(name, LEXICAL | if as_var { 0 } else { VAR });
				names.push(name, FUNCTION, serial);
			}
			_ => {
				for i in (0..self.scratch.checks.scopes.len()).rev() {
					let flags = self.scratch.checks.scopes[i].flags;
					let as_var = self.treat_functions_as_var_in(flags);
					let names = &mut self.scratch.checks.names[i];
					// the parameter of a simple catch clause, its first name, may be redeclared by var
					let catch_param = flags & SCOPE_SIMPLE_CATCH != 0 && names.first() == Some(name);
					if (!catch_param && names.has(name, LEXICAL)) || (!as_var && names.has(name, FUNCTION)) {
						redeclared = true;
						break;
					}
					names.push(name, VAR, serial);
					if flags & SCOPE_VAR != 0 {
						break;
					}
				}
			}
		}
		if redeclared {
			self.raise_name(pos, Code::Redeclaration, name);
		}
	}

	pub(super) fn rebinds_catch_param(&self, name: StrId) -> bool {
		let checks = &self.scratch.checks;
		for (scope, names) in checks.scopes.iter().zip(&checks.names).rev() {
			if scope.flags & SCOPE_SIMPLE_CATCH != 0 && names.first() == Some(name) {
				return true;
			}
			if scope.flags & SCOPE_VAR != 0 {
				return false;
			}
		}
		false
	}

	pub(super) fn declares_export(&self, name: StrId) -> bool {
		if TS && self.exports_in_script() {
			return true;
		}
		self.scratch.checks.names[0].has(name, VAR | LEXICAL | FUNCTION | TYPE | EXPORT_ONLY)
	}

	/// Whether the name written at `start` may be one of the words the checks know: by its
	/// first byte.
	pub(super) fn special_at(&self, start: u32) -> bool {
		const STARTS: u32 = 1 << (b'a' - b'a')
			| 1 << (b'e' - b'a')
			| 1 << (b'i' - b'a')
			| 1 << (b'l' - b'a')
			| 1 << (b'p' - b'a')
			| 1 << (b's' - b'a')
			| 1 << (b'y' - b'a');
		// a name of no width, standing for one under recovery, may stand where the text ends
		let b = self.src.as_bytes().get(start as usize).copied().unwrap_or(0);
		let first = b.wrapping_sub(b'a');
		b == b'\\' || (first < 26 && STARTS >> first & 1 != 0)
	}

	pub(super) fn word_flags(&self, name: StrId) -> u8 {
		word::flags(self.ast.strings.get(name))
	}

	pub(super) fn is_reserved_word(&self, flags: u8) -> bool {
		flags & word::ENUM != 0 || (flags & word::AWAIT != 0 && self.module) || (flags & word::STRICT != 0 && self.strict)
	}

	/// A name the grammar read where an identifier stands: not one of the words it may not be.
	#[cold]
	#[inline(never)]
	pub(super) fn check_unreserved(&mut self, id: NodeId) {
		let node = *self.ast.node(id);
		let NodeKind::Identifier { name } = node.kind else {
			return;
		};
		let flags = self.word_flags(name);
		if flags == 0 {
			return;
		}
		let start = node.start;
		if flags & word::YIELD != 0 && self.in_generator() {
			self.raise(start, Code::YieldAsIdentifier);
		} else if flags & word::AWAIT != 0 && self.in_async() {
			self.raise(start, Code::AwaitAsIdentifier);
		} else if flags & word::ARGUMENTS != 0 && self.in_class_field_init() {
			self.raise(start, Code::ArgumentsInFieldInitializer);
		} else if flags & (word::ARGUMENTS | word::AWAIT) != 0 && self.in_class_static_block() {
			self.raise_name(start, Code::InvalidInStaticBlock, name);
		} else if flags & word::KEYWORD != 0 {
			self.raise_name(start, Code::UnexpectedKeyword, name);
		} else if self.is_reserved_word(flags) {
			if !self.in_async() && flags & word::AWAIT != 0 {
				self.raise(start, Code::AwaitOutsideAsync);
			} else {
				self.raise_name(start, Code::ReservedWord, name);
			}
		}
		if flags & word::AWAIT != 0 && self.await_ident_pos == 0 {
			self.await_ident_pos = start;
		}
	}

	pub(super) fn ident_is(&self, id: NodeId, word: &str) -> bool {
		matches!(self.ast.node(id).kind, NodeKind::Identifier { name } if self.ast.strings.is(name, word))
	}

	/// What was read as an expression is a pattern: the same nodes, their kinds changed.
	pub(super) fn make_pattern(&mut self, id: NodeId, facts: Option<Facts>) -> NodeId {
		if self.error {
			return id;
		}
		let node = *self.ast.node(id);
		let start = node.start;
		match node.kind {
			NodeKind::Identifier { name } => {
				if self.in_async() && self.ast.strings.is(name, "await") {
					self.raise(start, Code::AwaitAsIdentifier);
				}
			}
			NodeKind::ObjectPattern { .. }
			| NodeKind::ArrayPattern { .. }
			| NodeKind::AssignmentPattern { .. }
			| NodeKind::RestElement { .. } => {}
			NodeKind::ObjectExpression { properties } => {
				self.ast.node_mut(id).kind = NodeKind::ObjectPattern { properties };
				if !self.check_pattern_errors(facts) {
					return id;
				}
				for i in 0..properties.len {
					let prop = self.ast.nth(properties, i).unwrap();
					self.make_pattern(prop, None);
					if self.error {
						return id;
					}
					if let NodeKind::RestElement { argument } = self.ast.node(prop).kind
						&& matches!(
							self.ast.node(argument).kind,
							NodeKind::ArrayPattern { .. } | NodeKind::ObjectPattern { .. }
						) {
						let at = self.ast.node(argument).start;
						self.raise(at, Code::UnexpectedToken);
						return id;
					}
				}
			}
			NodeKind::Property { key, value, kind, .. } => {
				if kind != PropertyKind::Init {
					let at = self.ast.node(key).start;
					self.raise(at, Code::AccessorInPattern);
					return id;
				}
				let pattern = self.make_pattern(value, None);
				if pattern != value
					&& !self.error
					&& let NodeKind::Property { value, .. } = &mut self.ast.node_mut(id).kind
				{
					*value = pattern;
				}
			}
			NodeKind::ArrayExpression { elements } => {
				self.ast.node_mut(id).kind = NodeKind::ArrayPattern { elements };
				if !self.check_pattern_errors(facts) {
					return id;
				}
				for i in 0..elements.len {
					if let Some(element) = self.ast.nth(elements, i) {
						self.make_pattern(element, None);
						if self.error {
							return id;
						}
					}
				}
			}
			NodeKind::SpreadElement { argument } => {
				let argument = self.make_pattern(argument, None);
				if self.error {
					return id;
				}
				self.ast.node_mut(id).kind = NodeKind::RestElement { argument };
				if matches!(self.ast.node(argument).kind, NodeKind::AssignmentPattern { .. }) {
					let at = self.ast.node(argument).start;
					self.raise(at, Code::RestWithDefault);
				}
			}
			NodeKind::AssignmentExpression { operator, left, right } => {
				if operator != AssignmentOperator::Assign {
					let at = self.ast.node(left).end;
					self.raise(at, Code::InvalidDefaultOperator);
					return id;
				}
				let left = self.make_pattern(left, None);
				if self.error {
					return id;
				}
				self.ast.node_mut(id).kind = NodeKind::AssignmentPattern { left, right };
			}
			NodeKind::ChainExpression { .. } => {
				self.raise(start, Code::OptionalChainAssignment);
			}
			NodeKind::MemberExpression { .. } => {}
			_ => {
				if TS && let Some(inner) = self.ts_wrapped(id) {
					return self.make_pattern(inner, facts);
				}
				self.raise(start, Code::InvalidAssignmentTarget);
			}
		}
		id
	}

	/// The facts of a literal that is a pattern after all; whether it may be one.
	fn check_pattern_errors(&mut self, facts: Option<Facts>) -> bool {
		let Some(facts) = facts else { return true };
		if facts.operand.is_none() && facts.trailing_comma.is_none() && facts.parenthesized_assign.is_none() {
			return true;
		}
		let (pos, code) = first_pending([
			facts.operand,
			facts.trailing_comma.map(|pos| (pos, Code::CommaAfterRest)),
			facts.parenthesized_assign.map(|pos| (pos, Code::InvalidAssignmentTarget)),
		])
		.unwrap();
		self.raise(pos, code);
		false
	}

	/// The pending errors of an expression that is no pattern; whether there were any.
	#[cold]
	#[inline(never)]
	pub(super) fn raise_expression_errors(&mut self, facts: &Facts) -> bool {
		match first_pending([
			facts.operand,
			facts.shorthand_assign.map(|pos| (pos, Code::ShorthandAssignment)),
			facts.double_proto.map(|pos| (pos, Code::DuplicateProto)),
		]) {
			Some((pos, code)) => {
				self.raise(pos, code);
				true
			}
			None => false,
		}
	}

	pub(super) fn check_lval_simple(&mut self, id: NodeId, binding: Binding, clashes: bool) {
		if self.error {
			return;
		}
		let is_bind = binding != Binding::None;
		let node = *self.ast.node(id);
		let start = node.start;
		match node.kind {
			NodeKind::Identifier { name } => {
				// the words that matter here start with one of a few letters, or with an escape
				if self.special_at(start) {
					let flags = self.word_flags(name);
					if self.strict
						&& (self.is_reserved_word(flags)
							|| (flags & (word::EVAL | word::ARGUMENTS) != 0 && !(TS && self.in_ambient())))
					{
						self.raise_name(start, Code::StrictBinding, name);
						return;
					}
					if binding == Binding::Lexical && self.ast.strings.is(name, "let") {
						self.raise(start, Code::LetAsBinding);
						return;
					}
				}
				if is_bind {
					if clashes {
						if self.scratch.checks.param_names.contains(&name) {
							self.raise(start, Code::DuplicateParameter);
							return;
						}
						self.scratch.checks.param_names.push(name);
					}
					if binding != Binding::Outside {
						self.declare_name(name, binding, start);
					}
				}
			}
			NodeKind::ChainExpression { .. } => {
				self.raise(start, Code::OptionalChainAssignment);
			}
			NodeKind::MemberExpression { .. } => {
				if is_bind {
					self.raise(start, Code::BindingMemberExpression);
				}
			}
			_ => {
				if TS && let Some(inner) = self.unwrap_simple(id) {
					return self.check_lval_simple(inner, binding, clashes);
				}
				self.raise(
					start,
					if is_bind {
						Code::InvalidBindingTarget
					} else {
						Code::InvalidAssignmentTarget
					},
				);
			}
		}
	}

	pub(super) fn check_lval_pattern(&mut self, id: NodeId, binding: Binding, clashes: bool) {
		if self.error {
			return;
		}
		match self.ast.node(id).kind {
			NodeKind::ObjectPattern { properties } => {
				for i in 0..properties.len {
					let prop = self.ast.nth(properties, i).unwrap();
					self.check_lval_inner_pattern(prop, binding, clashes);
				}
			}
			NodeKind::ArrayPattern { elements } => {
				for i in 0..elements.len {
					if let Some(element) = self.ast.nth(elements, i) {
						self.check_lval_inner_pattern(element, binding, clashes);
					}
				}
			}
			_ => self.check_lval_simple(id, binding, clashes),
		}
	}

	pub(super) fn check_lval_inner_pattern(&mut self, id: NodeId, binding: Binding, clashes: bool) {
		if self.error {
			return;
		}
		match self.ast.node(id).kind {
			NodeKind::Property { value, .. } => self.check_lval_inner_pattern(value, binding, clashes),
			NodeKind::AssignmentPattern { left, .. } => self.check_lval_pattern(left, binding, clashes),
			NodeKind::RestElement { argument } => self.check_lval_pattern(argument, binding, clashes),
			_ => {
				if TS && let Some(inner) = self.unwrap_inner_pattern(id) {
					return self.check_lval_inner_pattern(inner, binding, clashes);
				}
				self.check_lval_pattern(id, binding, clashes)
			}
		}
	}

	pub(super) fn is_simple_param_list(&self, params: List) -> bool {
		(0..params.len).all(|i| matches!(self.ast.node(self.ast.nth(params, i).unwrap()).kind, NodeKind::Identifier { .. }))
	}

	pub(super) fn check_params(&mut self, params: List, allow_duplicates: bool) {
		self.scratch.checks.param_names.clear();
		for i in 0..params.len {
			let param = self.ast.nth(params, i).unwrap();
			self.check_lval_inner_pattern(param, Binding::Var, !allow_duplicates);
		}
		self.scratch.checks.param_names.clear();
	}

	pub(super) fn is_simple_assign_target(&self, id: NodeId) -> bool {
		matches!(
			self.ast.node(id).kind,
			NodeKind::Identifier { .. } | NodeKind::MemberExpression { .. }
		)
	}

	pub(super) fn check_yield_await_in_default_params(&mut self) {
		if self.yield_pos != 0 && (self.await_pos == 0 || self.yield_pos < self.await_pos) {
			self.raise(self.yield_pos, Code::YieldInDefaultValue);
		} else if self.await_pos != 0 {
			self.raise(self.await_pos, Code::AwaitInDefaultValue);
		}
	}
}
