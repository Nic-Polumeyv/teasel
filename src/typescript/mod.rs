//! TypeScript as an extension of the JavaScript grammar, matching `@sveltejs/acorn-typescript`.

pub mod ast;
mod declarations;
mod estree;
#[cfg(test)]
mod tests;
mod types;

use crate::ast::{Ast, List, NodeId, NodeKind, VariableKind};
use crate::error::SyntaxError;
use crate::interner::StrId;
use crate::lexer::token::{Keyword, TokenKind};
use crate::parser::class::ClassKind;
use crate::parser::expression::starts_expression;
use crate::parser::{
	Context, DestructuringErrors, Errors, Extension, ForInit, FunctionKind, Options, Parser, Result, Unwrap,
};
use ast::{Accessibility, Data, Extras, Kind, TsKind};
use std::collections::HashMap;
use types::TypeParameterModifiers;

pub fn parse(src: &str, options: Options) -> std::result::Result<Ast<Data>, SyntaxError> {
	crate::parser::parse::<TypeScript>(src, options)
}

pub fn parse_expression_at(
	src: &str,
	offset: u32,
	options: Options,
) -> std::result::Result<(Ast<Data>, NodeId), SyntaxError> {
	crate::parser::parse_expression_at::<TypeScript>(src, offset, options)
}

pub fn parse_pattern_at(
	src: &str,
	offset: u32,
	options: Options,
) -> std::result::Result<(Ast<Data>, NodeId), SyntaxError> {
	crate::parser::parse_pattern_at::<TypeScript>(src, offset, options)
}

pub fn parse_params_at(
	src: &str,
	offset: u32,
	options: Options,
) -> std::result::Result<(Ast<Data>, Vec<NodeId>, u32), SyntaxError> {
	crate::parser::parse_params_at::<TypeScript>(src, offset, options)
}

pub fn parse_statement_at(
	src: &str,
	offset: u32,
	options: Options,
) -> std::result::Result<(Ast<Data>, NodeId), SyntaxError> {
	crate::parser::parse_statement_at::<TypeScript>(src, offset, options)
}

/// Parser state that only TypeScript needs. `State` is copied into every snapshot, so it stays
/// small; the name tables only grow within a scope and are truncated instead.
#[derive(Default)]
pub struct TypeScript {
	state: State,
	types: Names,
	export_only: Names,
}

impl std::ops::Deref for TypeScript {
	type Target = State;

	fn deref(&self) -> &State {
		&self.state
	}
}

impl std::ops::DerefMut for TypeScript {
	fn deref_mut(&mut self) -> &mut State {
		&mut self.state
	}
}

#[derive(Clone, Default)]
pub struct State {
	disallow_conditional_types: bool,
	ambient: bool,
	in_abstract_class: bool,
	/// Inside a parenthesized list that may turn out to be arrow parameters.
	maybe_in_arrow_parameters: bool,
	functions: Vec<FunctionFrame>,
	classes: Vec<ClassFrame>,
	elements: Vec<ElementFrame>,
	/// Parsed ahead of `=>`, taken by the arrow's frame.
	arrow_return_type: Option<NodeId>,
	paren_lists: Vec<bool>,
	parameter_modifiers: Vec<ParameterHead>,
	definite: Vec<bool>,
	/// The kind of the import or export being parsed.
	outer_kind: Option<Kind>,
	export_kind: Option<Kind>,
	/// Modifiers for the class about to be parsed.
	next_class: ClassFrame,
	/// Decorators waiting for their class, one list per nesting level of decorator expressions.
	decorators: Vec<Vec<NodeId>>,
}

/// Names declared per scope depth, in declaration order so a scope's names are a suffix.
#[derive(Default)]
struct Names {
	log: Vec<(usize, StrId)>,
	depths: HashMap<StrId, Vec<usize>>,
}

impl Names {
	fn push(&mut self, depth: usize, name: StrId) {
		self.log.push((depth, name));
		self.depths.entry(name).or_default().push(depth);
	}

	fn contains(&self, name: StrId) -> bool {
		self.depths.get(&name).is_some_and(|d| !d.is_empty())
	}

	fn contains_at(&self, depth: usize, name: StrId) -> bool {
		self.depths.get(&name).is_some_and(|d| d.contains(&depth))
	}

	fn truncate(&mut self, len: usize) {
		debug_assert!(self.log.len() >= len);
		while self.log.len() > len {
			let (_, name) = self.log.pop().unwrap();
			self.depths.get_mut(&name).unwrap().pop();
		}
	}

	/// Forgets the names of a scope being left.
	fn exit(&mut self, depth: usize) {
		let len = self.log.iter().rposition(|(d, _)| *d < depth).map_or(0, |i| i + 1);
		self.truncate(len);
	}
}

/// What precedes a parameter: its decorators, and modifiers that make it a parameter property.
#[derive(Clone, Copy, Default)]
struct ParameterHead {
	modifiers: Option<(u32, Extras)>,
	decorators: Option<List>,
}

#[derive(Clone, Copy, Default)]
struct FunctionFrame {
	in_class_method: bool,
	type_parameters: Option<NodeId>,
	return_type: Option<NodeId>,
	arrow_parameters: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ClassFrame {
	is_abstract: bool,
	declare: bool,
	decorators: Option<List>,
	start: Option<u32>,
	type_parameters: Option<NodeId>,
	super_type_arguments: Option<NodeId>,
	implements: Option<List>,
	has_super: bool,
	outer_abstract: bool,
}

#[derive(Clone, Copy, Default)]
struct ElementFrame {
	start: u32,
	extras: Extras,
	key: Option<NodeId>,
	computed: bool,
	/// The ambient flag before a `declare` member set it.
	outer_ambient: Option<bool>,
}

#[derive(Default)]
pub(crate) struct Modifiers {
	pub extras: Extras,
	pub is_in: bool,
	pub is_out: bool,
	pub is_const: bool,
}

impl Modifiers {
	fn has(&self, modifier: &str) -> bool {
		match modifier {
			"public" => self.extras.accessibility == Some(Accessibility::Public),
			"private" => self.extras.accessibility == Some(Accessibility::Private),
			"protected" => self.extras.accessibility == Some(Accessibility::Protected),
			"declare" => self.extras.declare,
			"abstract" => self.extras.is_abstract,
			"override" => self.extras.is_override,
			"readonly" => self.extras.readonly,
			"accessor" => self.extras.accessor,
			"static" => self.extras.is_static,
			"in" => self.is_in,
			"out" => self.is_out,
			"const" => self.is_const,
			_ => unreachable!(),
		}
	}

	fn set(&mut self, modifier: &str) {
		match modifier {
			"public" => self.extras.accessibility = Some(Accessibility::Public),
			"private" => self.extras.accessibility = Some(Accessibility::Private),
			"protected" => self.extras.accessibility = Some(Accessibility::Protected),
			"declare" => self.extras.declare = true,
			"abstract" => self.extras.is_abstract = true,
			"override" => self.extras.is_override = true,
			"readonly" => self.extras.readonly = true,
			"accessor" => self.extras.accessor = true,
			"static" => self.extras.is_static = true,
			"in" => self.is_in = true,
			"out" => self.is_out = true,
			"const" => self.is_const = true,
			_ => unreachable!(),
		}
	}
}

impl Parser<'_, TypeScript> {
	pub(crate) fn ts(&mut self, kind: TsKind, start: u32) -> NodeId {
		let index = self.ast.extension.nodes.len() as u32;
		self.ast.extension.nodes.push(kind);
		self.add(NodeKind::Extension(index), start)
	}

