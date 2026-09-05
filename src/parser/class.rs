use super::expression::ForInit;
use super::pattern::Binding;
use super::scope::{SCOPE_CLASS_FIELD_INIT, SCOPE_CLASS_STATIC_BLOCK, SCOPE_SUPER};
use super::statement::Context;
use super::{Extension, Parser, PrivateKind, PrivateNameScope, Result};
use crate::ast::{Class, MethodKind, NodeId, NodeKind};
use crate::interner::StrId;
use crate::lexer::token::{Keyword, TokenKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassKind {
	Expression,
	Declaration,
	NullableId,
}

impl<E: Extension> Parser<'_, E> {
	pub(crate) fn parse_class(&mut self, kind: ClassKind) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		E::class_start(self)?;
		let old_strict = self.strict;
		self.set_strict(true);
		let id = if matches!(self.tok.kind, TokenKind::Ident(_))
			&& !(kind == ClassKind::Expression && E::starts_class_heritage(self))
		{
			let id = self.parse_ident(false)?;
			if kind != ClassKind::Expression {
				self.check_lval_simple(id, Binding::Lexical, &mut None)?;
			}
			Some(id)
		} else {
			if kind == ClassKind::Declaration {
				return self.unexpected();
			}
			None
		};
		E::class_type_parameters(self)?;
		let super_class = if self.eat_keyword(Keyword::Extends)? {
			Some(self.parse_expr_subscripts(&mut None, ForInit::No)?)
		} else {
			None
		};
		E::class_heritage(self, super_class.is_some())?;
		self.private_names.push(PrivateNameScope::default());
		let body_start = self.tok.start;
		let mut body = Vec::new();
		let mut had_constructor = false;
		self.expect(TokenKind::BraceL)?;
		while !self.is(TokenKind::BraceR) {
			let Some(element) = self.parse_class_element(super_class.is_some())? else {
				continue;
			};
			body.push(element);
			match self.kind(element) {
				NodeKind::MethodDefinition {
					kind: MethodKind::Constructor,
					value,
					..
				} if matches!(self.kind(value), NodeKind::FunctionExpression { .. }) => {
					if had_constructor {
						return self.error(self.start_of(element), "Duplicate constructor in the same class");
					}
					had_constructor = true;
				}
				NodeKind::MethodDefinition {
					key, is_static, value, ..
				} if matches!(self.kind(key), NodeKind::PrivateIdentifier { .. })
					&& !matches!(self.kind(value), NodeKind::Extension(_)) =>
				{
					self.declare_private_element(element, key, is_static)?;
				}
				NodeKind::PropertyDefinition { key, is_static, .. }
					if matches!(self.kind(key), NodeKind::PrivateIdentifier { .. }) =>
				{
					self.declare_private_element(element, key, is_static)?;
				}
				_ => {}
			}
		}
		self.set_strict(old_strict);
		self.next()?;
		let body = self.list_of(&body);
		let body = self.add(NodeKind::ClassBody { body }, body_start);
		self.exit_class_body()?;
		let class = Class { id, super_class, body };
		let kind = if kind == ClassKind::Expression {
			NodeKind::ClassExpression { class }
		} else {
			NodeKind::ClassDeclaration { class }
		};
		let node = self.add(kind, start);
		E::class_end(self, node);
		Ok(node)
	}

	fn declare_private_element(&mut self, element: NodeId, key: NodeId, is_static: bool) -> Result<()> {
		let NodeKind::PrivateIdentifier { name } = self.kind(key) else {
			unreachable!()
		};
		let private_kind = match (self.kind(element), is_static) {
			(
				NodeKind::MethodDefinition {
					kind: MethodKind::Get, ..
				},
				false,
			) => PrivateKind::InstanceGet,
			(
				NodeKind::MethodDefinition {
					kind: MethodKind::Set, ..
				},
				false,
			) => PrivateKind::InstanceSet,
			(
				NodeKind::MethodDefinition {
					kind: MethodKind::Get, ..
				},
				true,
			) => PrivateKind::StaticGet,
			(
				NodeKind::MethodDefinition {
					kind: MethodKind::Set, ..
				},
				true,
			) => PrivateKind::StaticSet,
			_ => PrivateKind::Any,
		};
		if self.declare_private_name(name, private_kind) {
			return self.error(
				self.start_of(key),
				format!("Identifier '#{}' has already been declared", self.str(name)),
			);
		}
		Ok(())
	}

	/// Returns whether the name conflicts with an earlier declaration.
	fn declare_private_name(&mut self, name: StrId, kind: PrivateKind) -> bool {
		let scope = self.private_names.last_mut().unwrap();
		let Some(entry) = scope.declared.iter_mut().find(|(n, _)| *n == name) else {
			scope.declared.push((name, kind));
			return false;
		};
		let pair = |a: PrivateKind, b: PrivateKind| (entry.1 == a && kind == b) || (entry.1 == b && kind == a);
		if pair(PrivateKind::InstanceGet, PrivateKind::InstanceSet)
			|| pair(PrivateKind::StaticGet, PrivateKind::StaticSet)
		{
			entry.1 = PrivateKind::Any;
			return false;
		}
		true
	}

	fn exit_class_body(&mut self) -> Result<()> {
		let scope = self.private_names.pop().unwrap();
		for (name, pos) in scope.used {
			if scope.declared.iter().any(|(n, _)| *n == name) {
				continue;
			}
			match self.private_names.last_mut() {
				Some(parent) => parent.used.push((name, pos)),
				None => {
					return self.error(
						pos,
						format!(
							"Private field '#{}' must be declared in an enclosing class",
							self.str(name)
						),
					);
				}
			}
		}
		Ok(())
	}

	fn parse_class_element(&mut self, constructor_allows_super: bool) -> Result<Option<NodeId>> {
		if self.eat(TokenKind::Semi)? {
			return Ok(None);
		}
		let start = self.tok.start;
		let mut key_name: Option<&'static str> = None;
		let mut generator = false;
		let mut is_async = false;
		let mut kind = MethodKind::Method;
		let mut is_static = E::class_modifiers(self)?;
		if let Some(signature) = E::class_index_signature(self, start)? {
			E::class_element_end(self, signature)?;
			return Ok(Some(signature));
		}
		if !is_static
			&& self.is_contextual("static")
			&& (!E::STATIC_IS_A_MODIFIER || self.peek_char().0 == Some('{'))
			&& self.eat_contextual("static")?
		{
			if self.eat(TokenKind::BraceL)? {
				let block = self.parse_class_static_block(start)?;
				E::class_element_end(self, block)?;
				return Ok(Some(block));
			}
			if self.is_class_element_name_start() || self.is(TokenKind::Star) {
				is_static = true;
			} else {
				key_name = Some("static");
			}
		}
		if key_name.is_none() && self.eat_contextual("async")? {
			if (self.is_class_element_name_start() || self.is(TokenKind::Star)) && !self.can_insert_semicolon() {
				is_async = true;
			} else {
				key_name = Some("async");
			}
		}
		if key_name.is_none() && self.eat(TokenKind::Star)? {
			generator = true;
		}
		if key_name.is_none() && !is_async && !generator {
			let accessor = if self.is_contextual("get") {
				Some(("get", MethodKind::Get))
			} else if self.is_contextual("set") {
				Some(("set", MethodKind::Set))
			} else {
				None
			};
			if let Some((name, accessor_kind)) = accessor {
				self.next()?;
				if self.is_class_element_name_start() {
					kind = accessor_kind;
				} else {
					key_name = Some(name);
				}
			}
		}
		let (key, computed) = match key_name {
			Some(name) => {
				let key_start = self.prev_end - name.len() as u32;
				let name = self.intern(name);
				(
					self.add_with_end(NodeKind::Identifier { name }, key_start, self.prev_end),
					false,
				)
			}
			None => self.parse_class_element_name()?,
		};
		E::class_key_end(self, key, computed)?;
		if self.is(TokenKind::ParenL)
			|| E::starts_class_method(self)
			|| kind != MethodKind::Method
			|| generator
			|| is_async
		{
			let is_constructor = !is_static && self.check_key_name(key, computed, "constructor");
			let allows_direct_super = is_constructor && constructor_allows_super;
			if is_constructor && kind != MethodKind::Method {
				return self.error(self.start_of(key), "Constructor can't have get/set modifier");
			}
			if is_constructor {
				kind = MethodKind::Constructor;
			}
			let method = self.parse_class_method(
				start,
				key,
				computed,
				kind,
				is_static,
				generator,
				is_async,
				allows_direct_super,
			)?;
			return Ok(Some(method));
		}
		Ok(Some(self.parse_class_field(start, key, computed, is_static)?))
	}

	fn is_class_element_name_start(&self) -> bool {
		matches!(
			self.tok.kind,
			TokenKind::Ident(_)
				| TokenKind::PrivateName(_)
				| TokenKind::Number(_)
				| TokenKind::BigInt
				| TokenKind::String(_)
				| TokenKind::BracketL
				| TokenKind::Keyword(_)
		)
	}

	fn parse_class_element_name(&mut self) -> Result<(NodeId, bool)> {
		if let TokenKind::PrivateName(name) = self.tok.kind {
			if self.str(name) == "constructor" {
				return self.error(self.tok.start, "Classes can't have an element named '#constructor'");
			}
			return Ok((self.parse_private_ident()?, false));
		}
		self.parse_property_name()
	}

	fn check_key_name(&self, key: NodeId, computed: bool, name: &str) -> bool {
		if computed {
			return false;
		}
		match self.kind(key) {
			NodeKind::Identifier { name: n } | NodeKind::StringLiteral { value: n } => self.str(n) == name,
			_ => false,
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn parse_class_method(
		&mut self,
		start: u32,
		key: NodeId,
		computed: bool,
		kind: MethodKind,
		is_static: bool,
		generator: bool,
		is_async: bool,
		allows_direct_super: bool,
	) -> Result<NodeId> {
		if kind == MethodKind::Constructor {
			if generator {
				return self.error(self.start_of(key), "Constructor can't be a generator");
			}
			if is_async {
				return self.error(self.start_of(key), "Constructor can't be an async method");
			}
		} else if is_static && self.check_key_name(key, computed, "prototype") {
			return self.error(
				self.start_of(key),
				"Classes may not have a static property named prototype",
			);
		}
		E::class_method_start(self)?;
		let value = self.parse_method(generator, is_async, allows_direct_super, true)?;
		if kind == MethodKind::Get || kind == MethodKind::Set {
			self.check_accessor_params(value, kind == MethodKind::Get, true)?;
		}
		let node = self.add(
			NodeKind::MethodDefinition {
				key,
				value,
				kind,
				computed,
				is_static,
			},
			start,
		);
		E::class_element_end(self, node)?;
		Ok(node)
	}

	fn parse_class_field(&mut self, start: u32, key: NodeId, computed: bool, is_static: bool) -> Result<NodeId> {
		if self.check_key_name(key, computed, "constructor") {
			return self.error(self.start_of(key), "Classes can't have a field named 'constructor'");
		}
		if is_static && self.check_key_name(key, computed, "prototype") {
			return self.error(
				self.start_of(key),
				"Classes can't have a static field named 'prototype'",
			);
		}
		E::class_field_annotation(self)?;
		let value = if self.eat(TokenKind::Eq)? {
			self.enter_scope(SCOPE_CLASS_FIELD_INIT | SCOPE_SUPER);
			let value = self.parse_maybe_assign(ForInit::No, &mut None)?;
			self.exit_scope();
			Some(value)
		} else {
			None
		};
		self.semicolon()?;
		let node = self.add(
			NodeKind::PropertyDefinition {
				key,
				value,
				computed,
				is_static,
			},
			start,
		);
		E::class_element_end(self, node)?;
		Ok(node)
	}

	fn parse_class_static_block(&mut self, start: u32) -> Result<NodeId> {
		let old_labels = std::mem::take(&mut self.labels);
		self.enter_scope(SCOPE_CLASS_STATIC_BLOCK | SCOPE_SUPER);
		let mut body = Vec::new();
		while !self.is(TokenKind::BraceR) {
			body.push(self.parse_statement(Context::None, false, None)?);
		}
		self.next()?;
		self.exit_scope();
		self.labels = old_labels;
		let body = self.list_of(&body);
		Ok(self.add(NodeKind::StaticBlock { body }, start))
	}
}
