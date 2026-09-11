use super::{Extension, Parser, Result};
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
pub(crate) const SCOPE_VAR: u32 = SCOPE_TOP | SCOPE_FUNCTION | SCOPE_CLASS_STATIC_BLOCK;
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

pub(crate) struct Scope {
	pub flags: u32,
	/// The names declared here, each with how, in declaration order.
	names: Vec<(StrId, u8)>,
	/// The same by name, once the list is long enough that a scan costs more than a lookup.
	index: Option<FastMap<StrId, u8>>,
}

const INDEXED: usize = 64;

impl Scope {
	fn has(&self, name: StrId, kinds: u8) -> bool {
		match &self.index {
			Some(index) => index.get(&name).is_some_and(|kind| kind & kinds != 0),
			None => self.names.iter().any(|&(n, kind)| n == name && kind & kinds != 0),
		}
	}

	fn push(&mut self, name: StrId, kind: u8) {
		self.names.push((name, kind));
		match &mut self.index {
			Some(index) => *index.entry(name).or_default() |= kind,
			None if self.names.len() == INDEXED => {
				let mut index = FastMap::default();
				for &(n, k) in &self.names {
					*index.entry(n).or_default() |= k;
				}
				self.index = Some(index);
			}
			None => {}
		}
	}
}

impl<E: Extension> Parser<'_, E> {
	pub(crate) fn enter_scope(&mut self, flags: u32) {
		let names = self.spare_names.pop().unwrap_or_default();
		self.scopes.push(Scope {
			flags,
			names,
			index: None,
		});
	}

	pub(crate) fn exit_scope(&mut self) {
		E::scope_exit(self);
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
		scope.flags & SCOPE_FUNCTION != 0 || (!self.options.module && scope.flags & SCOPE_TOP != 0)
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
		self.options.module || self.options.allow_await_outside_function
	}

	pub(crate) fn allow_super(&self) -> bool {
		self.current_this_scope().flags & SCOPE_SUPER != 0 || self.options.allow_super_outside_method
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
		match binding {
			Binding::Lexical => {
				let scope = self.current_scope_mut();
				redeclared = scope.has(name, LEXICAL | FUNCTION | VAR);
				scope.push(name, LEXICAL);
				if self.current_scope().flags & SCOPE_TOP != 0 {
					self.undeclared_exports.remove(&name);
				}
			}
			Binding::SimpleCatch => self.current_scope_mut().push(name, LEXICAL),
			Binding::Function => {
				let as_var = self.treat_functions_as_var();
				let scope = self.current_scope_mut();
				redeclared = scope.has(name, LEXICAL | if as_var { 0 } else { VAR });
				scope.push(name, FUNCTION);
			}
			_ => {
				for i in (0..self.scopes.len()).rev() {
					let as_var = self.treat_functions_as_var_in(&self.scopes[i]);
					let scope = &mut self.scopes[i];
					// the parameter of a simple catch clause, its first name, may be redeclared by var
					let catch_param =
						scope.flags & SCOPE_SIMPLE_CATCH != 0 && scope.names.first().map(|n| n.0) == Some(name);
					if (!catch_param && scope.has(name, LEXICAL)) || (!as_var && scope.has(name, FUNCTION)) {
						redeclared = true;
						break;
					}
					scope.push(name, VAR);
					let top = scope.flags & SCOPE_TOP != 0;
					let stop = scope.flags & SCOPE_VAR != 0;
					if top {
						self.undeclared_exports.remove(&name);
					}
					if stop {
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
			if scope.flags & SCOPE_SIMPLE_CATCH != 0 && scope.names.first().map(|n| n.0) == Some(name) {
				return true;
			}
			if scope.flags & SCOPE_VAR != 0 {
				return false;
			}
		}
		false
	}

	pub(crate) fn check_local_export(&mut self, name: StrId, pos: u32) {
		if E::declares_export(self, name) {
			return;
		}
		if !self.scopes[0].has(name, VAR | LEXICAL | FUNCTION) {
			let order = self.undeclared_exports.len();
			self.undeclared_exports
				.entry(name)
				.and_modify(|e| e.0 = pos)
				.or_insert((pos, order));
		}
	}
}