	fn ts_index(&self, id: NodeId) -> Option<usize> {
		match self.kind(id) {
			NodeKind::Extension(index) => Some(index as usize),
			_ => None,
		}
	}

	fn ts_kind(&self, id: NodeId) -> Option<TsKind> {
		Some(self.ast.extension.nodes[self.ts_index(id)?])
	}

	fn ts_kind_mut(&mut self, id: NodeId) -> Option<&mut TsKind> {
		let index = self.ts_index(id)?;
		Some(&mut self.ast.extension.nodes[index])
	}

	/// The byte column acorn-typescript reports as a position for modifier order errors.
	fn column(&self, pos: u32) -> u32 {
		let source = self.source();
		let line_start = source[..pos as usize].rfind(['\n', '\r']).map_or(0, |i| i + 1);
		let column: usize = source[line_start..pos as usize].chars().map(char::len_utf16).sum();
		// The column is reported as if it were an offset, so it is mapped back to the byte
		// offset that serializes to that number.
		let mut units = 0;
		for (byte, c) in source.char_indices() {
			if units >= column {
				return byte as u32;
			}
			units += c.len_utf16();
		}
		source.len() as u32
	}

	fn extras_mut(&mut self, id: NodeId) -> &mut Extras {
		self.ast.extension.extras.entry(id).or_default()
	}

	fn ext_data(&self) -> &Data {
		&self.ast.extension
	}

	fn is_ident(&self) -> bool {
		matches!(self.tok.kind, TokenKind::Ident(_))
	}

	fn is_ident_or_keyword(&self) -> bool {
		matches!(self.tok.kind, TokenKind::Ident(_) | TokenKind::Keyword(_))
	}

	fn is_literal_property_name(&self) -> bool {
		matches!(
			self.tok.kind,
			TokenKind::Ident(_)
				| TokenKind::String(_)
				| TokenKind::Number(_)
				| TokenKind::BigInt
				| TokenKind::Keyword(_)
		)
	}

	fn is_arrow(&self, id: NodeId) -> bool {
		matches!(self.kind(id), NodeKind::ArrowFunctionExpression { .. })
	}

	/// A line break, or the end of the input, after the current token.
	fn has_following_line_break(&self) -> bool {
		let (next, newline, _) = self.peek_char();
		newline || next.is_none()
	}

	/// Eats a semicolon, or accepts the place one could be inserted.
	fn is_line_terminator(&mut self) -> Result<bool> {
		Ok(self.eat(TokenKind::Semi)? || self.can_insert_semicolon())
	}

	fn peek_is_contextual(&mut self, name: &str) -> Result<bool> {
		Ok(matches!(self.peek_token()?.kind, TokenKind::Ident(n) if self.str(n) == name))
	}

	/// Turns a `<<` read as an expression token into the `<` that opens type arguments.
	fn split_lt(&mut self) {
		self.lexer.set_pos(self.tok.start + 1);
		self.tok.kind = TokenKind::Lt;
		self.tok.end = self.tok.start + 1;
	}

	/// At the start of an expression the plugin reads a lone `<` whatever follows it.
	fn starts_with_lt(&mut self) -> bool {
		match self.tok.kind {
			TokenKind::Lt => true,
			TokenKind::LtLt | TokenKind::LtEq | TokenKind::LtLtEq => {
				self.split_lt();
				true
			}
			_ => false,
		}
	}

	/// Re-reads a `<` or `>` left by a type context, where they are single characters.
	fn rescan_lt_gt(&mut self) -> Result<()> {
		if self.is(TokenKind::Lt) || self.is(TokenKind::Gt) {
			self.relex()?;
		}
		Ok(())
	}

	fn parse_decorator(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		self.ext.decorators.push(Vec::new());
		let expression_start = self.tok.start;
		let mut expression = if self.is(TokenKind::ParenL) {
			self.next()?;
			let inner = self.parse_expression(false, &mut None)?;
			self.expect(TokenKind::ParenR)?;
			if self.options.preserve_parens {
				self.add(
					NodeKind::ParenthesizedExpression { expression: inner },
					expression_start,
				)
			} else {
				inner
			}
		} else {
			let mut expression = self.parse_ident(false)?;
			while self.eat(TokenKind::Dot)? {
				let property = self.parse_ident(true)?;
				expression = self.add(
					NodeKind::MemberExpression {
						object: expression,
						property,
						computed: false,
						optional: false,
					},
					expression_start,
				);
			}
			expression
		};
		if self.eat(TokenKind::ParenL)? {
			let args = self.parse_expr_list(TokenKind::ParenR, false, false, &mut None)?;
			let arguments = self.list(&args);
			let callee_start = self.start_of(expression);
			expression = self.add(
				NodeKind::CallExpression {
					callee: expression,
					arguments,
					optional: false,
				},
				callee_start,
			);
		}
		self.ext.decorators.pop();
		Ok(self.ts(TsKind::Decorator { expression }, start))
	}

	/// Decorators before a class, kept until the class takes them.
	fn parse_decorators(&mut self, allow_export: bool) -> Result<()> {
		let mut decorators = Vec::new();
		while self.is(TokenKind::At) {
			decorators.push(self.parse_decorator()?);
		}
		if self.is_keyword(Keyword::Export) {
			if !allow_export {
				return self.unexpected();
			}
		} else if !self.is_keyword(Keyword::Class)
			&& !(self.is_contextual("abstract") && self.peek_token()?.kind == TokenKind::Keyword(Keyword::Class))
		{
			return self.error(
				self.tok.start,
				"Leading decorators must be attached to a class declaration.",
			);
		}
		if self.ext.decorators.is_empty() {
			self.ext.decorators.push(Vec::new());
		}
		self.ext.decorators.last_mut().unwrap().extend(decorators);
		Ok(())
	}

	fn take_decorators(&mut self) -> Option<List> {
		let pending = self.ext.decorators.last_mut()?;
		if pending.is_empty() {
			return None;
		}
		let decorators = std::mem::take(pending);
		Some(self.list_of(&decorators))
	}

	fn scope_depth(&self) -> usize {
		self.scopes.len() - 1
	}

	fn declare_type(&mut self, id: NodeId, unique: bool) -> Result<()> {
		let NodeKind::Identifier { name } = self.kind(id) else {
			unreachable!()
		};
		let depth = self.scope_depth();
		if unique && self.ext.types.contains_at(depth, name) {
			return self.error(
				self.start_of(id),
				format!("type '{}' has already been declared.", self.str(name)),
			);
		}
		self.ext.types.push(depth, name);
		Ok(())
	}

	fn declare_export_only(&mut self, id: NodeId) {
		let NodeKind::Identifier { name } = self.kind(id) else {
			unreachable!()
		};
		let depth = self.scope_depth();
		if depth == 0 {
			self.undeclared_exports.remove(&name);
		}
		self.ext.export_only.push(depth, name);
	}

