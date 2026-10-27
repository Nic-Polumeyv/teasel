//! TypeScript's grammar where it departs from JavaScript's, and what the parser keeps to read it.

use crate::error::Code;
pub mod ast;
mod declarations;
#[cfg(test)]
mod tests;
mod types;

use crate::ast::{FieldKind, List, MethodKind, NodeId, NodeKind, UnaryOperator, VariableKind};
use crate::error::SyntaxError;
use crate::interner::FastMap;
use crate::interner::StrId;
use crate::lexer::token::{Keyword, TokenKind};
use crate::parser::expression::starts_expression;
use crate::parser::scope::{SCOPE_ARROW, function_flags};
use crate::parser::statement::{ClassKind, StatementPlace};
use crate::parser::{Context, DestructuringErrors, Errors, ForInit, FunctionKind, Parser, Result, Unwrap};
use ast::{Accessibility, Extras, Kind, TsKind};
use declarations::Head;
use types::TypeParameterModifiers;

/// Parser state that only TypeScript needs. `State` is copied into every snapshot, so it stays
/// small; the name tables only grow within a scope and are truncated instead.
#[derive(Default)]
pub(crate) struct TypeScript {
	state: State,
	types: Names,
	export_only: Names,
	// outside State so a rewind keeps them: nested `<A>(<A>(…))` and ternary arrows were read twice per level
	not_arrow: crate::interner::FastSet<(u32, bool)>,
	colon_after_body: FastMap<u32, bool>,
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

/// What a snapshot keeps of TypeScript's state: the state, and how many names each table had.
pub(crate) type Saved = (State, usize, usize);

#[derive(Clone, Default)]
pub(crate) struct State {
	disallow_conditional_types: bool,
	infer_depth: u32,
	ambient: bool,
	/// The innermost module body is a namespace's, not a quoted module's or global's.
	in_namespace: bool,
	/// Inside `declare global` or a `global` block: what is declared there is the module's to export.
	global_depth: u32,
	globals: crate::interner::FastSet<StrId>,
	in_abstract_class: bool,
	classes: Vec<ClassFrame>,
	elements: Vec<ElementFrame>,
	/// Parsed ahead of `=>`, taken by the arrow's frame.
	arrow_return_type: Option<NodeId>,
	needs_colon_at: Option<u32>,
	body_needs_colon: bool,
	/// The kind of the import or export being parsed.
	outer_kind: Option<Kind>,
	export_kind: Option<Kind>,
	/// Modifiers for the class about to be parsed.
	next_class: ClassFrame,
	/// Inside a namespace or module block, whose exports name its own scope.
	module_blocks: u32,
	/// Decorators waiting for their class, one list per nesting level of decorator expressions.
	decorators: Vec<Vec<NodeId>>,
	decorators_before_export: bool,
}

/// Names declared per scope depth, in declaration order so a scope's names are a suffix.
#[derive(Default)]
struct Names {
	log: Vec<(usize, StrId)>,
	depths: FastMap<StrId, Vec<usize>>,
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

/// What TypeScript reads of a function before its body and sets on it after.
#[derive(Clone, Copy, Default)]
pub(crate) struct FunctionFrame {
	in_class_method: bool,
	type_parameters: Option<NodeId>,
	return_type: Option<NodeId>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ClassFrame {
	is_abstract: bool,
	declare: bool,
	is_declaration: bool,
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
	accessor: bool,
}

#[derive(Default, PartialEq)]
pub(crate) struct Modifiers {
	pub extras: Extras,
	pub accessor: bool,
	pub is_in: bool,
	pub is_out: bool,
	pub is_const: bool,
}

/// The modifier words and the field each one sets.
macro_rules! modifiers {
	($($word:literal => $($field:ident).+ = $value:expr),* $(,)?) => {
		pub(super) const MODIFIERS: &[&str] = &[$($word),*];

		impl Modifiers {
			fn has(&self, modifier: &str) -> bool {
				match modifier {
					$($word => self.$($field).+ == $value,)*
					_ => unreachable!(),
				}
			}

			fn set(&mut self, modifier: &str) {
				match modifier {
					$($word => self.$($field).+ = $value,)*
					_ => unreachable!(),
				}
			}
		}
	};
}

modifiers! {
	"public" => extras.accessibility = Some(Accessibility::Public),
	"private" => extras.accessibility = Some(Accessibility::Private),
	"protected" => extras.accessibility = Some(Accessibility::Protected),
	"declare" => extras.declare = true,
	"abstract" => extras.is_abstract = true,
	"override" => extras.is_override = true,
	"readonly" => extras.readonly = true,
	"accessor" => accessor = true,
	"static" => extras.is_static = true,
	"in" => is_in = true,
	"out" => is_out = true,
	"const" => is_const = true,
}

impl Parser<'_> {
	pub(crate) fn ts(&mut self, kind: TsKind, start: u32) -> NodeId {
		let index = self.ast.typescript.nodes.len() as u32;
		self.ast.typescript.nodes.push(kind);
		self.add(NodeKind::Extension(index), start)
	}

	fn ts_index(&self, id: NodeId) -> Option<usize> {
		match self.kind(id) {
			NodeKind::Extension(index) => Some(index as usize),
			_ => None,
		}
	}

	fn ts_kind(&self, id: NodeId) -> Option<TsKind> {
		Some(self.ast.typescript.nodes[self.ts_index(id)?])
	}

	fn extras_mut(&mut self, id: NodeId) -> &mut Extras {
		self.ast.typescript.extras.entry(id)
	}

	fn check_parameter_list(&self, params: List, signature: bool) -> Result<()> {
		let mut seen_optional = false;
		for &item in self.ast.list(params).iter().flatten() {
			let param = TypeScript::unwrap(self, item, Unwrap::InnerPattern).unwrap_or(item);
			let binding = match self.kind(param) {
				NodeKind::AssignmentPattern { left, .. } => left,
				_ => param,
			};
			if self.ident_is(binding, "this") {
				continue;
			}
			let optional = self
				.ast
				.typescript
				.extras(binding)
				.is_some_and(|extras| extras.optional);
			let rest = matches!(self.kind(param), NodeKind::RestElement { .. });
			if rest && optional {
				return self.error(self.start_of(param), Code::OptionalRest);
			}
			let initialized = matches!(self.kind(param), NodeKind::AssignmentPattern { .. });
			if optional && initialized {
				return self.error(self.start_of(param), Code::OptionalWithInitializer);
			}
			if optional
				&& !signature
				&& !self.ts.ambient
				&& matches!(
					self.kind(binding),
					NodeKind::ObjectPattern { .. } | NodeKind::ArrayPattern { .. }
				) {
				return self.error(self.start_of(param), Code::OptionalPatternParameter);
			}
			if signature && let Some(default) = self.first_default(param) {
				let at = if default == param { item } else { default };
				return self.error(self.start_of(at), Code::SignatureParameterDefault);
			}
			if seen_optional && !optional && !initialized && !rest {
				return self.error(self.start_of(param), Code::RequiredAfterOptional);
			}
			seen_optional |= optional;
		}
		Ok(())
	}

