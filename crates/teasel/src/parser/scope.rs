use super::{Parser, Result};
use crate::Options;
use crate::error::Code;
use crate::interner::{FastMap, StrId};

pub(crate) const SCOPE_TOP: u32 = 1;
pub(crate) const SCOPE_FUNCTION: u32 = 2;
pub(crate) const SCOPE_ASYNC: u32 = 4;
pub(crate) const SCOPE_GENERATOR: u32 = 8;
pub(crate) const SCOPE_ARROW: u32 = 16;
pub(crate) const SCOPE_SIMPLE_CATCH: u32 = 32;
pub(crate) const SCOPE_SUPER: u32 = 64;
pub(crate) const SCOPE_DIRECT_SUPER: u32 = 128;
pub(crate) const SCOPE_CLASS_STATIC_BLOCK: u32 = 256;
pub(crate) const SCOPE_CLASS_FIELD_INIT: u32 = 512;
/// A TypeScript namespace or module body: `var` stops there.
pub(crate) const SCOPE_TS_MODULE: u32 = 1024;
pub(crate) const SCOPE_VAR: u32 = SCOPE_TOP | SCOPE_FUNCTION | SCOPE_CLASS_STATIC_BLOCK | SCOPE_TS_MODULE;
const SCOPE_VAR_LIKE: u32 = SCOPE_VAR | SCOPE_CLASS_FIELD_INIT;