	/// A class member's name the way error messages print it.
	fn element_name(&self, element: ElementFrame) -> String {
		match element.key {
			Some(key) if !element.computed && matches!(self.kind(key), NodeKind::Identifier { .. }) => {
				let NodeKind::Identifier { name } = self.kind(key) else {
					unreachable!()
				};
				self.str(name).to_string()
			}
			Some(key) => format!(
				"[{}]",
				&self.source()[self.start_of(key) as usize..self.end_of(key) as usize]
			),
			None => String::new(),
		}
	}

	// Expressions

	/// `const` after `as`, `satisfies` or `<`: a type reference to the word itself.
	fn try_next_parse_constant_context(&mut self) -> Result<Option<NodeId>> {
		if self.peek_token()?.kind != TokenKind::Keyword(Keyword::Const) {
			return Ok(None);
		}
		self.next()?;
		let reference = self.parse_type_reference()?;
		if let Some(TsKind::TypeReference {
			type_arguments: Some(_),
			type_name,
		}) = self.ts_kind(reference)
		{
			return self.error(self.start_of(type_name), "Cannot find name 'const'.");
		}
		Ok(Some(reference))
	}

	fn parse_type_assertion(&mut self, for_init: ForInit) -> Result<NodeId> {
		let start = self.tok.start;
		let snapshot = self.snapshot();
		let assertion: Result<NodeId> = (|| {
			let type_annotation = match self.try_next_parse_constant_context()? {
				Some(constant) => constant,
				None => self.next_then_parse_type()?,
			};
			self.expect(TokenKind::Gt)?;
			let expression = self.parse_maybe_unary(&mut None, false, false, for_init)?;
			Ok(self.ts(
				TsKind::TypeAssertion {
					type_annotation,
					expression,
				},
				start,
			))
		})();
		match assertion {
			Ok(node) => Ok(node),
			Err(_) => {
				self.restore(snapshot);
				self.parse_type_parameters(TypeParameterModifiers::Const)
			}
		}
	}

	fn at_possible_async_arrow(&self, base: NodeId) -> bool {
		self.ident_is(base, "async")
			&& self.prev_end == self.end_of(base)
			&& !self.can_insert_semicolon()
			&& self.end_of(base) - self.start_of(base) == 5
			&& self.start_of(base) == self.potential_arrow_at
	}

	fn try_generic_async_arrow(&mut self, start: u32, for_init: ForInit) -> Result<Option<NodeId>> {
		let old = self.ext.maybe_in_arrow_parameters;
		self.ext.maybe_in_arrow_parameters = true;
		let snapshot = self.snapshot();
		let head: Result<(NodeId, Vec<Option<NodeId>>, Option<NodeId>)> = (|| {
			let type_parameters = self.parse_type_parameters(TypeParameterModifiers::Const)?;
			self.expect(TokenKind::ParenL)?;
			let params = self.parse_binding_list(TokenKind::ParenR, false, true, false)?;
			let return_type = if self.is(TokenKind::Colon) {
				Some(self.parse_type_or_type_predicate_annotation(TokenKind::Colon)?)
			} else {
				None
			};
			self.expect(TokenKind::Arrow)?;
			Ok((type_parameters, params, return_type))
		})();
		self.ext.maybe_in_arrow_parameters = old;
		let Ok((type_parameters, params, return_type)) = head else {
			self.restore(snapshot);
			return Ok(None);
		};
		self.ext.arrow_return_type = return_type;
		let arrow = self.parse_arrow_expression(start, params, true, for_init)?;
		self.extras_mut(arrow).type_parameters = Some(type_parameters);
		Ok(Some(arrow))
	}

	/// Type arguments after an expression: a call, a tagged template, or an instantiation
	/// expression. `None` means this is not one and the tokenizer must go back.
	#[allow(clippy::too_many_arguments)]
	fn parse_type_arguments_subscript(
		&mut self,
		base: NodeId,
		start: u32,
		no_calls: bool,
		chained: bool,
		is_optional_call: bool,
		for_init: ForInit,
	) -> Result<Option<NodeId>> {
		if !no_calls
			&& self.at_possible_async_arrow(base)
			&& let Some(arrow) = self.try_generic_async_arrow(start, for_init)?
		{
			return Ok(Some(arrow));
		}
		let Some(type_arguments) = self.parse_type_arguments_in_expression()? else {
			return Ok(Some(base));
		};
		if is_optional_call && !self.is(TokenKind::ParenL) {
			return Ok(None);
		}
		if self.is(TokenKind::Backquote) {
			if chained {
				return self.error(start, "Tagged Template Literals are not allowed in optionalChain.");
			}
			let quasi = self.parse_template(true)?;
			let node = self.add(NodeKind::TaggedTemplateExpression { tag: base, quasi }, start);
			self.extras_mut(node).type_arguments = Some(type_arguments);
			return Ok(Some(node));
		}
		if !no_calls && self.eat(TokenKind::ParenL)? {
			let mut errors = Some(DestructuringErrors::default());
			let args = self.parse_expr_list(TokenKind::ParenR, true, false, &mut errors)?;
			for arg in args.iter().flatten() {
				if let Some(TsKind::TypeCastExpression { type_annotation, .. }) = self.ts_kind(*arg) {
					return self.error(self.start_of(type_annotation), "Did not expect a type annotation here.");
				}
			}
			let arguments = self.list(&args);
			let node = self.add(
				NodeKind::CallExpression {
					callee: base,
					arguments,
					optional: is_optional_call,
				},
				start,
			);
			self.extras_mut(node).type_arguments = Some(type_arguments);
			self.check_expression_errors(&errors, true)?;
			return Ok(Some(node));
		}
		if self.is(TokenKind::Gt)
			|| self.is(TokenKind::GtGt)
			|| self.is(TokenKind::GtGtGt)
			|| self.is(TokenKind::LtLt)
			|| (!self.is(TokenKind::ParenL) && starts_expression(self.tok.kind) && !self.tok.newline_before)
		{
			return Ok(None);
		}
		Ok(Some(self.ts(
			TsKind::InstantiationExpression {
				expression: base,
				type_arguments,
			},
			start,
		)))
	}

	fn is_assignable(&self, id: NodeId, is_binding: bool) -> bool {
		match self.kind(id) {
			NodeKind::Identifier { .. }
			| NodeKind::ObjectPattern { .. }
			| NodeKind::ArrayPattern { .. }
			| NodeKind::AssignmentPattern { .. }
			| NodeKind::RestElement { .. } => true,
			NodeKind::ObjectExpression { properties } => {
				let last = properties.len.saturating_sub(1);
				self.ast.list(properties).iter().enumerate().all(|(i, prop)| {
					let prop = prop.unwrap();
					(i as u32 == last || !matches!(self.kind(prop), NodeKind::SpreadElement { .. }))
						&& self.is_assignable(prop, false)
				})
			}
			NodeKind::Property { value, .. } => self.is_assignable(value, false),
			NodeKind::SpreadElement { argument } => self.is_assignable(argument, false),
			NodeKind::ArrayExpression { elements } => self
				.ast
				.list(elements)
				.iter()
				.all(|element| element.is_none_or(|e| self.is_assignable(e, false))),
			NodeKind::AssignmentExpression { operator, .. } => operator == crate::ast::AssignmentOperator::Assign,
			NodeKind::ParenthesizedExpression { expression } => self.is_assignable(expression, false),
			NodeKind::MemberExpression { .. } => !is_binding,
			NodeKind::Extension(_) => match self.ts_kind(id) {
				Some(TsKind::TypeCastExpression { expression, .. }) => self.is_assignable(expression, is_binding),
				Some(TsKind::ParameterProperty { .. }) => true,
				_ => false,
			},
			_ => false,
		}
	}

