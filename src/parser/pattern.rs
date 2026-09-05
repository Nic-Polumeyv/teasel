use super::expression::ForInit;
use super::{DestructuringErrors, Extension, Parser, Result, Unwrap};
use crate::ast::{AssignmentOperator, NodeId, NodeKind, PropertyKind};
use crate::interner::StrId;
use crate::lexer::token::TokenKind;

pub(crate) use super::scope::Binding;

impl<E: Extension> Parser<'_, E> {
	/// Reinterprets an expression as an assignment or binding pattern, in place.
	pub(crate) fn make_pattern(
		&mut self,
		id: NodeId,
		is_binding: bool,
		errors: &mut Option<DestructuringErrors>,
	) -> Result<NodeId> {
		let start = self.start_of(id);
		match self.kind(id) {
			NodeKind::Identifier { name } => {
				if self.in_async() && self.str(name) == "await" {
					return self.error(start, "Cannot use 'await' as identifier inside an async function");
				}
			}
			NodeKind::ObjectPattern { .. }
			| NodeKind::ArrayPattern { .. }
			| NodeKind::AssignmentPattern { .. }
			| NodeKind::RestElement { .. } => {}
			NodeKind::ObjectExpression { properties } => {
				self.ast.node_mut(id).kind = NodeKind::ObjectPattern { properties };
				self.check_pattern_errors(errors, true)?;
				for i in 0..properties.len {
					let prop = self.ast.lists[(properties.start + i) as usize].unwrap();
					self.make_pattern(prop, is_binding, &mut None)?;
					if let NodeKind::RestElement { argument } = self.kind(prop)
						&& matches!(
							self.kind(argument),
							NodeKind::ArrayPattern { .. } | NodeKind::ObjectPattern { .. }
						) {
						return self.unexpected_at(self.start_of(argument));
					}
				}
			}
			NodeKind::Property { key, value, kind, .. } => {
				if kind != PropertyKind::Init {
					return self.error(self.start_of(key), "Object pattern can't contain getter or setter");
				}
				let pattern = self.make_pattern(value, is_binding, &mut None)?;
				if pattern != value
					&& let NodeKind::Property { value, .. } = &mut self.ast.node_mut(id).kind
				{
					*value = pattern;
				}
			}
			NodeKind::ArrayExpression { elements } => {
				self.ast.node_mut(id).kind = NodeKind::ArrayPattern { elements };
				self.check_pattern_errors(errors, true)?;
				let mut items = self.ast.list(elements).to_vec();
				E::convert_items(self, &mut items);
				self.ast.lists[elements.start as usize..(elements.start + elements.len) as usize]
					.copy_from_slice(&items);
				for element in items.into_iter().flatten() {
					self.make_pattern(element, is_binding, &mut None)?;
				}
			}
			NodeKind::SpreadElement { argument } => {
				let argument = self.make_pattern(argument, is_binding, &mut None)?;
				self.ast.node_mut(id).kind = NodeKind::RestElement { argument };
				if matches!(self.kind(argument), NodeKind::AssignmentPattern { .. }) {
					return self.error(self.start_of(argument), "Rest elements cannot have a default value");
				}
			}
			NodeKind::AssignmentExpression { operator, left, right } => {
				if operator != AssignmentOperator::Assign {
					return self.error(
						self.end_of(left),
						"Only '=' operator can be used for specifying default value.",
					);
				}
				let left = self.make_pattern(left, is_binding, &mut None)?;
				self.ast.node_mut(id).kind = NodeKind::AssignmentPattern { left, right };
			}
			NodeKind::ParenthesizedExpression { expression } => {
				let pattern = self.make_pattern(expression, is_binding, errors)?;
				return Ok(E::parenthesized_pattern(self, id, expression, pattern));
			}
			NodeKind::ChainExpression { .. } => {
				return self.error(start, "Optional chaining cannot appear in left-hand side");
			}
			NodeKind::MemberExpression { .. } if !is_binding => {}
			NodeKind::Extension(_) => {
				return match E::make_pattern(self, id, is_binding, errors)? {
					Some(pattern) => Ok(pattern),
					None => self.error(start, "Assigning to rvalue"),
				};
			}
			_ => return self.error(start, "Assigning to rvalue"),
		}
		Ok(id)
	}

	pub(crate) fn make_patterns(
		&mut self,
		mut items: Vec<Option<NodeId>>,
		is_binding: bool,
	) -> Result<Vec<Option<NodeId>>> {
		E::convert_items(self, &mut items);
		for item in items.iter().flatten() {
			self.make_pattern(*item, is_binding, &mut None)?;
		}
		Ok(items)
	}

	pub(crate) fn parse_binding_atom(&mut self) -> Result<NodeId> {
		self.enter()?;
		let result = self.parse_binding_atom_inner();
		self.leave();
		result
	}

	fn parse_binding_atom_inner(&mut self) -> Result<NodeId> {
		if let Some(atom) = E::binding_atom(self)? {
			return Ok(atom);
		}
		match self.tok.kind {
			TokenKind::BracketL => {
				let start = self.tok.start;
				self.next()?;
				let elements = self.parse_binding_list(TokenKind::BracketR, true, true, false)?;
				let elements = self.list(&elements);
				Ok(self.add(NodeKind::ArrayPattern { elements }, start))
			}
			TokenKind::BraceL => self.parse_obj(true, &mut None),
			_ => self.parse_ident(false),
		}
	}

	pub(crate) fn parse_binding_list(
		&mut self,
		close: TokenKind,
		allow_empty: bool,
		allow_trailing_comma: bool,
		allow_modifiers: bool,
	) -> Result<Vec<Option<NodeId>>> {
		let mut elements = Vec::new();
		let mut first = true;
		while !self.eat(close)? {
			if first {
				first = false;
			} else {
				self.expect(TokenKind::Comma)?;
			}
			if allow_empty && self.is(TokenKind::Comma) {
				elements.push(None);
			} else if allow_trailing_comma && self.after_trailing_comma(close, false)? {
				break;
			} else if self.is(TokenKind::Ellipsis) {
				let rest = self.parse_rest_binding()?;
				E::binding_annotation(self, rest)?;
				elements.push(Some(rest));
				if self.is(TokenKind::Comma) {
					return self.error(self.tok.start, "Comma is not permitted after the rest element");
				}
				self.expect(close)?;
				break;
			} else {
				let start = E::binding_item_start(self, allow_modifiers)?;
				let left = self.parse_maybe_default(start, None)?;
				E::binding_annotation(self, left)?;
				let item = self.parse_maybe_default(self.start_of(left), Some(left))?;
				elements.push(Some(E::binding_item_end(self, item)?));
			}
		}
		Ok(elements)
	}

	pub(crate) fn parse_maybe_default(&mut self, start: u32, left: Option<NodeId>) -> Result<NodeId> {
		let left = match left {
			Some(left) => left,
			None => self.parse_binding_atom()?,
		};
		if !self.eat(TokenKind::Eq)? {
			return Ok(left);
		}
		let right = self.parse_maybe_assign(ForInit::No, &mut None)?;
		Ok(self.add(NodeKind::AssignmentPattern { left, right }, start))
	}

	pub(crate) fn parse_rest_binding(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		let argument = self.parse_binding_atom()?;
		Ok(self.add(NodeKind::RestElement { argument }, start))
	}

	pub(crate) fn check_lval_simple(
		&mut self,
		id: NodeId,
		binding: Binding,
		clashes: &mut Option<Vec<StrId>>,
	) -> Result<()> {
		let is_bind = binding != Binding::None;
		let start = self.start_of(id);
		match self.kind(id) {
			NodeKind::Identifier { name } => {
				let text = self.str(name);
				if self.strict && (self.is_reserved_word(text) || text == "eval" || text == "arguments") {
					let verb = if is_bind { "Binding " } else { "Assigning to " };
					return self.error(start, format!("{verb}{text} in strict mode"));
				}
				if is_bind {
					if binding == Binding::Lexical && text == "let" {
						return self.error(start, "let is disallowed as a lexically bound name");
					}
					if let Some(seen) = clashes {
						if seen.contains(&name) {
							return self.error(start, "Argument name clash");
						}
						seen.push(name);
					}
					if binding != Binding::Outside {
						self.declare_name(name, binding, start)?;
					}
				}
				Ok(())
			}
			NodeKind::ChainExpression { .. } => self.error(start, "Optional chaining cannot appear in left-hand side"),
			NodeKind::MemberExpression { .. } => {
				if is_bind {
					return self.error(start, "Binding member expression");
				}
				Ok(())
			}
			NodeKind::ParenthesizedExpression { expression } => {
				if is_bind {
					return self.error(start, "Binding parenthesized expression");
				}
				self.check_lval_simple(expression, binding, clashes)
			}
			NodeKind::Extension(_) if let Some(inner) = E::unwrap(self, id, Unwrap::Simple) => {
				self.check_lval_simple(inner, binding, clashes)
			}
			_ => self.error(
				start,
				if is_bind {
					"Binding rvalue"
				} else {
					"Assigning to rvalue"
				},
			),
		}
	}

	pub(crate) fn check_lval_pattern(
		&mut self,
		id: NodeId,
		binding: Binding,
		clashes: &mut Option<Vec<StrId>>,
	) -> Result<()> {
		match self.kind(id) {
			NodeKind::ObjectPattern { properties } => {
				for i in 0..properties.len {
					let prop = self.ast.lists[(properties.start + i) as usize].unwrap();
					self.check_lval_inner_pattern(prop, binding, clashes)?;
				}
				Ok(())
			}
			NodeKind::ArrayPattern { elements } => {
				for i in 0..elements.len {
					if let Some(element) = self.ast.lists[(elements.start + i) as usize] {
						self.check_lval_inner_pattern(element, binding, clashes)?;
					}
				}
				Ok(())
			}
			_ => self.check_lval_simple(id, binding, clashes),
		}
	}

	pub(crate) fn check_lval_inner_pattern(
		&mut self,
		id: NodeId,
		binding: Binding,
		clashes: &mut Option<Vec<StrId>>,
	) -> Result<()> {
		match self.kind(id) {
			NodeKind::Property { value, .. } => self.check_lval_inner_pattern(value, binding, clashes),
			NodeKind::AssignmentPattern { left, .. } => self.check_lval_pattern(left, binding, clashes),
			NodeKind::RestElement { argument } => self.check_lval_pattern(argument, binding, clashes),
			NodeKind::Extension(_) if let Some(inner) = E::unwrap(self, id, Unwrap::InnerPattern) => {
				self.check_lval_inner_pattern(inner, binding, clashes)
			}
			_ => self.check_lval_pattern(id, binding, clashes),
		}
	}
}