	fn first_default(&self, pattern: NodeId) -> Option<NodeId> {
		match self.kind(pattern) {
			NodeKind::AssignmentPattern { .. } => Some(pattern),
			NodeKind::ObjectPattern { properties: list } | NodeKind::ArrayPattern { elements: list } => self
				.ast
				.list(list)
				.iter()
				.flatten()
				.find_map(|&element| self.first_default(element)),
			NodeKind::Property { value: inner, .. } | NodeKind::RestElement { argument: inner } => {
				self.first_default(inner)
			}
			_ => None,
		}
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

	/// Re-reads a `<` or `>` left by a type context, where they are single characters; a type
	/// context ends an operand, so the token after it may be the host's.
	fn rescan_lt_gt(&mut self) -> Result<()> {
		if self.is(TokenKind::Lt) || self.is(TokenKind::Gt) {
			self.relex()?;
		}
		self.stop_after_operand();
		Ok(())
	}

	fn parse_decorator(&mut self) -> Result<NodeId> {
		let start = self.tok.start;
		self.next()?;
		self.ts.decorators.push(Vec::new());
		let expression_start = self.tok.start;
		let mut expression = if self.is(TokenKind::ParenL) {
			self.next()?;
			let inner = self.parse_expression(false, &mut None)?;
			self.expect(TokenKind::ParenR)?;
			inner
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
			let args = self.parse_expr_list(TokenKind::ParenR, true, false, &mut None)?;
			let arguments = self.list_from(args);
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
		self.ts.decorators.pop();
		Ok(self.ts(TsKind::Decorator { expression }, start))
	}

	/// Decorators before a class, kept until the class takes them.
	fn parse_decorators(&mut self, allow_export: bool) -> Result<()> {
		if self
			.ts
			.decorators
			.last()
			.is_some_and(|decorators| !decorators.is_empty())
		{
			return self.error(self.tok.start, Code::DecoratorPlacement);
		}
		let mut decorators = Vec::new();
		while self.is(TokenKind::At) {
			decorators.push(self.parse_decorator()?);
		}
		if self.is_keyword(Keyword::Export) && !allow_export {
			return self.unexpected();
		}
		if !self.peek_tokens(|p| p.decorated_class_follows())? {
			return self.error(self.tok.start, Code::DecoratorPlacement);
		}
		self.ts.decorators_before_export = self.is_keyword(Keyword::Export);
		if self.ts.decorators.is_empty() {
			self.ts.decorators.push(Vec::new());
		}
		self.ts.decorators.last_mut().unwrap().extend(decorators);
		Ok(())
	}

	fn decorated_class_follows(&mut self) -> Result<bool> {
		if self.is_keyword(Keyword::Export) {
			self.next_liberal()?;
			if self.is_keyword(Keyword::Default) {
				self.next_liberal()?;
			}
			if self.is(TokenKind::At) {
				return Ok(true);
			}
		}
		while matches!(self.head(), Some(Head::Abstract | Head::Declare)) {
			self.next_liberal()?;
			if self.tok.newline_before {
				return Ok(false);
			}
		}
		Ok(self.is_keyword(Keyword::Class))
	}

	fn take_decorators(&mut self) -> Option<List> {
		let pending = self.ts.decorators.last_mut()?;
		if pending.is_empty() {
			return None;
		}
		let decorators = std::mem::take(pending);
		Some(self.list_of(&decorators))
	}

	fn parameter_decorator_error<T>(&self, decorators: List) -> Result<T> {
		self.error(
			self.start_of(self.ast.list(decorators)[0].unwrap()),
			Code::DecoratorPlacement,
		)
	}

	fn scope_depth(&self) -> usize {
		self.scopes.len() - 1
	}

	fn declare_type(&mut self, id: NodeId, unique: bool) -> Result<()> {
		let NodeKind::Identifier { name } = self.kind(id) else {
			unreachable!()
		};
		let depth = self.scope_depth();
		if unique && self.ts.types.contains_at(depth, name) {
			return self.error_name(self.start_of(id), Code::TypeRedeclaration, name);
		}
		if self.ts.global_depth > 0 {
			self.ts.globals.insert(name);
		}
		self.ts.types.push(depth, name);
		Ok(())
	}

	fn declare_export_only(&mut self, id: NodeId) {
		let NodeKind::Identifier { name } = self.kind(id) else {
			unreachable!()
		};
		let depth = self.scope_depth();
		self.ts.export_only.push(depth, name);
	}

	/// A class member's name the way error messages print it.
	fn element_name(&mut self, element: ElementFrame) -> StrId {
		match element.key {
			Some(key) if !element.computed && matches!(self.kind(key), NodeKind::Identifier { .. }) => {
				let NodeKind::Identifier { name } = self.kind(key) else {
					unreachable!()
				};
				name
			}
			Some(key) => {
				let source = &self.source()[self.start_of(key) as usize..self.end_of(key) as usize];
				self.intern(&format!("[{source}]"))
			}
			None => self.intern(""),
		}
	}

	// Expressions

	/// `const` after `as`, `satisfies` or `<`: a type reference to the word itself.
	fn try_next_parse_constant_context(&mut self) -> Result<Option<NodeId>> {
		if self.peek_token()?.kind != TokenKind::Keyword(Keyword::Const) {
			return Ok(None);
		}
		self.next()?;
		let reference = self.parse_type_reference(true)?;
		if let Some(TsKind::TypeReference {
			type_arguments: Some(_),
			type_name,
		}) = self.ts_kind(reference)
		{
			return self.error(self.start_of(type_name), Code::InvalidConst);
		}
		Ok(Some(reference))
	}

	fn check_const_assertion(&self, expression: NodeId, type_annotation: NodeId) -> Result<()> {
		if !matches!(self.ts_kind(type_annotation), Some(TsKind::TypeReference { type_name, .. }) if self.ident_is(type_name, "const"))
		{
			return Ok(());
		}
		let valid = match self.kind(expression) {
			NodeKind::StringLiteral { .. }
			| NodeKind::NumberLiteral { .. }
			| NodeKind::BigIntLiteral
			| NodeKind::BooleanLiteral { .. }
			| NodeKind::TemplateLiteral { .. }
			| NodeKind::ArrayExpression { .. }
			| NodeKind::ObjectExpression { .. }
			| NodeKind::MemberExpression { .. } => true,
			NodeKind::UnaryExpression {
				operator: UnaryOperator::Plus | UnaryOperator::Minus,
				argument,
			} => matches!(
				self.kind(argument),
				NodeKind::NumberLiteral { .. } | NodeKind::BigIntLiteral
			),
			_ => false,
		};
		if !valid {
			return self.error(self.start_of(expression), Code::ConstAssertionTarget);
		}
		Ok(())
	}

	fn parse_type_assertion(&mut self, for_init: ForInit) -> Result<NodeId> {
		let start = self.tok.start;
		let assertion = self.attempt(|p| {
			p.lexer.depth += 1;
			let type_annotation = match p.try_next_parse_constant_context()? {
				Some(constant) => constant,
				None => p.next_then_parse_type()?,
			};
			p.lexer.depth -= 1;
			p.expect(TokenKind::Gt)?;
			let expression = p.parse_maybe_unary(&mut None, false, false, for_init)?;
			Ok(p.ts(
				TsKind::TypeAssertion {
					type_annotation,
					expression,
				},
				start,
			))
		})?;
		match assertion {
			Some(node) => {
				let Some(TsKind::TypeAssertion {
					expression,
					type_annotation,
				}) = self.ts_kind(node)
				else {
					unreachable!()
				};
				self.check_const_assertion(expression, type_annotation)?;
				Ok(node)
			}
			// not an assertion: the type parameters of a generic arrow, whose parameters must follow
			None => {
				let type_parameters = self.parse_type_parameters(TypeParameterModifiers::Const)?;
				if !self.is(TokenKind::ParenL) {
					return self.unexpected();
				}
				Ok(type_parameters)
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

	// the head alone is tried: trying the whole expression read it again as an assertion or a call, 2x per level
	/// An arrow's head tried at `<` (`generic`) or at a list's `(`.
	fn try_arrow_head(
		&mut self,
		start: u32,
		at: u32,
		is_async: bool,
		for_init: ForInit,
		generic: bool,
	) -> Result<Option<NodeId>> {
		let spine = self.ts.needs_colon_at == Some(start);
		if self.ts.not_arrow.contains(&(at, spine)) {
			return Ok(None);
		}
		let before = spine.then(|| self.snapshot());
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		let mut head = self.attempt(|p| {
			if !generic {
				return Ok((None, p.read_arrow_params(is_async)?.0));
			}
			let type_parameters = p.parse_type_parameters(TypeParameterModifiers::Const)?;
			Ok((Some(type_parameters), p.read_arrow_head(is_async)?.0))
		})?;
		if head.is_some()
			&& self.ts.arrow_return_type.is_some()
			&& let Some(before) = before
			&& !self.colon_after_body(is_async)?
		{
			self.restore(before);
			head = None;
		}
		let Some((type_parameters, params)) = head else {
			self.restore_yield_await(outer);
			self.ts.not_arrow.insert((at, spine));
			return Ok(None);
		};
		// checked after the attempt, which would have read a real parameter error as no arrow
		self.check_arrow_head(&params, is_async, outer)?;
		let arrow = self.parse_arrow_expression(start, params, is_async, for_init)?;
		if type_parameters.is_some() {
			self.extras_mut(arrow).type_parameters = type_parameters;
		}
		Ok(Some(arrow))
	}

	fn colon_after_body(&mut self, is_async: bool) -> Result<bool> {
		let at = self.tok.start;
		if let Some(&colon) = self.ts.colon_after_body.get(&at) {
			return Ok(colon);
		}
		let colon = match self.lookahead(|p| {
			let old = p.take_yield_await();
			p.enter_scope(function_flags(is_async, false) | SCOPE_ARROW);
			p.ts.body_needs_colon = true;
			TypeScript::function_start(p, FunctionKind::Arrow)?;
			let params = p.list_from(Vec::new());
			p.parse_function_body(at, None, params, true, false, ForInit::No)?;
			p.restore_yield_await(old);
			Ok(p.is(TokenKind::Colon))
		}) {
			Err(error) if error.code.is_limit() => return Err(error),
			answer => answer.unwrap_or(false),
		};
		self.ts.colon_after_body.insert(at, colon);
		Ok(colon)
	}

	/// Type arguments after an expression: a call, a tagged template, or an instantiation
	/// expression. `None` means this is not one and the tokenizer must go back.
	fn parse_type_arguments_subscript(
		&mut self,
		base: NodeId,
		start: u32,
		no_calls: bool,
		is_optional_call: bool,
		for_init: ForInit,
	) -> Result<Option<NodeId>> {
		if !no_calls
			&& self.at_possible_async_arrow(base)
			&& let Some(arrow) = self.try_arrow_head(start, self.tok.start, true, for_init, true)?
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
			let quasi = self.parse_template(true)?;
			let node = self.add(NodeKind::TaggedTemplateExpression { tag: base, quasi }, start);
			self.extras_mut(node).type_arguments = Some(type_arguments);
			return Ok(Some(node));
		}
		if !no_calls && self.eat(TokenKind::ParenL)? {
			let mut errors = Some(DestructuringErrors::default());
			let args = self.parse_expr_list(TokenKind::ParenR, true, false, &mut errors)?;
			let arguments = self.list_from(args);
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
}

/// TypeScript's grammar at the points where it departs from JavaScript's, each read where the parser
/// meets one in a TypeScript parse.
// each one cold: without it a JavaScript parse, which calls none of them, ran over 1% slower
impl TypeScript {
	pub(crate) fn save(&self) -> Saved {
		(self.state.clone(), self.types.log.len(), self.export_only.log.len())
	}

	pub(crate) fn restore(&mut self, (state, types, export_only): Saved) {
		self.state = state;
		self.types.truncate(types);
		self.export_only.truncate(export_only);
	}

	// Statements and modules

	/// First look at a statement; `Some` replaces it entirely.
	#[cold]
	pub(crate) fn statement(p: &mut Parser) -> Result<Option<NodeId>> {
		if p.is(TokenKind::At) {
			p.parse_decorators(true)?;
		}
		let start = p.tok.start;
		if (p.head().is_some() || p.is_keyword(Keyword::Const))
			&& let Some(opened) = p.open_declaration()?
		{
			return p.parse_opened(start, opened).map(Some);
		}
		Ok(None)
	}

	/// Right after `export`; `Some` is a whole export statement.
	#[cold]
	pub(crate) fn export_head(p: &mut Parser, start: u32) -> Result<Option<NodeId>> {
		if p.is_keyword(Keyword::Import) {
			p.next()?;
			if p.is_contextual("type") && p.peek_char().0 != Some('=') {
				p.ts.outer_kind = Some(Kind::Type);
				p.next()?;
			} else {
				p.ts.outer_kind = Some(Kind::Value);
			}
			let node = p.parse_import_equals_declaration(start, true)?;
			p.ts.outer_kind = None;
			return Ok(Some(node));
		}
		if p.eat(TokenKind::Eq)? {
			if p.ts.in_namespace {
				return p.error(start, Code::ExportAssignmentInNamespace);
			}
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
			p.ts.outer_kind = Some(Kind::Type);
			p.ts.export_kind = Some(Kind::Type);
		} else {
			p.ts.outer_kind = Some(Kind::Value);
		}
		Ok(None)
	}

	#[cold]
	pub(crate) fn export_declaration(p: &mut Parser) -> Result<Option<NodeId>> {
		if p.is(TokenKind::At) {
			return p.parse_statement(Context::None, StatementPlace::Block, None).map(Some);
		}
		let start = p.tok.start;
		if p.head().is_none_or(|head| head == Head::Global) {
			return Ok(None);
		}
		let Some(opened) = p.open_declaration()? else {
			if p.head() == Some(Head::Declare) && !p.peek_token()?.newline_before {
				p.next()?;
				return p.error(p.tok.start, Code::ExportDeclareWithoutDeclaration);
			}
			return Ok(None);
		};
		let declaration = p.parse_opened(start, opened)?;
		if let Some(TsKind::ModuleDeclaration { global: true, id, .. }) = p.ts_kind(declaration) {
			return p.error(p.start_of(id), Code::ExportDeclareWithoutDeclaration);
		}
		if matches!(
			p.ts_kind(declaration),
			Some(TsKind::InterfaceDeclaration { .. } | TsKind::TypeAliasDeclaration { .. })
		) {
			p.ts.export_kind = Some(Kind::Type);
		}
		Ok(Some(declaration))
	}

	#[cold]
	pub(crate) fn export_default(p: &mut Parser) -> Result<Option<NodeId>> {
		if p.is(TokenKind::At) {
			p.parse_decorators(false)?;
		}
		let start = p.tok.start;
		if p.head() == Some(Head::Abstract) {
			let next = p.peek_token()?;
			if next.kind == TokenKind::Keyword(Keyword::Class) && !next.newline_before {
				p.next()?;
				p.ts.next_class = ClassFrame {
					is_abstract: true,
					start: Some(start),
					..ClassFrame::default()
				};
				return p.parse_class(ClassKind::NullableId).map(Some);
			}
		}
		if p.head() == Some(Head::Interface) {
			p.next()?;
			return p.parse_interface_declaration(start).map(Some);
		}
		Ok(None)
	}

	#[cold]
	pub(crate) fn export_end(p: &mut Parser, node: NodeId) {
		if let Some(kind) = p.ts.export_kind.take() {
			p.extras_mut(node).export_kind = Some(kind);
		}
		p.ts.outer_kind = None;
	}

	#[cold]
	pub(crate) fn export_specifier(p: &mut Parser) -> Result<Option<NodeId>> {
		if p.is_contextual("type") {
			return p.parse_type_only_specifier(false).map(Some);
		}
		Ok(None)
	}

	/// Right after `import`; `Some` is a whole import statement.
	#[cold]
	pub(crate) fn import_head(p: &mut Parser, start: u32) -> Result<Option<NodeId>> {
		p.ts.outer_kind = Some(Kind::Value);
		if p.is_ident() || p.is(TokenKind::Star) || p.is(TokenKind::BraceL) {
			let mut ahead = p.peek_token()?;
			let ahead_is_from = matches!(ahead.kind, TokenKind::Ident(n) if p.str(n) == "from");
			if p.is_contextual("type")
				&& ahead.kind != TokenKind::Comma
				&& !ahead_is_from
				&& ahead.kind != TokenKind::Eq
			{
				p.next()?;
				p.ts.outer_kind = Some(Kind::Type);
				ahead = p.peek_token()?;
			}
			if p.is_ident() && ahead.kind == TokenKind::Eq {
				let node = p.parse_import_equals_declaration(start, false)?;
				p.ts.outer_kind = None;
				return Ok(Some(node));
			}
		}
		Ok(None)
	}

	#[cold]
	pub(crate) fn import_end(p: &mut Parser, node: NodeId) -> Result<()> {
		let kind = p.ts.outer_kind.take().unwrap_or(Kind::Value);
		p.extras_mut(node).import_kind = Some(kind);
		let NodeKind::ImportDeclaration { specifiers, .. } = p.kind(node) else {
			unreachable!()
		};
		if kind == Kind::Type {
			for specifier in p.ast.list(specifiers).to_vec().into_iter().flatten() {
				let (NodeKind::ImportSpecifier { local, .. }
				| NodeKind::ImportDefaultSpecifier { local }
				| NodeKind::ImportNamespaceSpecifier { local }) = p.kind(specifier)
				else {
					continue;
				};
				p.declare_export_only(local);
			}
		}
		if kind == Kind::Type
			&& specifiers.len > 1
			&& matches!(
				p.kind(p.ast.list(specifiers)[0].unwrap()),
				NodeKind::ImportDefaultSpecifier { .. }
			) {
			return p.error(p.start_of(node), Code::TypeImportDefaultAndNamed);
		}
		Ok(())
	}

	#[cold]
	pub(crate) fn import_specifier(p: &mut Parser) -> Result<Option<NodeId>> {
		if p.is_contextual("type") {
			return p.parse_type_only_specifier(true).map(Some);
		}
		Ok(None)
	}

	// a type-only import binds no value: a value may take the name, an export may still name it
	/// How an import's local name binds: `None` for one that binds no value.
	#[cold]
	pub(crate) fn import_binding(p: &Parser) -> crate::parser::scope::Binding {
		use crate::parser::scope::Binding;
		if p.ts.outer_kind == Some(Kind::Type) {
			Binding::None
		} else {
			Binding::Lexical
		}
	}

	/// Whether an `export` here belongs to TypeScript's own scoping, a namespace body say, so the
	/// script goal does not forbid it.
	#[cold]
	pub(crate) fn exports_in_script(p: &Parser) -> bool {
		p.ts.module_blocks > 0
	}

	/// Whether the code describes rather than defines, an ambient declaration's body, where a
	/// definition's rules do not apply.
	#[cold]
	pub(crate) fn in_ambient(p: &Parser) -> bool {
		p.ts.ambient
	}

	/// Whether TypeScript declared `name` in a way that satisfies a local `export { name }`.
	#[cold]
	pub(crate) fn declares_export(p: &mut Parser, name: StrId) -> bool {
		p.ts.module_blocks > 0
			|| p.ts.types.contains(name)
			|| p.ts.export_only.contains(name)
			|| p.ts.globals.contains(&name)
	}

	#[cold]
	pub(crate) fn scope_exit(p: &mut Parser) {
		let depth = p.scope_depth();
		p.ts.types.exit(depth);
		p.ts.export_only.exit(depth);
	}

	// Bindings

	/// After the id of a variable declarator; whether it is definite, `!`.
	#[cold]
	pub(crate) fn var_id(p: &mut Parser, id: NodeId) -> Result<bool> {
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
		Ok(definite)
	}

	#[cold]
	pub(crate) fn var_declarator(p: &mut Parser, node: NodeId, kind: VariableKind, definite: bool) -> Result<()> {
		if definite {
			p.extras_mut(node).definite = true;
			if let NodeKind::VariableDeclarator { init: Some(_), .. } = p.kind(node) {
				return p.error(p.start_of(node), Code::DefiniteWithInitializer);
			}
		}
		if p.ts.ambient {
			p.check_ambient_initializer(node, kind)?;
		}
		Ok(())
	}

	/// Whether a declarator may go without an initializer where JavaScript requires one.
	#[cold]
	pub(crate) fn allows_missing_initializer(p: &mut Parser) -> bool {
		p.ts.ambient
	}

	/// A whole parameter of a parameter list; `allow_modifiers` inside class methods.
	#[cold]
	pub(crate) fn parameter(p: &mut Parser, allow_modifiers: bool) -> Result<Option<NodeId>> {
		let mut decorators = Vec::new();
		while p.is(TokenKind::At) {
			decorators.push(p.parse_decorator()?);
		}
		let decorators = (!decorators.is_empty()).then(|| p.list_of(&decorators));
		let start = p.tok.start;
		let mut property = None;
		if allow_modifiers {
			let extras = p
				.parse_modifiers(
					&["public", "private", "protected", "override", "readonly"],
					&[],
					false,
					None,
				)?
				.extras;
			if extras.accessibility.is_some() || extras.readonly || extras.is_override {
				property = Some(extras);
			}
		}
		let this = p.is_keyword(Keyword::This);
		if !allow_modifiers
			&& !this && let Some(decorators) = decorators
		{
			return p.parameter_decorator_error(decorators);
		}
		let left = if this {
			let this = p.parse_ident(true)?;
			if decorators.is_some() || property.is_some() {
				return p.error(p.start_of(this), Code::ThisParameterModifiers);
			}
			this
		} else if p.is(TokenKind::Ellipsis) {
			p.parse_rest_binding()?
		} else {
			p.parse_binding_atom()?
		};
		let rest = matches!(p.kind(left), NodeKind::RestElement { .. });
		if !this && p.eat(TokenKind::Question)? {
			p.extras_mut(left).optional = true;
		}
		if p.is(TokenKind::Colon) {
			let annotation = p.parse_type_annotation(true, None)?;
			p.extras_mut(left).type_annotation = Some(annotation);
		}
		p.ast.node_mut(left).end = p.prev_end;
		if rest && p.ts.ambient && p.is(TokenKind::Comma) && p.peek_char().0 == Some(')') {
			p.next()?;
		}
		let item = if rest || this {
			left
		} else {
			p.parse_maybe_default(p.start_of(left), Some(left))?
		};
		if p.is(TokenKind::Colon) && item != left {
			return p.error(p.tok.start, Code::TypeAnnotationAfterDefault);
		}
		if decorators.is_some() {
			p.extras_mut(item).decorators = decorators;
		}
		let Some(extras) = property else {
			return Ok(Some(item));
		};
		if !matches!(p.kind(left), NodeKind::Identifier { .. }) {
			return p.error(start, Code::ParameterPropertyPattern);
		}
		let node = p.ts(TsKind::ParameterProperty { parameter: item }, start);
		*p.extras_mut(node) = extras;
		Ok(Some(node))
	}

	#[cold]
	pub(crate) fn catch_param(p: &mut Parser, param: NodeId) -> Result<()> {
		if p.is(TokenKind::Colon) {
			let annotation = p.parse_type_annotation(true, None)?;
			let Some(TsKind::TypeAnnotation { type_annotation }) = p.ts_kind(annotation) else {
				unreachable!()
			};
			if !matches!(
				p.ts_kind(type_annotation),
				Some(TsKind::Keyword(ast::Keyword::Any | ast::Keyword::Unknown))
			) {
				return p.error(p.start_of(type_annotation), Code::CatchClauseType);
			}
			p.extras_mut(param).type_annotation = Some(annotation);
			p.ast.node_mut(param).end = p.prev_end;
		}
		Ok(())
	}

	/// After a pattern read on its own, the `Pattern` entry.
	#[cold]
	pub(crate) fn pattern_annotation(p: &mut Parser, pattern: NodeId) -> Result<()> {
		if p.is(TokenKind::Colon) {
			let annotation = p.parse_type_annotation(true, None)?;
			p.extras_mut(pattern).type_annotation = Some(annotation);
			p.ast.node_mut(pattern).end = p.end_of(annotation);
		}
		Ok(())
	}

	// Functions and classes

	/// Before the parameters of a function, method or arrow.
	#[cold]
	pub(crate) fn function_start(p: &mut Parser, kind: FunctionKind) -> Result<FunctionFrame> {
		let mut frame = FunctionFrame {
			in_class_method: matches!(kind, FunctionKind::Method { in_class: true, .. }),
			..FunctionFrame::default()
		};
		match kind {
			FunctionKind::Arrow => {
				frame.return_type = p.ts.arrow_return_type.take();
				if std::mem::take(&mut p.ts.body_needs_colon) && !p.is(TokenKind::BraceL) {
					p.ts.needs_colon_at = Some(p.tok.start);
				}
			}
			FunctionKind::Method { in_class: true, .. } => {}
			_ => frame.type_parameters = p.try_parse_type_parameters(TypeParameterModifiers::Const)?,
		}
		if matches!(
			kind,
			FunctionKind::Method {
				kind: MethodKind::Get | MethodKind::Set,
				..
			}
		) && let Some(type_parameters) = frame.type_parameters
		{
			return p.error(p.start_of(type_parameters), Code::AccessorTypeParameters);
		}
		Ok(frame)
	}

	/// After the parameters, before the body; `Some` is a function without a body.
	#[cold]
	#[expect(clippy::too_many_arguments)]
	pub(crate) fn function_body(
		p: &mut Parser,
		frame: &mut FunctionFrame,
		start: u32,
		id: Option<NodeId>,
		params: List,
		is_async: bool,
		generator: bool,
		kind: FunctionKind,
	) -> Result<Option<NodeId>> {
		if matches!(
			kind,
			FunctionKind::Method {
				kind: MethodKind::Set,
				..
			}
		) {
			for param in p.ast.list(params).iter().flatten() {
				if p.ident_is(*param, "this") {
					continue;
				}
				if p.ast.typescript.extras(*param).is_some_and(|extras| extras.optional) {
					return p.error(p.start_of(*param), Code::SetterOptionalParameter);
				}
				if matches!(p.kind(*param), NodeKind::AssignmentPattern { .. }) {
					return p.error(p.start_of(*param), Code::SetterParameterInitializer);
				}
			}
		}
		if p.is(TokenKind::Colon) {
			let allow_predicate = !matches!(
				kind,
				FunctionKind::Method {
					kind: MethodKind::Get,
					..
				}
			);
			let return_type = p.parse_type_or_type_predicate_annotation(TokenKind::Colon, allow_predicate)?;
			match kind {
				FunctionKind::Method {
					kind: MethodKind::Set, ..
				} => {
					return p.error(p.start_of(return_type), Code::SetterReturnType);
				}
				FunctionKind::Method {
					kind: MethodKind::Constructor,
					..
				} => {
					return p.error(p.start_of(return_type), Code::ConstructorReturnType);
				}
				_ => {}
			}
			frame.return_type = Some(return_type);
		}
		let bodiless = match kind {
			FunctionKind::Declaration => Some(TsKind::DeclareFunction {
				id,
				params,
				is_async,
				generator,
			}),
			FunctionKind::Method { in_class: true, .. } => Some(TsKind::DeclareMethod {
				params,
				is_async,
				generator,
			}),
			_ => None,
		};
		if generator && p.ts.ambient && bodiless.is_some() {
			return p.error(start, Code::GeneratorInAmbient);
		}
		let signature = bodiless.is_some() && !p.is(TokenKind::BraceL) && p.is_line_terminator()?;
		p.check_parameter_list(params, signature)?;
		if signature && let Some(kind) = bodiless {
			if generator {
				return p.error(start, Code::GeneratorSignature);
			}
			return Ok(Some(p.ts(kind, start)));
		}
		if p.ts.ambient && bodiless.is_some() {
			return p.error(start, Code::ImplementationInAmbient);
		}
		Ok(None)
	}

	#[cold]
	pub(crate) fn function_end(p: &mut Parser, node: NodeId, frame: FunctionFrame) -> Result<()> {
		if let NodeKind::ArrowFunctionExpression { params, .. } = p.kind(node) {
			p.check_parameter_list(params, false)?;
		}
		if let Some(TsKind::DeclareFunction { id: Some(id), .. }) = p.ts_kind(node) {
			p.declare_export_only(id);
		}
		if frame.type_parameters.is_some() || frame.return_type.is_some() {
			let extras = p.extras_mut(node);
			if frame.type_parameters.is_some() {
				extras.type_parameters = frame.type_parameters;
			}
			extras.return_type = frame.return_type;
		}
		if frame.in_class_method
			&& matches!(p.kind(node), NodeKind::FunctionExpression { .. })
			&& let Some(element) = p.ts.elements.last()
			&& element.extras.is_abstract
		{
			let (start, element) = (element.start, *element);
			let name = p.element_name(element);
			return p.error_name(start, Code::AbstractWithImplementation, name);
		}
		Ok(())
	}

	/// Whether an object literal accessor's first parameter is a `this` parameter, which does not
	/// count.
	#[cold]
	pub(crate) fn accessor_this_param(p: &Parser, params: List) -> bool {
		p.ast
			.list(params)
			.first()
			.is_some_and(|param| param.is_some_and(|param| p.ident_is(param, "this")))
	}

	/// The parameters of a function-shaped TypeScript node.
	#[cold]
	pub(crate) fn function_params(p: &Parser, node: NodeId) -> Option<List> {
		match p.ts_kind(node) {
			Some(TsKind::DeclareFunction { params, .. } | TsKind::DeclareMethod { params, .. }) => Some(params),
			_ => None,
		}
	}

	#[cold]
	pub(crate) fn class_start(p: &mut Parser, kind: ClassKind) -> Result<()> {
		let mut frame = std::mem::take(&mut p.ts.next_class);
		frame.is_declaration = kind != ClassKind::Expression;
		if let Some(decorators) = p.take_decorators() {
			frame.decorators = Some(decorators);
			if !std::mem::take(&mut p.ts.decorators_before_export) {
				frame.start = Some(p.start_of(p.ast.list(decorators)[0].unwrap()));
			}
		}
		frame.outer_abstract = p.ts.in_abstract_class;
		p.ts.in_abstract_class = frame.is_abstract;
		p.ts.classes.push(frame);
		Ok(())
	}

	/// Whether the token after `class` opens a heritage clause rather than naming the class.
	#[cold]
	pub(crate) fn starts_class_heritage(p: &mut Parser) -> bool {
		p.is_contextual("implements")
	}

	#[cold]
	pub(crate) fn class_type_parameters(p: &mut Parser) -> Result<()> {
		let type_parameters = p.try_parse_type_parameters(TypeParameterModifiers::Class)?;
		p.ts.classes.last_mut().unwrap().type_parameters = type_parameters;
		Ok(())
	}

	#[cold]
	pub(crate) fn class_heritage(p: &mut Parser, has_super: bool) -> Result<()> {
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
		let frame = p.ts.classes.last_mut().unwrap();
		frame.has_super = has_super;
		frame.super_type_arguments = super_type_arguments;
		frame.implements = implements;
		Ok(())
	}

	#[cold]
	pub(crate) fn class_end(p: &mut Parser, node: NodeId) {
		let frame = p.ts.classes.pop().unwrap();
		p.ts.in_abstract_class = frame.outer_abstract;
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

	/// Modifiers before a class element; returns whether `static` was among them.
	#[cold]
	pub(crate) fn class_modifiers(p: &mut Parser) -> Result<bool> {
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
			Some(Code::TypeParameterModifier),
		)?;
		if !decorators.is_empty() {
			if p.is(TokenKind::BraceR) {
				return p.error(p.tok.end, Code::DecoratorPlacement);
			}
			modifiers.extras.decorators = Some(p.list_of(&decorators));
		}
		if modifiers != Modifiers::default() && p.is_contextual("static") && p.peek_char().0 == Some('{') {
			let snapshot = p.token_snapshot();
			p.next()?;
			p.next()?;
			let inside = p.tok.start;
			p.restore_tokens(snapshot);
			return p.error(inside, Code::StaticBlockModifier);
		}
		let outer_ambient = modifiers
			.extras
			.declare
			.then(|| std::mem::replace(&mut p.ts.ambient, true));
		p.ts.elements.push(ElementFrame {
			start,
			extras: modifiers.extras,
			key: None,
			computed: false,
			outer_ambient,
			accessor: modifiers.accessor,
		});
		Ok(modifiers.extras.is_static)
	}

	/// A type parameter list `<...>` at the cursor, for the entry that parses one on its own.
	#[cold]
	pub(crate) fn type_parameters(p: &mut Parser) -> Result<NodeId> {
		p.parse_type_parameters(TypeParameterModifiers::Const)
	}

	#[cold]
	pub(crate) fn class_index_signature(p: &mut Parser, start: u32) -> Result<Option<NodeId>> {
		let Some(signature) = p.try_parse_index_signature(start)? else {
			let has_super = p.ts.classes.last().is_some_and(|c| c.has_super);
			let extras = p.ts.elements.last().unwrap().extras;
			if !p.ts.in_abstract_class && extras.is_abstract {
				return p.error(start, Code::AbstractOutsideAbstractClass);
			}
			if extras.is_override && !has_super {
				return p.error(start, Code::OverrideWithoutExtends);
			}
			return Ok(None);
		};
		let ElementFrame { extras, accessor, .. } = *p.ts.elements.last().unwrap();
		if extras.is_abstract {
			return p.error_arg(start, Code::IndexSignatureModifier, "abstract");
		}
		if let Some(accessibility) = extras.accessibility {
			let accessibility = p.intern(accessibility.as_str());
			return p.raise(SyntaxError::new(start, Code::IndexSignatureModifier).arg(accessibility));
		}
		if extras.declare {
			return p.error_arg(start, Code::IndexSignatureModifier, "declare");
		}
		if extras.is_override {
			return p.error_arg(start, Code::IndexSignatureModifier, "override");
		}
		if accessor {
			return p.error_arg(start, Code::IndexSignatureModifier, "accessor");
		}
		Ok(Some(signature))
	}

	/// After the key of a class element.
	#[cold]
	pub(crate) fn class_key_end(p: &mut Parser, key: NodeId, computed: bool) -> Result<()> {
		let element = p.ts.elements.last_mut().unwrap();
		element.key = Some(key);
		element.computed = computed;
		let start = element.start;
		let extras = element.extras;
		let accessor = element.accessor;
		if p.eat(TokenKind::Question)? {
			p.ts.elements.last_mut().unwrap().extras.optional = true;
		}
		let method = p.is(TokenKind::ParenL) || p.is(TokenKind::Lt);
		if extras.readonly && method {
			return p.error(start, Code::ReadonlyOnMethod);
		}
		if extras.declare && method {
			return p.error(start, Code::DeclareOnMethod);
		}
		if accessor && method {
			return p.error(start, Code::AccessorPlacement);
		}
		Ok(())
	}

	#[cold]
	pub(crate) fn starts_class_method(p: &mut Parser) -> bool {
		p.is(TokenKind::Lt)
	}

	#[cold]
	pub(crate) fn class_method_start(p: &mut Parser, kind: MethodKind) -> Result<()> {
		let type_parameters = p.try_parse_type_parameters(TypeParameterModifiers::Const)?;
		if matches!(kind, MethodKind::Get | MethodKind::Set)
			&& let Some(type_parameters) = type_parameters
		{
			return p.error(p.start_of(type_parameters), Code::AccessorTypeParameters);
		}
		let element = *p.ts.elements.last().unwrap();
		let key = element.key.unwrap();
		if matches!(p.kind(key), NodeKind::PrivateIdentifier { .. }) {
			if let Some(accessibility) = element.extras.accessibility {
				let accessibility = p.intern(accessibility.as_str());
				return p.raise(SyntaxError::new(element.start, Code::PrivateModifier).arg(accessibility));
			}
		} else if let Some(type_parameters) = type_parameters
			&& !element.extras.is_static
			&& !element.computed
			&& matches!(p.kind(key), NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } if p.str(name) == "constructor")
		{
			return p.error(p.start_of(type_parameters), Code::ConstructorTypeParameters);
		}
		p.ts.elements.last_mut().unwrap().extras.type_parameters = type_parameters;
		Ok(())
	}

	/// After the key of a class field, before its initializer.
	#[cold]
	pub(crate) fn class_field_annotation(p: &mut Parser) -> Result<()> {
		let element = *p.ts.elements.last().unwrap();
		let mut definite = false;
		let mut optional = element.extras.optional;
		if !optional {
			if p.is(TokenKind::Bang) && !p.tok.newline_before {
				p.next()?;
				definite = true;
			} else {
				optional = p.eat(TokenKind::Question)?;
			}
		}
		let type_annotation = p.try_parse_type_annotation()?;
		let extras = &mut p.ts.elements.last_mut().unwrap().extras;
		extras.definite = definite;
		extras.optional = optional;
		extras.type_annotation = type_annotation;
		let private = element
			.key
			.is_some_and(|key| matches!(p.kind(key), NodeKind::PrivateIdentifier { .. }));
		if private {
			if element.extras.is_abstract {
				return p.error_arg(element.start, Code::PrivateModifier, "abstract");
			}
			if let Some(accessibility) = element.extras.accessibility {
				let accessibility = p.intern(accessibility.as_str());
				return p.raise(SyntaxError::new(element.start, Code::PrivateModifier).arg(accessibility));
			}
		} else if p.is(TokenKind::Eq) {
			if p.ts.ambient && !(element.extras.readonly && type_annotation.is_none()) {
				return p.error(p.tok.start, Code::InitializerInAmbient);
			}
			if element.extras.is_abstract {
				let name = p.element_name(element);
				return p.error_name(p.tok.start, Code::AbstractWithInitializer, name);
			}
		}
		Ok(())
	}

	#[cold]
	pub(crate) fn class_element_end(p: &mut Parser, node: NodeId) -> Result<()> {
		let frame = p.ts.elements.pop().unwrap();
		if let Some(outer) = frame.outer_ambient {
			p.ts.ambient = outer;
		}
		if frame.accessor
			&& let NodeKind::PropertyDefinition { kind, .. } = &mut p.ast.node_mut(node).kind
		{
			*kind = FieldKind::Accessor;
		}
		if let NodeKind::MethodDefinition {
			key,
			kind,
			computed: false,
			..
		} = p.kind(node)
			&& matches!(kind, MethodKind::Constructor | MethodKind::Method)
			&& matches!(p.kind(key), NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } if p.str(name) == "constructor")
		{
			if frame.extras.is_abstract {
				return p.error(frame.start, Code::AbstractPlacement);
			}
			let modifier = if frame.extras.is_static {
				Some("static")
			} else if frame.extras.is_override {
				Some("override")
			} else {
				None
			};
			if let Some(modifier) = modifier {
				return p.error_arg(frame.start, Code::ConstructorModifier, modifier);
			}
		}
		if let Some(decorators) = frame.extras.decorators {
			let start = p.start_of(p.ast.list(decorators)[0].unwrap());
			p.ast.node_mut(node).start = start;
			if let NodeKind::MethodDefinition { kind, value, .. } = p.kind(node) {
				if kind == MethodKind::Constructor {
					return p.error(start, Code::DecoratorOnConstructor);
				}
				if matches!(p.ts_kind(value), Some(TsKind::DeclareMethod { .. })) {
					return p.error(start, Code::DecoratorWithoutBody);
				}
			}
			if matches!(p.ts_kind(node), Some(TsKind::IndexSignature { .. })) {
				return p.error(start, Code::DecoratorPlacement);
			}
		}
		if let NodeKind::MethodDefinition { kind, value, .. } = p.kind(node) {
			let (params, has_body) = match p.kind(value) {
				NodeKind::FunctionExpression { function } => (function.params, true),
				_ => (Self::function_params(p, value).unwrap(), false),
			};
			if !has_body || kind == MethodKind::Get || !p.ts.classes.last().unwrap().is_declaration {
				for &param in p.ast.list(params).iter().flatten() {
					let param = Self::unwrap(p, param, Unwrap::InnerPattern).unwrap_or(param);
					if let Some(decorators) = p.ast.typescript.extras(param).and_then(|extras| extras.decorators) {
						return p.parameter_decorator_error(decorators);
					}
				}
			}
		}
		let mut extras = frame.extras;
		extras.is_static &= matches!(p.kind(node), NodeKind::Extension(_));
		if extras != Extras::default() {
			*p.extras_mut(node) = extras;
		}
		Ok(())
	}

	// Expressions

	#[cold]
	pub(crate) fn maybe_assign(p: &mut Parser, for_init: ForInit, _: &mut Errors) -> Result<Option<NodeId>> {
		if !p.starts_with_lt() {
			return Ok(None);
		}
		p.try_arrow_head(p.tok.start, p.tok.start, false, for_init, true)
	}

	/// At the `(` of a list that a type must be read to decide, or that the closer reader could
	/// not; `Some` is the arrow it starts.
	#[cold]
	pub(crate) fn arrow_head(
		p: &mut Parser,
		start: u32,
		at: u32,
		is_async: bool,
		for_init: ForInit,
	) -> Result<Option<NodeId>> {
		p.try_arrow_head(start, at, is_async, for_init, false)
	}

	/// After an arrow's parameters, before `=>`.
	#[cold]
	pub(crate) fn arrow_return_type(p: &mut Parser) -> Result<()> {
		p.ts.arrow_return_type = if p.is(TokenKind::Colon) {
			Some(p.parse_type_or_type_predicate_annotation(TokenKind::Colon, true)?)
		} else {
			None
		};
		Ok(())
	}

	#[cold]
	pub(crate) fn arrow_start(p: &mut Parser, start: u32) {
		p.ts.body_needs_colon = p.ts.needs_colon_at == Some(start);
	}

	#[cold]
	pub(crate) fn conditional_branch(p: &mut Parser, start: u32, alternate: bool, for_init: ForInit) -> Result<NodeId> {
		let old = p.ts.needs_colon_at;
		if !alternate || old == Some(start) {
			p.ts.needs_colon_at = Some(p.tok.start);
		}
		let branch = p.parse_maybe_assign(if alternate { for_init } else { ForInit::No }, &mut None);
		p.ts.needs_colon_at = old;
		branch
	}

	#[cold]
	pub(crate) fn unary(p: &mut Parser, for_init: ForInit) -> Result<Option<NodeId>> {
		if p.starts_with_lt() {
			return p.parse_type_assertion(for_init).map(Some);
		}
		Ok(None)
	}

	/// First look at an atom.
	#[cold]
	pub(crate) fn atom(p: &mut Parser, errors: &mut Errors, for_init: ForInit, for_new: bool) -> Result<NodeId> {
		p.parse_decorators(false)?;
		p.parse_expr_atom(errors, for_init, for_new)
	}

	/// Whether the current token, one of the host's stop tokens, is one TypeScript reads after an
	/// expression; it then decides where the host's syntax starts.
	#[cold]
	pub(crate) fn reads_stop(p: &Parser) -> bool {
		(p.is_contextual("as") || p.is_contextual("satisfies")) && p.forced_stop != Some(p.tok.start)
	}

	/// At an operator position; `Some` is the new left operand.
	#[cold]
	pub(crate) fn expr_op(p: &mut Parser, left: NodeId, left_start: u32) -> Result<Option<NodeId>> {
		let is_as = p.is_contextual("as");
		if !is_as && !p.is_contextual("satisfies") {
			return Ok(None);
		}
		if p.tok.stop {
			if p.forced_stop == Some(p.tok.start) {
				p.stop_here();
				return Ok(None);
			}
			// read as the assertion for now; the entry decides afterwards which one was the host's
			p.stop_word_at = Some(p.tok.start);
		}
		assertion(p, left, left_start, is_as).map(Some)
	}

	#[cold]
	pub(crate) fn subscript(
		p: &mut Parser,
		base: NodeId,
		start: u32,
		no_calls: bool,
		_maybe_async_arrow: bool,
		optional_chained: bool,
		for_init: ForInit,
	) -> Result<Option<(NodeId, bool)>> {
		if p.is(TokenKind::Bang) {
			if p.tok.newline_before {
				return Ok(None);
			}
			p.next()?;
			p.stop_after_operand();
			let node = p.ts(TsKind::NonNullExpression { expression: base }, start);
			return Ok(Some((node, false)));
		}
		let optional_call = p.is(TokenKind::QuestionDot);
		if optional_call && p.peek_char().0 != Some('<') {
			return Ok(None);
		}
		if optional_call && no_calls {
			return Ok(Some((base, false)));
		}
		let subscript = p.attempt(|p| {
			if optional_call {
				p.next()?;
			}
			match p.parse_type_arguments_subscript(base, start, no_calls, optional_call, for_init)? {
				Some(node) => Ok((node, optional_call)),
				None => p.unexpected(),
			}
		})?;
		let Some((node, is_optional_call)) = subscript else {
			return Ok(None);
		};
		// raised inside the attempt, the error read `<T>` as comparisons instead
		if optional_chained && let NodeKind::TaggedTemplateExpression { quasi, .. } = p.kind(node) {
			let at = p.start_of(quasi);
			return Err(Box::new(
				SyntaxError::new(at, Code::OptionalChainInTaggedTemplate).to(at + 1),
			));
		}
		if matches!(p.ts_kind(node), Some(TsKind::InstantiationExpression { .. }))
			&& (p.is(TokenKind::Dot) || (p.is(TokenKind::QuestionDot) && p.peek_char().0 != Some('(')))
		{
			return p.error(p.tok.start, Code::PropertyAfterInstantiation);
		}
		Ok(Some((node, is_optional_call)))
	}

	#[cold]
	pub(crate) fn new_expression(p: &mut Parser, node: NodeId) {
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

	/// An object property whose value starts unexpectedly for JavaScript.
	#[cold]
	pub(crate) fn property_value(
		p: &mut Parser,
		start: u32,
		key: NodeId,
		computed: bool,
		is_pattern: bool,
		generator: bool,
		is_async: bool,
	) -> Result<NodeId> {
		if is_pattern {
			return p.unexpected();
		}
		let value = p.parse_method(generator, is_async, false, false, MethodKind::Method)?;
		Ok(p.add(
			NodeKind::Property {
				key,
				value,
				kind: crate::ast::PropertyKind::Init,
				computed,
				method: true,
				shorthand: false,
			},
			start,
		))
	}

	#[cold]
	pub(crate) fn template_expression(p: &mut Parser) -> Result<NodeId> {
		p.parse_type()
	}

	/// Reinterprets a TypeScript node as a pattern; the result replaces it.
	#[cold]
	pub(crate) fn make_pattern(p: &mut Parser, id: NodeId, errors: &mut Errors) -> Result<Option<NodeId>> {
		match p.ts_kind(id) {
			Some(
				TsKind::AsExpression { expression, .. }
				| TsKind::SatisfiesExpression { expression, .. }
				| TsKind::NonNullExpression { expression }
				| TsKind::TypeAssertion { expression, .. },
			) => p.make_pattern(expression, errors).map(Some),
			_ => Ok(None),
		}
	}

	/// The plain node a TypeScript wrapper stands for in a check, if any.
	#[cold]
	pub(crate) fn unwrap(p: &Parser, id: NodeId, context: Unwrap) -> Option<NodeId> {
		match (context, p.ts_kind(id)?) {
			(Unwrap::Simple, TsKind::NonNullExpression { expression } | TsKind::AsExpression { expression, .. }) => {
				Some(expression)
			}
			(Unwrap::InnerPattern, TsKind::ParameterProperty { parameter: inner }) => Some(inner),
			_ => None,
		}
	}
}

/// `left as T` or `left satisfies T`, the word being the current token.
fn assertion(p: &mut Parser, left: NodeId, left_start: u32, is_as: bool) -> Result<NodeId> {
	let type_annotation = match p.try_next_parse_constant_context()? {
		Some(constant) => constant,
		None => p.next_then_parse_type()?,
	};
	let kind = if is_as {
		p.check_const_assertion(left, type_annotation)?;
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
	Ok(node)
}