	/// Reads a `: type` before `=>` into `arrow_return_type`; false when there is a colon but no
	/// arrow follows it.
	fn take_arrow_return_type(&mut self) -> Result<bool> {
		self.ext.arrow_return_type = None;
		if !self.is(TokenKind::Colon) {
			return Ok(true);
		}
		let snapshot = self.snapshot();
		match self.parse_type_or_type_predicate_annotation(TokenKind::Colon) {
			Ok(return_type) if !self.can_insert_semicolon() && self.is(TokenKind::Arrow) => {
				self.ext.arrow_return_type = Some(return_type);
				Ok(true)
			}
			_ => {
				self.restore(snapshot);
				Ok(false)
			}
		}
	}

	fn type_cast_to_parameter(&mut self, id: NodeId) -> NodeId {
		let Some(TsKind::TypeCastExpression {
			expression,
			type_annotation,
		}) = self.ts_kind(id)
		else {
			unreachable!()
		};
		self.extras_mut(expression).type_annotation = Some(type_annotation);
		self.ast.node_mut(expression).end = self.end_of(type_annotation);
		expression
	}
}

impl Extension for TypeScript {
	type Data = Data;
	type Snapshot = (State, usize, usize);

	const DUPLICATE_EXPORT_ERRORS: bool = false;
	const DECLARES_FUNCTION_NAME_AFTER_BODY: bool = true;
	const STATIC_IS_A_MODIFIER: bool = true;

	fn init(p: &mut Parser<Self>) {
		p.lexer.at_sign = true;
	}

	fn save(&self) -> Self::Snapshot {
		(self.state.clone(), self.types.log.len(), self.export_only.log.len())
	}

	fn restore(&mut self, (state, types, export_only): Self::Snapshot) {
		self.state = state;
		self.types.truncate(types);
		self.export_only.truncate(export_only);
	}

	// Statements and modules

	fn statement(p: &mut Parser<Self>, _context: Context, _top_level: bool) -> Result<Option<NodeId>> {
		if p.is(TokenKind::At) {
			p.parse_decorators(true)?;
		}
		let start = p.tok.start;
		if p.is_keyword(Keyword::Const) && p.peek_is_contextual("enum")? {
			p.next()?;
			return p.parse_enum_declaration(start, true, false).map(Some);
		}
		if p.is_contextual("enum") {
			return p.parse_enum_declaration(start, false, false).map(Some);
		}
		if p.is_contextual("interface") {
			return p.parse_interface_declaration(start, false, false);
		}
		Ok(None)
	}

	fn expression_statement(p: &mut Parser<Self>, start: u32, expression: NodeId) -> Result<Option<NodeId>> {
		p.parse_declaration_statement(start, expression)
	}

	fn starts_export_declaration(p: &mut Parser<Self>) -> bool {
		[
			"abstract",
			"declare",
			"enum",
			"module",
			"namespace",
			"interface",
			"type",
		]
		.iter()
		.any(|name| p.is_contextual(name))
			|| p.is(TokenKind::At)
	}

	fn export_head(p: &mut Parser<Self>, start: u32) -> Result<Option<NodeId>> {
		if p.is_keyword(Keyword::Import) {
			p.next()?;
			if p.is_contextual("type") && p.peek_char().0 != Some('=') {
				p.ext.outer_kind = Some(Kind::Type);
				p.next()?;
			} else {
				p.ext.outer_kind = Some(Kind::Value);
			}
			let node = p.parse_import_equals_declaration(start, true)?;
			p.ext.outer_kind = None;
			return Ok(Some(node));
		}
		if p.eat(TokenKind::Eq)? {
			let expression = p.parse_expression(false, &mut None)?;
			p.semicolon()?;
			return Ok(Some(p.ts(TsKind::ExportAssignment { expression }, start)));
		}
		if p.eat_contextual("as")? {
			p.expect_contextual("namespace")?;
			let id = p.parse_ident(false)?;
			p.semicolon()?;
			return Ok(Some(p.ts(TsKind::NamespaceExportDeclaration { id }, start)));
		}
		if p.is_contextual("type") && matches!(p.peek_token()?.kind, TokenKind::BraceL | TokenKind::Star) {
			p.next()?;
			p.ext.outer_kind = Some(Kind::Type);
			p.ext.export_kind = Some(Kind::Type);
		} else {
			p.ext.outer_kind = Some(Kind::Value);
		}
		Ok(None)
	}

	fn export_declaration(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		let start = p.tok.start;
		let old_ambient = p.ext.ambient;
		let is_declare = p.eat_contextual("declare")?;
		if is_declare {
			p.ext.ambient = true;
			if p.is_contextual("declare") || !p.should_parse_export_statement() {
				return p.error(
					p.tok.start,
					"'export declare' must be followed by an ambient declaration.",
				);
			}
		}
		let declaration = match p.tok.kind {
			TokenKind::Ident(name) => p.parse_declaration(start, name, true)?,
			_ => None,
		};
		let declaration = match declaration {
			Some(declaration) => declaration,
			None => p.parse_statement(Context::None, false, None)?,
		};
		p.ext.ambient = old_ambient;
		if is_declare
			|| matches!(
				p.ts_kind(declaration),
				Some(TsKind::InterfaceDeclaration { .. } | TsKind::TypeAliasDeclaration { .. })
			) {
			p.ext.export_kind = Some(Kind::Type);
		}
		if is_declare {
			p.ast.node_mut(declaration).start = start;
			p.extras_mut(declaration).declare = true;
		}
		Ok(Some(declaration))
	}

	fn export_default(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		let start = p.tok.start;
		if p.is_contextual("abstract") && p.peek_token()?.kind == TokenKind::Keyword(Keyword::Class) {
			p.next()?;
			p.ext.next_class = ClassFrame {
				is_abstract: true,
				start: Some(start),
				..ClassFrame::default()
			};
			return p.parse_class(ClassKind::Declaration).map(Some);
		}
		if p.is_contextual("interface") {
			return p.parse_interface_declaration(start, false, false);
		}
		Ok(None)
	}

	fn export_end(p: &mut Parser<Self>, node: NodeId) {
		if let Some(kind) = p.ext.export_kind.take() {
			p.extras_mut(node).export_kind = Some(kind);
		}
		p.ext.outer_kind = None;
	}

	fn export_specifier(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		if p.is_contextual("type") {
			return p.parse_type_only_specifier(false).map(Some);
		}
		Ok(None)
	}

	fn import_head(p: &mut Parser<Self>, start: u32) -> Result<Option<NodeId>> {
		p.ext.outer_kind = Some(Kind::Value);
		if p.is_ident() || p.is(TokenKind::Star) || p.is(TokenKind::BraceL) {
			let mut ahead = p.peek_token()?;
			let ahead_is_from = matches!(ahead.kind, TokenKind::Ident(n) if p.str(n) == "from");
			if p.is_contextual("type")
				&& ahead.kind != TokenKind::Comma
				&& !ahead_is_from
				&& ahead.kind != TokenKind::Eq
			{
				p.next()?;
				p.ext.outer_kind = Some(Kind::Type);
				ahead = p.peek_token()?;
			}
			if p.is_ident() && ahead.kind == TokenKind::Eq {
				let node = p.parse_import_equals_declaration(start, false)?;
				p.ext.outer_kind = None;
				return Ok(Some(node));
			}
		}
		Ok(None)
	}