pub(crate) fn function_flags(is_async: bool, generator: bool) -> u32 {
	SCOPE_FUNCTION | if is_async { SCOPE_ASYNC } else { 0 } | if generator { SCOPE_GENERATOR } else { 0 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Binding {
	None,
	Var,
	Lexical,
	Function,
	SimpleCatch,
	Outside,
}

/// How a name was declared in a scope, as bits so one scan answers a question about several.
const VAR: u8 = 1;
const LEXICAL: u8 = 2;
const FUNCTION: u8 = 4;
pub(crate) const TYPE: u8 = 8;
pub(crate) const EXPORT_ONLY: u8 = 16;

pub(crate) struct Scope {
	pub flags: u32,
	names: Vec<Name>,
	/// The same by name, once the list is long enough that a scan costs more than a lookup.
	index: Option<FastMap<StrId, u8>>,
}

#[derive(Clone, Copy)]
pub(crate) struct Name {
	name: StrId,
	kind: u8,
	/// What the index held for the name before.
	before: u8,
	serial: u32,
}

const INDEXED: usize = 64;

impl Scope {
	pub(crate) fn declared(&self) -> impl Iterator<Item = StrId> + '_ {
		self.names.iter().map(|n| n.name)
	}

	pub(crate) fn has(&self, name: StrId, kinds: u8) -> bool {
		match &self.index {
			Some(index) => index.get(&name).is_some_and(|kind| kind & kinds != 0),
			None => self.names.iter().any(|n| n.name == name && n.kind & kinds != 0),
		}
	}

	fn indexed(&self) -> FastMap<StrId, u8> {
		let mut index = FastMap::default();
		for n in &self.names {
			*index.entry(n.name).or_default() |= n.kind;
		}
		index
	}

	fn forget(&mut self, serial: u32) {
		let kept = self.names.iter().rposition(|n| n.serial < serial).map_or(0, |i| i + 1);
		match &mut self.index {
			// rebuilding the index instead was quadratic in a long scope's skipped statements
			Some(index) if kept >= INDEXED => {
				for n in self.names[kept..].iter().rev() {
					if n.before == 0 {
						index.remove(&n.name);
					} else {
						index.insert(n.name, n.before);
					}
				}
			}
			_ => self.index = None,
		}
		self.names.truncate(kept);
	}

	fn push(&mut self, name: StrId, kind: u8, serial: u32) {
		debug_assert!(self.names.last().is_none_or(|n| n.serial <= serial));
		let before = match &mut self.index {
			Some(index) => {
				let held = index.entry(name).or_default();
				std::mem::replace(held, *held | kind)
			}
			None => 0,
		};
		self.names.push(Name {
			name,
			kind,
			before,
			serial,
		});
		if self.index.is_none() && self.names.len() == INDEXED {
			self.index = Some(self.indexed());
		}
	}
}

impl<const TS: bool> Parser<'_, TS> {
	pub(crate) fn enter_scope(&mut self, flags: u32) {
		let names = self.spare_names.pop().unwrap_or_default();
		self.scopes.push(Scope {
			flags,
			names,
			index: None,
		});
	}

	pub(crate) fn exit_scope(&mut self) {
		let mut scope = self.scopes.pop().unwrap();
		scope.names.clear();
		self.spare_names.push(scope.names);
	}

	pub(crate) fn current_scope(&self) -> &Scope {
		self.scopes.last().unwrap()
	}

	fn current_scope_mut(&mut self) -> &mut Scope {
		self.scopes.last_mut().unwrap()
	}

	pub(crate) fn declare_here(&mut self, name: StrId, kind: u8) {
		let serial = self.serial();
		self.current_scope_mut().push(name, kind, serial);
	}

	pub(crate) fn declare_global(&mut self, name: StrId) {
		let serial = self.serial();
		self.scopes[0].push(name, EXPORT_ONLY, serial);
	}

	fn serial(&mut self) -> u32 {
		self.declarations += 1;
		self.declarations - 1
	}

	pub(crate) fn forget_names(&mut self, serial: u32) {
		// every open frame instead: each failed attempt cost the nesting depth, 2.4x at 900 nested blocks
		for scope in self.scopes.iter_mut().rev() {
			if scope.names.last().is_none_or(|n| n.serial < serial) {
				break;
			}
			scope.forget(serial);
		}
		if let Some(top) = self.scopes.first_mut()
			&& top.names.last().is_some_and(|n| n.serial >= serial)
		{
			top.forget(serial);
		}
		self.declarations = serial;
		debug_assert!(
			self.scopes
				.iter()
				.all(|scope| scope.names.last().is_none_or(|n| n.serial < serial))
		);
	}

	pub(crate) fn current_var_scope(&self) -> &Scope {
		self.scopes
			.iter()
			.rev()
			.find(|s| s.flags & SCOPE_VAR_LIKE != 0)
			.unwrap()
	}

	pub(crate) fn current_this_scope(&self) -> &Scope {
		self.scopes
			.iter()
			.rev()
			.find(|s| s.flags & SCOPE_VAR_LIKE != 0 && s.flags & SCOPE_ARROW == 0)
			.unwrap()
	}

	fn treat_functions_as_var_in(&self, scope: &Scope) -> bool {
		scope.flags & SCOPE_FUNCTION != 0 || (!self.options.has(Options::MODULE) && scope.flags & SCOPE_TOP != 0)
	}

	pub(crate) fn treat_functions_as_var(&self) -> bool {
		self.treat_functions_as_var_in(self.current_scope())
	}

	pub(crate) fn in_function(&self) -> bool {
		self.current_var_scope().flags & SCOPE_FUNCTION != 0
	}

	pub(crate) fn in_generator(&self) -> bool {
		self.current_var_scope().flags & SCOPE_GENERATOR != 0
	}

	pub(crate) fn in_async(&self) -> bool {
		self.current_var_scope().flags & SCOPE_ASYNC != 0
	}

	pub(crate) fn can_await(&self) -> bool {
		for scope in self.scopes.iter().rev() {
			if scope.flags & (SCOPE_CLASS_STATIC_BLOCK | SCOPE_CLASS_FIELD_INIT) != 0 {
				return false;
			}
			if scope.flags & SCOPE_FUNCTION != 0 {
				return scope.flags & SCOPE_ASYNC != 0;
			}
		}
		self.options.has(Options::MODULE) || self.options.has(Options::ALLOW_AWAIT_OUTSIDE_FUNCTION)
	}

	pub(crate) fn allow_super(&self) -> bool {
		self.current_this_scope().flags & SCOPE_SUPER != 0 || self.options.has(Options::ALLOW_SUPER_OUTSIDE_METHOD)
	}

	pub(crate) fn allow_direct_super(&self) -> bool {
		self.current_this_scope().flags & SCOPE_DIRECT_SUPER != 0
	}

	pub(crate) fn allow_new_target(&self) -> bool {
		for scope in self.scopes.iter().rev() {
			let flags = scope.flags;
			if flags & (SCOPE_CLASS_STATIC_BLOCK | SCOPE_CLASS_FIELD_INIT) != 0
				|| (flags & SCOPE_FUNCTION != 0 && flags & SCOPE_ARROW == 0)
			{
				return true;
			}
		}
		false
	}

	pub(crate) fn in_class_static_block(&self) -> bool {
		self.current_var_scope().flags & SCOPE_CLASS_STATIC_BLOCK != 0
	}

	pub(crate) fn in_class_field_init(&self) -> bool {
		self.current_this_scope().flags & SCOPE_VAR == 0
	}

	pub(crate) fn declare_name(&mut self, name: StrId, binding: Binding, pos: u32) -> Result<()> {
		let mut redeclared = false;
		let serial = self.serial();
		match binding {
			Binding::Lexical => {
				let scope = self.current_scope_mut();
				redeclared = scope.has(name, LEXICAL | FUNCTION | VAR);
				scope.push(name, LEXICAL, serial);
			}
			Binding::SimpleCatch => self.current_scope_mut().push(name, LEXICAL, serial),
			Binding::Function => {
				let as_var = self.treat_functions_as_var();
				let scope = self.current_scope_mut();
				redeclared = scope.has(name, LEXICAL | if as_var { 0 } else { VAR });
				scope.push(name, FUNCTION, serial);
			}
			_ => {
				for i in (0..self.scopes.len()).rev() {
					let as_var = self.treat_functions_as_var_in(&self.scopes[i]);
					let scope = &mut self.scopes[i];
					// the parameter of a simple catch clause, its first name, may be redeclared by var
					let catch_param =
						scope.flags & SCOPE_SIMPLE_CATCH != 0 && scope.names.first().map(|n| n.name) == Some(name);
					if (!catch_param && scope.has(name, LEXICAL)) || (!as_var && scope.has(name, FUNCTION)) {
						redeclared = true;
						break;
					}
					scope.push(name, VAR, serial);
					if scope.flags & SCOPE_VAR != 0 {
						break;
					}
				}
			}
		}
		if redeclared {
			return self.error_name(pos, Code::Redeclaration, name);
		}
		Ok(())
	}

	/// Whether a simple catch clause around the current scope, inside the nearest function, binds `name`.
	pub(crate) fn rebinds_catch_param(&self, name: StrId) -> bool {
		for scope in self.scopes.iter().rev() {
			if scope.flags & SCOPE_SIMPLE_CATCH != 0 && scope.names.first().map(|n| n.name) == Some(name) {
				return true;
			}
			if scope.flags & SCOPE_VAR != 0 {
				return false;
			}
		}
		false
	}

	pub(crate) fn declares_export(&self, name: StrId) -> bool {
		if self.typescript() && self.exports_in_script() {
			return true;
		}
		debug_assert_eq!(self.scopes.len(), 1);
		self.scopes[0].has(name, VAR | LEXICAL | FUNCTION | TYPE | EXPORT_ONLY)
	}

	pub(crate) fn check_local_export(&mut self, name: StrId, pos: u32) {
		if !self.declares_export(name) {
			self.undeclared_exports.push((name, pos));
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn forgetting_leaves_the_index_a_rebuild_would() {
		let mut scope = Scope {
			flags: 0,
			names: Vec::new(),
			index: None,
		};
		let mut seed = 7u32;
		let mut next = |n: usize| {
			seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
			(seed >> 8) as usize % n
		};
		let check = |scope: &Scope| {
			assert_eq!(scope.index, (scope.names.len() >= INDEXED).then(|| scope.indexed()));
		};
		let mut serial = 0;
		for _ in 0..2000 {
			// around the length the index starts at, names declared again with other kinds
			while scope.names.len() < INDEXED - 4 + next(16) {
				serial += 1 + next(2) as u32;
				scope.push(StrId::at(next(40) as u32), [VAR, LEXICAL, FUNCTION][next(3)], serial);
			}
			let skipped = serial + 1;
			for _ in 0..next(6) {
				serial += 1 + next(2) as u32;
				scope.push(StrId::at(next(40) as u32), [VAR, LEXICAL, FUNCTION][next(3)], serial);
			}
			check(&scope);
			scope.forget(skipped);
			check(&scope);
			if next(8) == 0 {
				scope.forget(scope.names[next(scope.names.len())].serial);
				check(&scope);
			}
		}
	}
}