	fn import_end(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		let kind = p.ext.outer_kind.take().unwrap_or(Kind::Value);
		p.extras_mut(node).import_kind = Some(kind);
		let NodeKind::ImportDeclaration { specifiers, .. } = p.kind(node) else {
			unreachable!()
		};
		if kind == Kind::Type
			&& specifiers.len > 1
			&& matches!(
				p.kind(p.ast.list(specifiers)[0].unwrap()),
				NodeKind::ImportDefaultSpecifier { .. }
			) {
			return p.error(
				p.start_of(node),
				"A type-only import can specify a default import or named bindings, but not both.",
			);
		}
		Ok(())
	}

	fn import_specifier(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		if p.is_contextual("type") {
			return p.parse_type_only_specifier(true).map(Some);
		}
		Ok(None)
	}

	fn declares_export(p: &mut Parser<Self>, name: StrId) -> bool {
		p.ext.types.contains(name) || p.ext.export_only.contains(name)
	}

	fn scope_exit(p: &mut Parser<Self>) {
		let depth = p.scope_depth();
		p.ext.types.exit(depth);
		p.ext.export_only.exit(depth);
	}

	// Bindings

	fn var_id(p: &mut Parser<Self>, id: NodeId) -> Result<()> {
		let mut definite = false;
		if matches!(p.kind(id), NodeKind::Identifier { .. }) && !p.tok.newline_before && p.is(TokenKind::Bang) {
			p.next()?;
			definite = true;
		}
		if p.is(TokenKind::Colon) {
			let annotation = p.parse_type_annotation(true, None)?;
			p.extras_mut(id).type_annotation = Some(annotation);
			p.ast.node_mut(id).end = p.prev_end;
		}
		p.ext.definite.push(definite);
		Ok(())
	}

	fn var_declarator(p: &mut Parser<Self>, node: NodeId, kind: VariableKind) -> Result<()> {
		if p.ext.definite.pop() == Some(true) {
			p.extras_mut(node).definite = true;
		}
		if p.ext.ambient {
			p.check_ambient_initializer(node, kind)?;
		}
		Ok(())
	}

	fn allows_missing_initializer(p: &mut Parser<Self>) -> bool {
		p.ext.ambient
	}

	fn binding_atom(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		if p.is_keyword(Keyword::This) {
			return p.parse_ident(true).map(Some);
		}
		Ok(None)
	}

	fn binding_item_start(p: &mut Parser<Self>, allow_modifiers: bool) -> Result<u32> {
		let mut decorators = Vec::new();
		while p.is(TokenKind::At) {
			decorators.push(p.parse_decorator()?);
		}
		let decorators = if decorators.is_empty() {
			None
		} else {
			Some(p.list_of(&decorators))
		};
		let start = p.tok.start;
		let mut entry = None;
		if allow_modifiers {
			let modifiers = p.parse_modifiers(
				&["public", "private", "protected", "override", "readonly"],
				&[],
				false,
				"",
			)?;
			let extras = modifiers.extras;
			if extras.accessibility.is_some() || extras.readonly || extras.is_override {
				entry = Some((start, extras));
			}
		}
		p.ext.parameter_modifiers.push(ParameterHead {
			modifiers: entry,
			decorators,
		});
		Ok(start)
	}

	fn binding_annotation(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		if p.eat(TokenKind::Question)? {
			if !matches!(p.kind(node), NodeKind::Identifier { .. }) && !p.ext.ambient && !p.lexer.in_type {
				return p.error(
					p.start_of(node),
					"A binding pattern parameter cannot be optional in an implementation signature.",
				);
			}
			p.extras_mut(node).optional = true;
		}
		if p.is(TokenKind::Colon) {
			let annotation = p.parse_type_annotation(true, None)?;
			if matches!(p.kind(node), NodeKind::AssignmentPattern { .. }) {
				return p.error(
					p.start_of(annotation),
					"Type annotations must come before default assignments, e.g. instead of `age = 25: number` use `age: number = 25`.",
				);
			}
			p.extras_mut(node).type_annotation = Some(annotation);
		}
		p.ast.node_mut(node).end = p.prev_end;
		if matches!(p.kind(node), NodeKind::RestElement { .. })
			&& p.ext.ambient
			&& p.is(TokenKind::Comma)
			&& p.peek_char().0 == Some(')')
		{
			p.next()?;
		}
		Ok(())
	}

	fn binding_item_end(p: &mut Parser<Self>, item: NodeId) -> Result<NodeId> {
		let head = p.ext.parameter_modifiers.pop().unwrap();
		if head.decorators.is_some() {
			p.extras_mut(item).decorators = head.decorators;
		}
		let Some((start, extras)) = head.modifiers else {
			return Ok(item);
		};
		if !matches!(
			p.kind(item),
			NodeKind::Identifier { .. } | NodeKind::AssignmentPattern { .. }
		) {
			return p.error(
				start,
				"A parameter property may not be declared using a binding pattern.",
			);
		}
		let node = p.ts(TsKind::ParameterProperty { parameter: item }, start);
		p.extras_mut(node).merge(extras);
		Ok(node)
	}

	fn catch_param(p: &mut Parser<Self>, param: NodeId) -> Result<()> {
		if p.is(TokenKind::Colon) {
			let annotation = p.parse_type_annotation(true, None)?;
			p.extras_mut(param).type_annotation = Some(annotation);
			p.ast.node_mut(param).end = p.prev_end;
		}
		Ok(())
	}

	/// Svelte keeps an identifier's own end here and extends a destructuring pattern's.
	fn pattern_annotation(p: &mut Parser<Self>, pattern: NodeId) -> Result<()> {
		if !p.is(TokenKind::Colon) {
			return Ok(());
		}
		let annotation = p.parse_type_annotation(true, None)?;
		p.extras_mut(pattern).type_annotation = Some(annotation);
		if !matches!(p.kind(pattern), NodeKind::Identifier { .. }) {
			p.ast.node_mut(pattern).end = p.end_of(annotation);
		}
		Ok(())
	}

	// Functions and classes

	fn function_start(p: &mut Parser<Self>, kind: FunctionKind) -> Result<()> {
		let mut frame = FunctionFrame {
			in_class_method: kind == FunctionKind::Method { in_class: true },
			arrow_parameters: p.ext.maybe_in_arrow_parameters,
			..FunctionFrame::default()
		};
		p.ext.maybe_in_arrow_parameters = false;
		match kind {
			FunctionKind::Arrow => frame.return_type = p.ext.arrow_return_type.take(),
			_ => frame.type_parameters = p.try_parse_type_parameters(TypeParameterModifiers::Const)?,
		}
		p.ext.functions.push(frame);
		Ok(())
	}

	fn function_body(
		p: &mut Parser<Self>,
		start: u32,
		id: Option<NodeId>,
		params: List,
		is_async: bool,
		generator: bool,
		kind: FunctionKind,
	) -> Result<Option<NodeId>> {
		if p.is(TokenKind::Colon) {
			let return_type = p.parse_type_or_type_predicate_annotation(TokenKind::Colon)?;
			p.ext.functions.last_mut().unwrap().return_type = Some(return_type);
		}
		let bodiless = match kind {
			FunctionKind::Declaration => Some(TsKind::DeclareFunction {
				id,
				params,
				is_async,
				generator,
			}),
			FunctionKind::Method { in_class: true } => Some(TsKind::DeclareMethod {
				params,
				is_async,
				generator,
			}),
			_ => None,
		};
		if let Some(kind) = bodiless
			&& !p.is(TokenKind::BraceL)
			&& p.is_line_terminator()?
		{
			return Ok(Some(p.ts(kind, start)));
		}
		if kind == FunctionKind::Declaration && p.ext.ambient {
			return p.error(start, "An implementation cannot be declared in ambient contexts.");
		}
		Ok(None)
	}

	fn function_end(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		let frame = p.ext.functions.pop().unwrap();
		p.ext.maybe_in_arrow_parameters = frame.arrow_parameters;
		if frame.type_parameters.is_some() || frame.return_type.is_some() {
			let extras = p.extras_mut(node);
			if frame.type_parameters.is_some() {
				extras.type_parameters = frame.type_parameters;
			}
			extras.return_type = frame.return_type;
		}
		if frame.in_class_method
			&& matches!(p.kind(node), NodeKind::FunctionExpression { .. })
			&& let Some(element) = p.ext.elements.last()
			&& element.extras.is_abstract
		{
			let name = p.element_name(*element);
			return p.error(
				element.start,
				format!("Method '{name}' cannot have an implementation because it is marked abstract."),
			);
		}
		Ok(())
	}

	fn accessor_this_param(p: &Parser<Self>, params: List) -> bool {
		p.ast
			.list(params)
			.first()
			.is_some_and(|param| param.is_some_and(|param| p.ident_is(param, "this")))
	}

	fn function_params(p: &Parser<Self>, node: NodeId) -> Option<List> {
		match p.ts_kind(node) {
			Some(TsKind::DeclareFunction { params, .. } | TsKind::DeclareMethod { params, .. }) => Some(params),
			_ => None,
		}
	}

	fn class_start(p: &mut Parser<Self>) -> Result<()> {
		let mut frame = std::mem::take(&mut p.ext.next_class);
		if let Some(decorators) = p.take_decorators() {
			frame.decorators = Some(decorators);
			frame.start = Some(p.start_of(p.ast.list(decorators)[0].unwrap()));
		}
		frame.outer_abstract = p.ext.in_abstract_class;
		p.ext.in_abstract_class = frame.is_abstract;
		p.ext.classes.push(frame);
		Ok(())
	}

	fn starts_class_heritage(p: &mut Parser<Self>) -> bool {
		p.is_contextual("implements")
	}

	fn class_type_parameters(p: &mut Parser<Self>) -> Result<()> {
		let type_parameters = p.try_parse_type_parameters(TypeParameterModifiers::InOut)?;
		p.ext.classes.last_mut().unwrap().type_parameters = type_parameters;
		Ok(())
	}

	fn class_heritage(p: &mut Parser<Self>, has_super: bool) -> Result<()> {
		let mut super_type_arguments = None;
		if has_super && (p.is(TokenKind::Lt) || p.is(TokenKind::LtLt)) {
			super_type_arguments = p.parse_type_arguments_in_expression()?;
		}
		let implements = if p.eat_contextual("implements")? {
			let clause = p.parse_heritage_clause("implements")?;
			Some(p.list_of(&clause))
		} else {
			None
		};
		let frame = p.ext.classes.last_mut().unwrap();
		frame.has_super = has_super;
		frame.super_type_arguments = super_type_arguments;
		frame.implements = implements;
		Ok(())
	}

	fn class_end(p: &mut Parser<Self>, node: NodeId) {
		let frame = p.ext.classes.pop().unwrap();
		p.ext.in_abstract_class = frame.outer_abstract;
		if let Some(start) = frame.start {
			p.ast.node_mut(node).start = start;
		}
		let extras = p.extras_mut(node);
		extras.type_parameters = frame.type_parameters;
		extras.super_type_arguments = frame.super_type_arguments;
		extras.implements = frame.implements;
		extras.decorators = frame.decorators;
		extras.is_abstract = frame.is_abstract;
		extras.declare = frame.declare;
	}

	fn class_modifiers(p: &mut Parser<Self>) -> Result<bool> {
		let mut decorators = Vec::new();
		while p.is(TokenKind::At) {
			decorators.push(p.parse_decorator()?);
		}
		let start = p.tok.start;
		let mut modifiers = p.parse_modifiers(
			&[
				"declare",
				"private",
				"public",
				"protected",
				"accessor",
				"override",
				"abstract",
				"readonly",
				"static",
			],
			&["in", "out"],
			true,
			"'{}' modifier can only appear on a type parameter of a class, interface or type alias.",
		)?;
		if !decorators.is_empty() {
			if p.is(TokenKind::BraceR) {
				return p.error(p.tok.end, "Decorators must be attached to a class element.");
			}
			modifiers.extras.decorators = Some(p.list_of(&decorators));
		}
		if modifiers.extras != Extras::default() && p.is_contextual("static") && p.peek_char().0 == Some('{') {
			let snapshot = p.token_snapshot();
			p.next()?;
			p.next()?;
			let inside = p.tok.start;
			p.restore_tokens(snapshot);
			return p.error(inside, "Static class blocks cannot have any modifier.");
		}
		let outer_ambient = modifiers
			.extras
			.declare
			.then(|| std::mem::replace(&mut p.ext.ambient, true));
		p.ext.elements.push(ElementFrame {
			start,
			extras: modifiers.extras,
			key: None,
			computed: false,
			outer_ambient,
		});
		Ok(modifiers.extras.is_static)
	}

	fn class_index_signature(p: &mut Parser<Self>, start: u32) -> Result<Option<NodeId>> {
		let Some(signature) = p.try_parse_index_signature(start)? else {
			let has_super = p.ext.classes.last().is_some_and(|c| c.has_super);
			let extras = p.ext.elements.last().unwrap().extras;
			if !p.ext.in_abstract_class && extras.is_abstract {
				return p.error(start, "Abstract methods can only appear within an abstract class.");
			}
			if extras.is_override && !has_super {
				return p.error(
					start,
					"This member cannot have an 'override' modifier because its containing class does not extend another class.",
				);
			}
			return Ok(None);
		};
		let extras = p.ext.elements.last().unwrap().extras;
		if extras.is_abstract {
			return p.error(start, "Index signatures cannot have the 'abstract' modifier.");
		}
		if let Some(accessibility) = extras.accessibility {
			return p.error(
				start,
				format!(
					"Index signatures cannot have an accessibility modifier ('{}').",
					accessibility.as_str()
				),
			);
		}
		if extras.declare {
			return p.error(start, "Index signatures cannot have the 'declare' modifier.");
		}
		if extras.is_override {
			return p.error(start, "'override' modifier cannot appear on an index signature.");
		}
		Ok(Some(signature))
	}

	fn class_key_end(p: &mut Parser<Self>, key: NodeId, computed: bool) -> Result<()> {
		let element = p.ext.elements.last_mut().unwrap();
		element.key = Some(key);
		element.computed = computed;
		let start = element.start;
		let extras = element.extras;
		if p.eat(TokenKind::Question)? {
			p.ext.elements.last_mut().unwrap().extras.optional = true;
		}
		if extras.readonly && p.is(TokenKind::ParenL) {
			return p.error(start, "Class methods cannot have the 'readonly' modifier.");
		}
		if extras.declare && p.is(TokenKind::ParenL) {
			return p.error(start, "Class methods cannot have the 'declare' modifier.");
		}
		Ok(())
	}

	fn starts_class_method(p: &mut Parser<Self>) -> bool {
		p.is(TokenKind::Lt)
	}

	fn class_method_start(p: &mut Parser<Self>) -> Result<()> {
		let type_parameters = p.try_parse_type_parameters(TypeParameterModifiers::Const)?;
		let element = *p.ext.elements.last().unwrap();
		let key = element.key.unwrap();
		if matches!(p.kind(key), NodeKind::PrivateIdentifier { .. }) {
			if let Some(accessibility) = element.extras.accessibility {
				return p.error(
					element.start,
					format!(
						"Private methods cannot have an accessibility modifier ('{}').",
						accessibility.as_str()
					),
				);
			}
		} else if let Some(type_parameters) = type_parameters
			&& !element.extras.is_static
			&& !element.computed
			&& matches!(p.kind(key), NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } if p.str(name) == "constructor")
		{
			return p.error(
				p.start_of(type_parameters),
				"Type parameters cannot appear on a constructor declaration.",
			);
		}
		p.ext.elements.last_mut().unwrap().extras.type_parameters = type_parameters;
		Ok(())
	}

	fn class_field_annotation(p: &mut Parser<Self>) -> Result<()> {
		let element = *p.ext.elements.last().unwrap();
		let mut definite = false;
		let mut optional = element.extras.optional;
		if !optional {
			if p.is(TokenKind::Bang) {
				p.next()?;
				definite = true;
			} else {
				optional = p.eat(TokenKind::Question)?;
			}
		}
		let type_annotation = p.try_parse_type_annotation()?;
		let extras = &mut p.ext.elements.last_mut().unwrap().extras;
		extras.definite = definite;
		extras.optional = optional;
		extras.type_annotation = type_annotation;
		let private = element
			.key
			.is_some_and(|key| matches!(p.kind(key), NodeKind::PrivateIdentifier { .. }));
		if private {
			if element.extras.is_abstract {
				return p.error(element.start, "Private elements cannot have the 'abstract' modifier.");
			}
			if let Some(accessibility) = element.extras.accessibility {
				return p.error(
					element.start,
					format!(
						"Private elements cannot have an accessibility modifier ('{}').",
						accessibility.as_str()
					),
				);
			}
		} else if p.is(TokenKind::Eq) {
			if p.ext.ambient && !(element.extras.readonly && type_annotation.is_none()) {
				return p.error(p.tok.start, "Initializers are not allowed in ambient contexts.");
			}
			if element.extras.is_abstract {
				let name = p.element_name(element);
				return p.error(
					p.tok.start,
					format!("Property '{name}' cannot have an initializer because it is marked abstract."),
				);
			}
		}
		Ok(())
	}

	fn class_element_end(p: &mut Parser<Self>, node: NodeId) -> Result<()> {
		let frame = p.ext.elements.pop().unwrap();
		if let Some(outer) = frame.outer_ambient {
			p.ext.ambient = outer;
		}
		if let Some(decorators) = frame.extras.decorators {
			let start = p.start_of(p.ast.list(decorators)[0].unwrap());
			p.ast.node_mut(node).start = start;
			if let NodeKind::MethodDefinition {
				kind: crate::ast::MethodKind::Constructor,
				value,
				..
			} = p.kind(node)
				&& matches!(p.kind(value), NodeKind::FunctionExpression { .. })
			{
				return p.error(
					start,
					"Decorators can't be used with a constructor. Did you mean '@dec class { ... }'?",
				);
			}
		}
		if frame.extras != Extras::default() {
			p.extras_mut(node).merge(frame.extras);
		}
		Ok(())
	}

	// Expressions

	fn maybe_assign(p: &mut Parser<Self>, for_init: ForInit, errors: &mut Errors) -> Result<Option<NodeId>> {
		if !p.starts_with_lt() {
			return Ok(None);
		}
		let snapshot = p.snapshot();
		let saved_errors = *errors;
		let attempt: Result<(NodeId, NodeId)> = (|| {
			let type_parameters = p.parse_type_parameters(TypeParameterModifiers::Const)?;
			let expr = p.parse_maybe_assign(for_init, errors)?;
			Ok((type_parameters, expr))
		})();
		match attempt {
			Ok((type_parameters, expr)) if p.is_arrow(expr) => {
				let start = p.start_of(type_parameters);
				p.ast.node_mut(expr).start = start;
				p.extras_mut(expr).type_parameters = Some(type_parameters);
				Ok(Some(expr))
			}
			_ => {
				p.restore(snapshot);
				*errors = saved_errors;
				Ok(None)
			}
		}
	}

	fn paren_list_start(p: &mut Parser<Self>) {
		let inside = p.ext.maybe_in_arrow_parameters;
		p.ext.paren_lists.push(inside);
		p.ext.maybe_in_arrow_parameters = true;
	}

	fn paren_list_end(p: &mut Parser<Self>) {
		p.ext.maybe_in_arrow_parameters = p.ext.paren_lists.pop().unwrap();
	}

	fn paren_item(p: &mut Parser<Self>, item: NodeId) -> Result<NodeId> {
		let start = p.tok.start;
		if p.eat(TokenKind::Question)? {
			p.extras_mut(item).optional = true;
			p.ast.node_mut(item).end = p.prev_end;
		}
		if p.is(TokenKind::Colon) {
			let type_annotation = p.parse_type_annotation(true, None)?;
			return Ok(p.ts(
				TsKind::TypeCastExpression {
					expression: item,
					type_annotation,
				},
				start,
			));
		}
		Ok(item)
	}

	fn spread(p: &mut Parser<Self>, spread: NodeId) -> Result<()> {
		if p.ext.maybe_in_arrow_parameters && p.is(TokenKind::Colon) {
			let annotation = p.parse_type_annotation(true, None)?;
			p.extras_mut(spread).type_annotation = Some(annotation);
		}
		Ok(())
	}

	fn conditional(p: &mut Parser<Self>, expr: NodeId, start: u32, for_init: ForInit) -> Result<Option<NodeId>> {
		if !p.ext.maybe_in_arrow_parameters {
			return Ok(None);
		}
		let snapshot = p.snapshot();
		match p.parse_conditional(expr, start, for_init) {
			Ok(node) => Ok(Some(node)),
			Err(_) => {
				p.restore(snapshot);
				Ok(Some(expr))
			}
		}
	}

	fn unary(p: &mut Parser<Self>, for_init: ForInit) -> Result<Option<NodeId>> {
		if p.starts_with_lt() {
			return p.parse_type_assertion(for_init).map(Some);
		}
		Ok(None)
	}

	fn atom(p: &mut Parser<Self>, errors: &mut Errors, for_init: ForInit, for_new: bool) -> Result<Option<NodeId>> {
		if p.is(TokenKind::At) {
			p.parse_decorators(false)?;
			return p.parse_expr_atom(errors, for_init, for_new).map(Some);
		}
		Ok(None)
	}

	fn expr_op(p: &mut Parser<Self>, left: NodeId, left_start: u32, min_prec: i8) -> Result<Option<NodeId>> {
		if 7 <= min_prec || p.tok.newline_before {
			return Ok(None);
		}
		let is_as = p.is_contextual("as");
		if !is_as && !p.is_contextual("satisfies") {
			return Ok(None);
		}
		let type_annotation = match p.try_next_parse_constant_context()? {
			Some(constant) => constant,
			None => p.next_then_parse_type()?,
		};
		let kind = if is_as {
			TsKind::AsExpression {
				expression: left,
				type_annotation,
			}
		} else {
			TsKind::SatisfiesExpression {
				expression: left,
				type_annotation,
			}
		};
		let node = p.ts(kind, left_start);
		p.rescan_lt_gt()?;
		Ok(Some(node))
	}

	fn subscript(
		p: &mut Parser<Self>,
		base: NodeId,
		start: u32,
		no_calls: bool,
		_maybe_async_arrow: bool,
		optional_chained: bool,
		for_init: ForInit,
	) -> Result<Option<(NodeId, bool)>> {
		if !p.tok.newline_before && p.is(TokenKind::Bang) {
			p.next()?;
			let node = p.ts(TsKind::NonNullExpression { expression: base }, start);
			return Ok(Some((node, false)));
		}
		let optional_call = p.is(TokenKind::QuestionDot) && p.peek_char().0 == Some('<');
		if !optional_call && !p.is(TokenKind::Lt) && !p.is(TokenKind::LtLt) {
			return Ok(None);
		}
		let mut chained = optional_chained;
		let mut is_optional_call = false;
		let snapshot = p.snapshot();
		if optional_call {
			if no_calls {
				return Ok(Some((base, false)));
			}
			p.extras_mut(base).optional = true;
			chained = true;
			is_optional_call = true;
			p.next()?;
		}
		if p.is(TokenKind::Lt) || p.is(TokenKind::LtLt) {
			match p.parse_type_arguments_subscript(base, start, no_calls, chained, is_optional_call, for_init) {
				Ok(Some(node)) => {
					if matches!(p.ts_kind(node), Some(TsKind::InstantiationExpression { .. }))
						&& (p.is(TokenKind::Dot) || (p.is(TokenKind::QuestionDot) && p.peek_char().0 != Some('(')))
					{
						return p.error(
							p.tok.start,
							"Invalid property access after an instantiation expression. You can either wrap the instantiation expression in parentheses, or delete the type arguments.",
						);
					}
					return Ok(Some((node, is_optional_call)));
				}
				Ok(None) | Err(_) => p.restore(snapshot),
			}
		}
		Ok(None)
	}

	fn should_parse_arrow(p: &mut Parser<Self>, items: &[Option<NodeId>]) -> Result<bool> {
		let should = if p.is(TokenKind::Colon) {
			items.iter().flatten().all(|item| p.is_assignable(*item, true))
		} else {
			!p.can_insert_semicolon()
		};
		Ok(should && p.take_arrow_return_type()? && p.is(TokenKind::Arrow))
	}

	fn should_parse_async_arrow(p: &mut Parser<Self>) -> Result<bool> {
		Ok(p.take_arrow_return_type()? && !p.can_insert_semicolon() && p.eat(TokenKind::Arrow)?)
	}

	fn checks_assignment_target(p: &mut Parser<Self>) -> bool {
		!p.ext.maybe_in_arrow_parameters
	}

	fn new_expression(p: &mut Parser<Self>, node: NodeId) {
		let NodeKind::NewExpression { callee, arguments } = p.kind(node) else {
			unreachable!()
		};
		if let Some(TsKind::InstantiationExpression {
			expression,
			type_arguments,
		}) = p.ts_kind(callee)
		{
			p.ast.node_mut(node).kind = NodeKind::NewExpression {
				callee: expression,
				arguments,
			};
			p.extras_mut(node).type_arguments = Some(type_arguments);
		}
	}

	fn property_value(
		p: &mut Parser<Self>,
		start: u32,
		key: NodeId,
		computed: bool,
		is_pattern: bool,
		generator: bool,
		is_async: bool,
	) -> Result<Option<NodeId>> {
		if !p.is(TokenKind::Lt) {
			return Ok(None);
		}
		if is_pattern {
			return p.unexpected();
		}
		let value = p.parse_method(generator, is_async, false, false)?;
		Ok(Some(p.add(
			NodeKind::Property {
				key,
				value,
				kind: crate::ast::PropertyKind::Init,
				computed,
				method: true,
				shorthand: false,
			},
			start,
		)))
	}

	fn template_expression(p: &mut Parser<Self>) -> Result<Option<NodeId>> {
		if p.lexer.in_type {
			return p.parse_type().map(Some);
		}
		Ok(None)
	}

	fn make_pattern(p: &mut Parser<Self>, id: NodeId, is_binding: bool, errors: &mut Errors) -> Result<Option<NodeId>> {
		match p.ts_kind(id) {
			Some(
				TsKind::AsExpression { expression, .. }
				| TsKind::SatisfiesExpression { expression, .. }
				| TsKind::NonNullExpression { expression }
				| TsKind::TypeAssertion { expression, .. },
			) => p.make_pattern(expression, is_binding, errors).map(Some),
			Some(TsKind::TypeCastExpression { .. }) => {
				let parameter = p.type_cast_to_parameter(id);
				p.make_pattern(parameter, is_binding, errors).map(Some)
			}
			_ => Ok(None),
		}
	}

	/// Only `expr: type` items become their parameter; other wrappers stay in a list.
	fn convert_items(p: &mut Parser<Self>, items: &mut [Option<NodeId>]) {
		for item in items.iter_mut().flatten() {
			if matches!(p.ts_kind(*item), Some(TsKind::TypeCastExpression { .. })) {
				*item = p.type_cast_to_parameter(*item);
			}
		}
	}

	/// Parentheses around a type wrapper or more parentheses drop away as an assignment target.
	fn parenthesized_pattern(p: &mut Parser<Self>, paren: NodeId, inner: NodeId, pattern: NodeId) -> NodeId {
		let wrapper = matches!(
			p.ts_kind(inner),
			Some(
				TsKind::AsExpression { .. }
					| TsKind::SatisfiesExpression { .. }
					| TsKind::NonNullExpression { .. }
					| TsKind::TypeAssertion { .. }
			)
		) || matches!(p.kind(inner), NodeKind::ParenthesizedExpression { .. });
		if wrapper { pattern } else { paren }
	}

	fn unwrap(p: &Parser<Self>, id: NodeId, context: Unwrap) -> Option<NodeId> {
		match (context, p.ts_kind(id)?) {
			(Unwrap::Simple, TsKind::NonNullExpression { expression } | TsKind::AsExpression { expression, .. }) => {
				Some(expression)
			}
			(Unwrap::InnerPattern, TsKind::ParameterProperty { parameter }) => Some(parameter),
			_ => None,
		}
	}
}
