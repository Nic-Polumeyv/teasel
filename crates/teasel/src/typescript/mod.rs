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
use crate::parser::scope::{EXPORT_ONLY, SCOPE_ARROW, TYPE, function_flags};
use crate::parser::statement::{ClassKind, StatementPlace};
use crate::parser::{Context, DestructuringErrors, Errors, ForInit, FunctionKind, Parser, Result, Unwrap};
use ast::{Accessibility, Extras, Kind, TsKind};
use declarations::Head;
use types::TypeParameterModifiers;

/// Parser state that only TypeScript needs. `State` is copied into every snapshot, so it stays
/// small.
#[derive(Default)]
pub(crate) struct TypeScript {
	state: State,
	// outside State so a rewind keeps them: nested `<A>(<A>(…))` and ternary arrows were read twice per level
	not_arrow: crate::interner::FastSet<(u32, bool)>,
	colon_after_body: FastMap<u32, bool>,
}

#[derive(Clone, Default)]
pub(crate) struct State {
	disallow_conditional_types: bool,
	infer_depth: u32,
	ambient: bool,
	/// The innermost module body is a namespace's, not a quoted module's or global's.
	in_namespace: bool,
	/// Inside `declare global` or a `global` block: what is declared there is the module's to export.
	global_depth: u32,
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

impl<const TS: bool> Parser<'_, TS> {
	pub(crate) fn ts(&mut self, kind: TsKind, start: u32) -> NodeId {
		let index = self.ast.typescript.nodes.len() as u32;
		self.ast.typescript.nodes.push(kind);
		self.add(NodeKind::Extension(index), start)
	}

	fn ts_index(&self, id: NodeId) -> Option<usize> {
		match self.kind(id) {
			NodeKind::Extension(index) if self.typescript() => Some(index as usize),
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
			let param = self.unwrap(item, Unwrap::InnerPattern).unwrap_or(item);
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
				&& !self.ts.state.ambient
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
	pub(crate) fn starts_with_lt(&mut self) -> bool {
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
		self.ts.state.decorators.push(Vec::new());
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
			let mut facts = Some(DestructuringErrors::default());
			let args = self.parse_expr_list(TokenKind::ParenR, true, false, &mut facts)?;
			self.check_expression_errors(&facts, true)?;
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
		self.ts.state.decorators.pop();
		Ok(self.ts(TsKind::Decorator { expression }, start))
	}

	/// Decorators before a class, kept until the class takes them.
	#[cold]
	pub(crate) fn parse_decorators(&mut self, allow_export: bool) -> Result<()> {
		if self
			.ts
			.state
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
		self.ts.state.decorators_before_export = self.is_keyword(Keyword::Export);
		if self.ts.state.decorators.is_empty() {
			self.ts.state.decorators.push(Vec::new());
		}
		self.ts.state.decorators.last_mut().unwrap().extend(decorators);
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
		let pending = self.ts.state.decorators.last_mut()?;
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

	fn declare_type(&mut self, id: NodeId, unique: bool) -> Result<()> {
		let NodeKind::Identifier { name } = self.kind(id) else {
			unreachable!()
		};
		if unique && self.current_scope().has(name, TYPE) {
			return self.error_name(self.start_of(id), Code::TypeRedeclaration, name);
		}
		if self.ts.state.global_depth > 0 {
			self.declare_global(name);
		}
		self.declare_here(name, TYPE);
		Ok(())
	}

	fn declare_export_only(&mut self, id: NodeId) {
		let NodeKind::Identifier { name } = self.kind(id) else {
			unreachable!()
		};
		self.declare_here(name, EXPORT_ONLY);
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

	#[cold]
	pub(crate) fn parse_type_assertion(&mut self, for_init: ForInit) -> Result<NodeId> {
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
	// not cold: every list the tokens leave undecided is tried here, and marked cold the app documents
	// took 1% longer
	/// An arrow's head tried at `<` (`generic`) or after a list's `(` at `at`: a list a type must be
	/// read to decide, the `:` after its closer at `colon`, or one the closer reader could not.
	pub(crate) fn try_arrow_head(
		&mut self,
		start: u32,
		at: u32,
		colon: Option<u32>,
		is_async: bool,
		for_init: ForInit,
		generic: bool,
	) -> Result<Option<NodeId>> {
		let spine = self.ts.state.needs_colon_at == Some(start);
		if self.ts.not_arrow.contains(&(at, spine)) {
			return Ok(None);
		}
		// the list read first and again after the type or the body failed: a nest of `c ? (a = …) : d`
		// read its inner lists once per level, 128 levels in 16 ms
		if let Some(colon) = colon
			&& !self.arrow_after_type(colon, is_async, spine)?
		{
			self.ts.not_arrow.insert((at, spine));
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
			&& self.ts.state.arrow_return_type.is_some()
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

	/// Whether a type and `=>` follow the `:` at `colon`, read without the list before it, and on a
	/// conditional's `spine` a `:` after the arrow's body.
	fn arrow_after_type(&mut self, colon: u32, is_async: bool, spine: bool) -> Result<bool> {
		match self.lookahead(|p| {
			p.lexer.set_pos(colon);
			p.lexer.next_token_into(&mut p.tok)?;
			p.parse_type_or_type_predicate_annotation(TokenKind::Colon, true)?;
			if p.can_insert_semicolon() || !p.eat(TokenKind::Arrow)? {
				return Ok(false);
			}
			Ok(!spine || p.colon_after_body(is_async)?)
		}) {
			Err(error) if error.code.is_limit() => Err(error),
			answer => Ok(answer.unwrap_or(false)),
		}
	}

	fn colon_after_body(&mut self, is_async: bool) -> Result<bool> {
		let at = self.tok.start;
		if let Some(&colon) = self.ts.colon_after_body.get(&at) {
			return Ok(colon);
		}
		let colon = match self.lookahead(|p| {
			let old = p.take_yield_await();
			p.enter_scope(function_flags(is_async, false) | SCOPE_ARROW);
			p.ts.state.body_needs_colon = true;
			p.function_start(FunctionKind::Arrow)?;
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
			&& let Some(arrow) = self.try_arrow_head(start, self.tok.start, None, true, for_init, true)?
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

impl TypeScript {
	pub(crate) fn save(&self) -> State {
		self.state.clone()
	}

	pub(crate) fn restore(&mut self, state: State) {
		self.state = state;
	}
}

/// TypeScript's grammar at the points where it departs from JavaScript's, each read where the parser
/// meets one in a TypeScript parse.
// cold where the hottest rules call in: without it JavaScript read as TypeScript took 1 to 2% longer
impl<const TS: bool> Parser<'_, TS> {
	// Statements and modules

	/// First look at a statement; `Some` replaces it entirely.
	#[cold]
	pub(crate) fn statement(&mut self) -> Result<Option<NodeId>> {
		if self.is(TokenKind::At) {
			self.parse_decorators(true)?;
		}
		let start = self.tok.start;
		if (self.head().is_some() || self.is_keyword(Keyword::Const))
			&& let Some(opened) = self.open_declaration()?
		{
			return self.parse_opened(start, opened).map(Some);
		}
		Ok(None)
	}

	/// Right after `export`; `Some` is a whole export statement.
	pub(crate) fn export_head(&mut self, start: u32) -> Result<Option<NodeId>> {
		if self.is_keyword(Keyword::Import) {
			self.next()?;
			if self.is_contextual("type") && self.peek_char().0 != Some('=') {
				self.ts.state.outer_kind = Some(Kind::Type);
				self.next()?;
			} else {
				self.ts.state.outer_kind = Some(Kind::Value);
			}
			let node = self.parse_import_equals_declaration(start, true)?;
			self.ts.state.outer_kind = None;
			return Ok(Some(node));
		}
		if self.eat(TokenKind::Eq)? {
			if self.ts.state.in_namespace {
				return self.error(start, Code::ExportAssignmentInNamespace);
			}
			let expression = self.parse_expression(false, &mut None)?;
			self.semicolon()?;
			return Ok(Some(self.ts(TsKind::ExportAssignment { expression }, start)));
		}
		if self.eat_contextual("as")? {
			self.expect_contextual("namespace")?;
			let id = self.parse_ident(false)?;
			self.semicolon()?;
			return Ok(Some(self.ts(TsKind::NamespaceExportDeclaration { id }, start)));
		}
		if self.is_contextual("type") && matches!(self.peek_token()?.kind, TokenKind::BraceL | TokenKind::Star) {
			self.next()?;
			self.ts.state.outer_kind = Some(Kind::Type);
			self.ts.state.export_kind = Some(Kind::Type);
		} else {
			self.ts.state.outer_kind = Some(Kind::Value);
		}
		Ok(None)
	}

	pub(crate) fn export_declaration(&mut self) -> Result<Option<NodeId>> {
		if self.is(TokenKind::At) {
			return self
				.parse_statement(Context::None, StatementPlace::Block, None)
				.map(Some);
		}
		let start = self.tok.start;
		if self.head().is_none_or(|head| head == Head::Global) {
			return Ok(None);
		}
		let Some(opened) = self.open_declaration()? else {
			if self.head() == Some(Head::Declare) && !self.peek_token()?.newline_before {
				self.next()?;
				return self.error(self.tok.start, Code::ExportDeclareWithoutDeclaration);
			}
			return Ok(None);
		};
		let declaration = self.parse_opened(start, opened)?;
		if let Some(TsKind::ModuleDeclaration { global: true, id, .. }) = self.ts_kind(declaration) {
			return self.error(self.start_of(id), Code::ExportDeclareWithoutDeclaration);
		}
		if matches!(
			self.ts_kind(declaration),
			Some(TsKind::InterfaceDeclaration { .. } | TsKind::TypeAliasDeclaration { .. })
		) {
			self.ts.state.export_kind = Some(Kind::Type);
		}
		Ok(Some(declaration))
	}

	pub(crate) fn export_default(&mut self) -> Result<Option<NodeId>> {
		if self.is(TokenKind::At) {
			self.parse_decorators(false)?;
		}
		let start = self.tok.start;
		if self.head() == Some(Head::Abstract) {
			let next = self.peek_token()?;
			if next.kind == TokenKind::Keyword(Keyword::Class) && !next.newline_before {
				self.next()?;
				self.ts.state.next_class = ClassFrame {
					is_abstract: true,
					start: Some(start),
					..ClassFrame::default()
				};
				return self.parse_class(ClassKind::NullableId).map(Some);
			}
		}
		if self.head() == Some(Head::Interface) {
			self.next()?;
			return self.parse_interface_declaration(start).map(Some);
		}
		Ok(None)
	}

	pub(crate) fn export_end(&mut self, node: NodeId) {
		if let Some(kind) = self.ts.state.export_kind.take() {
			self.extras_mut(node).export_kind = Some(kind);
		}
		self.ts.state.outer_kind = None;
	}

	pub(crate) fn export_specifier(&mut self) -> Result<Option<NodeId>> {
		if self.is_contextual("type") {
			return self.parse_type_only_specifier(false).map(Some);
		}
		Ok(None)
	}

	/// Right after `import`; `Some` is a whole import statement.
	pub(crate) fn import_head(&mut self, start: u32) -> Result<Option<NodeId>> {
		self.ts.state.outer_kind = Some(Kind::Value);
		if self.is_ident() || self.is(TokenKind::Star) || self.is(TokenKind::BraceL) {
			let mut ahead = self.peek_token()?;
			let ahead_is_from = matches!(ahead.kind, TokenKind::Ident(n) if self.str(n) == "from");
			if self.is_contextual("type")
				&& ahead.kind != TokenKind::Comma
				&& !ahead_is_from
				&& ahead.kind != TokenKind::Eq
			{
				self.next()?;
				self.ts.state.outer_kind = Some(Kind::Type);
				ahead = self.peek_token()?;
			}
			if self.is_ident() && ahead.kind == TokenKind::Eq {
				let node = self.parse_import_equals_declaration(start, false)?;
				self.ts.state.outer_kind = None;
				return Ok(Some(node));
			}
		}
		Ok(None)
	}

	pub(crate) fn import_end(&mut self, node: NodeId) -> Result<()> {
		let kind = self.ts.state.outer_kind.take().unwrap_or(Kind::Value);
		self.extras_mut(node).import_kind = Some(kind);
		let NodeKind::ImportDeclaration { specifiers, .. } = self.kind(node) else {
			unreachable!()
		};
		if kind == Kind::Type {
			for specifier in self.ast.list(specifiers).to_vec().into_iter().flatten() {
				let (NodeKind::ImportSpecifier { local, .. }
				| NodeKind::ImportDefaultSpecifier { local }
				| NodeKind::ImportNamespaceSpecifier { local }) = self.kind(specifier)
				else {
					continue;
				};
				self.declare_export_only(local);
			}
		}
		if kind == Kind::Type
			&& specifiers.len > 1
			&& matches!(
				self.kind(self.ast.list(specifiers)[0].unwrap()),
				NodeKind::ImportDefaultSpecifier { .. }
			) {
			return self.error(self.start_of(node), Code::TypeImportDefaultAndNamed);
		}
		Ok(())
	}

	pub(crate) fn import_specifier(&mut self) -> Result<Option<NodeId>> {
		if self.is_contextual("type") {
			return self.parse_type_only_specifier(true).map(Some);
		}
		Ok(None)
	}

	// a type-only import binds no value: a value may take the name, an export may still name it
	/// How an import's local name binds: `None` for one that binds no value.
	pub(crate) fn import_binding(&self) -> crate::parser::scope::Binding {
		use crate::parser::scope::Binding;
		if self.ts.state.outer_kind == Some(Kind::Type) {
			Binding::None
		} else {
			Binding::Lexical
		}
	}

	/// Whether an `export` here belongs to TypeScript's own scoping, a namespace body say, so the
	/// script goal does not forbid it.
	pub(crate) fn exports_in_script(&self) -> bool {
		self.ts.state.module_blocks > 0
	}

	/// Whether the code describes rather than defines, an ambient declaration's body, where a
	/// definition's rules do not apply.
	pub(crate) fn in_ambient(&self) -> bool {
		self.ts.state.ambient
	}

	// Bindings

	/// After the id of a variable declarator; whether it is definite, `!`.
	pub(crate) fn var_id(&mut self, id: NodeId) -> Result<bool> {
		let mut definite = false;
		if matches!(self.kind(id), NodeKind::Identifier { .. }) && !self.tok.newline_before && self.is(TokenKind::Bang)
		{
			self.next()?;
			definite = true;
		}
		if self.is(TokenKind::Colon) {
			let annotation = self.parse_type_annotation(true, None)?;
			self.extras_mut(id).type_annotation = Some(annotation);
			self.ast.node_mut(id).end = self.prev_end;
		}
		Ok(definite)
	}

	pub(crate) fn var_declarator(&mut self, node: NodeId, kind: VariableKind, definite: bool) -> Result<()> {
		if definite {
			self.extras_mut(node).definite = true;
			if let NodeKind::VariableDeclarator { init: Some(_), .. } = self.kind(node) {
				return self.error(self.start_of(node), Code::DefiniteWithInitializer);
			}
		}
		if self.ts.state.ambient {
			self.check_ambient_initializer(node, kind)?;
		}
		Ok(())
	}

	/// Whether a declarator may go without an initializer where JavaScript requires one.
	pub(crate) fn allows_missing_initializer(&mut self) -> bool {
		self.ts.state.ambient
	}

	/// A whole parameter of a parameter list; `allow_modifiers` inside class methods.
	#[cold]
	pub(crate) fn parameter(&mut self, allow_modifiers: bool) -> Result<Option<NodeId>> {
		let mut decorators = Vec::new();
		while self.is(TokenKind::At) {
			decorators.push(self.parse_decorator()?);
		}
		let decorators = (!decorators.is_empty()).then(|| self.list_of(&decorators));
		let start = self.tok.start;
		let mut property = None;
		if allow_modifiers {
			let extras = self
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
		let this = self.is_keyword(Keyword::This);
		if !allow_modifiers
			&& !this && let Some(decorators) = decorators
		{
			return self.parameter_decorator_error(decorators);
		}
		let left = if this {
			let this = self.parse_ident(true)?;
			if decorators.is_some() || property.is_some() {
				return self.error(self.start_of(this), Code::ThisParameterModifiers);
			}
			this
		} else if self.is(TokenKind::Ellipsis) {
			self.parse_rest_binding()?
		} else {
			self.parse_binding_atom()?
		};
		let rest = matches!(self.kind(left), NodeKind::RestElement { .. });
		if !this && self.eat(TokenKind::Question)? {
			self.extras_mut(left).optional = true;
		}
		if self.is(TokenKind::Colon) {
			let annotation = self.parse_type_annotation(true, None)?;
			self.extras_mut(left).type_annotation = Some(annotation);
		}
		self.ast.node_mut(left).end = self.prev_end;
		if rest && self.ts.state.ambient && self.is(TokenKind::Comma) && self.peek_char().0 == Some(')') {
			self.next()?;
		}
		let item = if rest || this {
			left
		} else {
			self.parse_maybe_default(self.start_of(left), Some(left))?
		};
		if self.is(TokenKind::Colon) && item != left {
			return self.error(self.tok.start, Code::TypeAnnotationAfterDefault);
		}
		if decorators.is_some() {
			self.extras_mut(item).decorators = decorators;
		}
		let Some(extras) = property else {
			return Ok(Some(item));
		};
		if !matches!(self.kind(left), NodeKind::Identifier { .. }) {
			return self.error(start, Code::ParameterPropertyPattern);
		}
		let node = self.ts(TsKind::ParameterProperty { parameter: item }, start);
		*self.extras_mut(node) = extras;
		Ok(Some(node))
	}

	pub(crate) fn catch_param(&mut self, param: NodeId) -> Result<()> {
		if self.is(TokenKind::Colon) {
			let annotation = self.parse_type_annotation(true, None)?;
			let Some(TsKind::TypeAnnotation { type_annotation }) = self.ts_kind(annotation) else {
				unreachable!()
			};
			if !matches!(
				self.ts_kind(type_annotation),
				Some(TsKind::Keyword(ast::Keyword::Any | ast::Keyword::Unknown))
			) {
				return self.error(self.start_of(type_annotation), Code::CatchClauseType);
			}
			self.extras_mut(param).type_annotation = Some(annotation);
			self.ast.node_mut(param).end = self.prev_end;
		}
		Ok(())
	}

	/// After a pattern read on its own, the `Pattern` entry.
	pub(crate) fn pattern_annotation(&mut self, pattern: NodeId) -> Result<()> {
		if self.is(TokenKind::Colon) {
			let annotation = self.parse_type_annotation(true, None)?;
			self.extras_mut(pattern).type_annotation = Some(annotation);
			self.ast.node_mut(pattern).end = self.end_of(annotation);
		}
		Ok(())
	}

	// Functions and classes

	/// Before the parameters of a function, method or arrow.
	pub(crate) fn function_start(&mut self, kind: FunctionKind) -> Result<FunctionFrame> {
		let mut frame = FunctionFrame {
			in_class_method: matches!(kind, FunctionKind::Method { in_class: true, .. }),
			..FunctionFrame::default()
		};
		match kind {
			FunctionKind::Arrow => {
				frame.return_type = self.ts.state.arrow_return_type.take();
				if std::mem::take(&mut self.ts.state.body_needs_colon) && !self.is(TokenKind::BraceL) {
					self.ts.state.needs_colon_at = Some(self.tok.start);
				}
			}
			FunctionKind::Method { in_class: true, .. } => {}
			_ => frame.type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::Const)?,
		}
		if matches!(
			kind,
			FunctionKind::Method {
				kind: MethodKind::Get | MethodKind::Set,
				..
			}
		) && let Some(type_parameters) = frame.type_parameters
		{
			return self.error(self.start_of(type_parameters), Code::AccessorTypeParameters);
		}
		Ok(frame)
	}

	/// After the parameters, before the body; `Some` is a function without a body.
	#[expect(clippy::too_many_arguments)]
	pub(crate) fn function_body(
		&mut self,
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
			for param in self.ast.list(params).iter().flatten() {
				if self.ident_is(*param, "this") {
					continue;
				}
				if self.ast.typescript.extras(*param).is_some_and(|extras| extras.optional) {
					return self.error(self.start_of(*param), Code::SetterOptionalParameter);
				}
				if matches!(self.kind(*param), NodeKind::AssignmentPattern { .. }) {
					return self.error(self.start_of(*param), Code::SetterParameterInitializer);
				}
			}
		}
		if self.is(TokenKind::Colon) {
			let allow_predicate = !matches!(
				kind,
				FunctionKind::Method {
					kind: MethodKind::Get,
					..
				}
			);
			let return_type = self.parse_type_or_type_predicate_annotation(TokenKind::Colon, allow_predicate)?;
			match kind {
				FunctionKind::Method {
					kind: MethodKind::Set, ..
				} => {
					return self.error(self.start_of(return_type), Code::SetterReturnType);
				}
				FunctionKind::Method {
					kind: MethodKind::Constructor,
					..
				} => {
					return self.error(self.start_of(return_type), Code::ConstructorReturnType);
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
		if generator && self.ts.state.ambient && bodiless.is_some() {
			return self.error(start, Code::GeneratorInAmbient);
		}
		let signature = bodiless.is_some() && !self.is(TokenKind::BraceL) && self.is_line_terminator()?;
		self.check_parameter_list(params, signature)?;
		if signature && let Some(kind) = bodiless {
			if generator {
				return self.error(start, Code::GeneratorSignature);
			}
			return Ok(Some(self.ts(kind, start)));
		}
		if self.ts.state.ambient && bodiless.is_some() {
			return self.error(start, Code::ImplementationInAmbient);
		}
		Ok(None)
	}

	pub(crate) fn function_end(&mut self, node: NodeId, frame: FunctionFrame) -> Result<()> {
		if let NodeKind::ArrowFunctionExpression { params, .. } = self.kind(node) {
			self.check_parameter_list(params, false)?;
		}
		if let Some(TsKind::DeclareFunction { id: Some(id), .. }) = self.ts_kind(node) {
			self.declare_export_only(id);
		}
		if frame.type_parameters.is_some() || frame.return_type.is_some() {
			let extras = self.extras_mut(node);
			if frame.type_parameters.is_some() {
				extras.type_parameters = frame.type_parameters;
			}
			extras.return_type = frame.return_type;
		}
		if frame.in_class_method
			&& matches!(self.kind(node), NodeKind::FunctionExpression { .. })
			&& let Some(element) = self.ts.state.elements.last()
			&& element.extras.is_abstract
		{
			let (start, element) = (element.start, *element);
			let name = self.element_name(element);
			return self.error_name(start, Code::AbstractWithImplementation, name);
		}
		Ok(())
	}

	/// Whether an object literal accessor's first parameter is a `this` parameter, which does not
	/// count.
	pub(crate) fn accessor_this_param(&self, params: List) -> bool {
		self.ast
			.list(params)
			.first()
			.is_some_and(|param| param.is_some_and(|param| self.ident_is(param, "this")))
	}

	/// The parameters of a function-shaped TypeScript node.
	pub(crate) fn function_params(&self, node: NodeId) -> Option<List> {
		match self.ts_kind(node) {
			Some(TsKind::DeclareFunction { params, .. } | TsKind::DeclareMethod { params, .. }) => Some(params),
			_ => None,
		}
	}

	pub(crate) fn class_start(&mut self, kind: ClassKind) -> Result<()> {
		let mut frame = std::mem::take(&mut self.ts.state.next_class);
		frame.is_declaration = kind != ClassKind::Expression;
		if let Some(decorators) = self.take_decorators() {
			frame.decorators = Some(decorators);
			if !std::mem::take(&mut self.ts.state.decorators_before_export) {
				frame.start = Some(self.start_of(self.ast.list(decorators)[0].unwrap()));
			}
		}
		frame.outer_abstract = self.ts.state.in_abstract_class;
		self.ts.state.in_abstract_class = frame.is_abstract;
		self.ts.state.classes.push(frame);
		Ok(())
	}

	/// Whether the token after `class` opens a heritage clause rather than naming the class.
	pub(crate) fn starts_class_heritage(&mut self) -> bool {
		self.is_contextual("implements")
	}

	pub(crate) fn class_type_parameters(&mut self) -> Result<()> {
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::Class)?;
		self.ts.state.classes.last_mut().unwrap().type_parameters = type_parameters;
		Ok(())
	}

	pub(crate) fn class_heritage(&mut self, has_super: bool) -> Result<()> {
		let mut super_type_arguments = None;
		if has_super && (self.is(TokenKind::Lt) || self.is(TokenKind::LtLt)) {
			super_type_arguments = self.parse_type_arguments_in_expression()?;
		}
		let implements = if self.eat_contextual("implements")? {
			let clause = self.parse_heritage_clause("implements")?;
			Some(self.list_of(&clause))
		} else {
			None
		};
		let frame = self.ts.state.classes.last_mut().unwrap();
		frame.has_super = has_super;
		frame.super_type_arguments = super_type_arguments;
		frame.implements = implements;
		Ok(())
	}

	pub(crate) fn class_end(&mut self, node: NodeId) {
		let frame = self.ts.state.classes.pop().unwrap();
		self.ts.state.in_abstract_class = frame.outer_abstract;
		if let Some(start) = frame.start {
			self.ast.node_mut(node).start = start;
		}
		let extras = self.extras_mut(node);
		extras.type_parameters = frame.type_parameters;
		extras.super_type_arguments = frame.super_type_arguments;
		extras.implements = frame.implements;
		extras.decorators = frame.decorators;
		extras.is_abstract = frame.is_abstract;
		extras.declare = frame.declare;
	}

	/// Modifiers before a class element; returns whether `static` was among them.
	pub(crate) fn class_modifiers(&mut self) -> Result<bool> {
		let mut decorators = Vec::new();
		while self.is(TokenKind::At) {
			decorators.push(self.parse_decorator()?);
		}
		let start = self.tok.start;
		let mut modifiers = self.parse_modifiers(
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
			if self.is(TokenKind::BraceR) {
				return self.error(self.tok.end, Code::DecoratorPlacement);
			}
			modifiers.extras.decorators = Some(self.list_of(&decorators));
		}
		if modifiers != Modifiers::default() && self.is_contextual("static") && self.peek_char().0 == Some('{') {
			let snapshot = self.token_snapshot();
			self.next()?;
			self.next()?;
			let inside = self.tok.start;
			self.restore_tokens(snapshot);
			return self.error(inside, Code::StaticBlockModifier);
		}
		let outer_ambient = modifiers
			.extras
			.declare
			.then(|| std::mem::replace(&mut self.ts.state.ambient, true));
		self.ts.state.elements.push(ElementFrame {
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
	pub(crate) fn type_parameters(&mut self) -> Result<NodeId> {
		self.parse_type_parameters(TypeParameterModifiers::Const)
	}

	pub(crate) fn class_index_signature(&mut self, start: u32) -> Result<Option<NodeId>> {
		let Some(signature) = self.try_parse_index_signature(start)? else {
			let has_super = self.ts.state.classes.last().is_some_and(|c| c.has_super);
			let extras = self.ts.state.elements.last().unwrap().extras;
			if !self.ts.state.in_abstract_class && extras.is_abstract {
				return self.error(start, Code::AbstractOutsideAbstractClass);
			}
			if extras.is_override && !has_super {
				return self.error(start, Code::OverrideWithoutExtends);
			}
			return Ok(None);
		};
		let ElementFrame { extras, accessor, .. } = *self.ts.state.elements.last().unwrap();
		if extras.is_abstract {
			return self.error_arg(start, Code::IndexSignatureModifier, "abstract");
		}
		if let Some(accessibility) = extras.accessibility {
			let accessibility = self.intern(accessibility.as_str());
			return self.raise(SyntaxError::new(start, Code::IndexSignatureModifier).arg(accessibility));
		}
		if extras.declare {
			return self.error_arg(start, Code::IndexSignatureModifier, "declare");
		}
		if extras.is_override {
			return self.error_arg(start, Code::IndexSignatureModifier, "override");
		}
		if accessor {
			return self.error_arg(start, Code::IndexSignatureModifier, "accessor");
		}
		Ok(Some(signature))
	}

	/// After the key of a class element.
	pub(crate) fn class_key_end(&mut self, key: NodeId, computed: bool) -> Result<()> {
		let element = self.ts.state.elements.last_mut().unwrap();
		element.key = Some(key);
		element.computed = computed;
		let start = element.start;
		let extras = element.extras;
		let accessor = element.accessor;
		if self.eat(TokenKind::Question)? {
			self.ts.state.elements.last_mut().unwrap().extras.optional = true;
		}
		let method = self.is(TokenKind::ParenL) || self.is(TokenKind::Lt);
		if extras.readonly && method {
			return self.error(start, Code::ReadonlyOnMethod);
		}
		if extras.declare && method {
			return self.error(start, Code::DeclareOnMethod);
		}
		if accessor && method {
			return self.error(start, Code::AccessorPlacement);
		}
		Ok(())
	}

	pub(crate) fn starts_class_method(&mut self) -> bool {
		self.is(TokenKind::Lt)
	}

	pub(crate) fn class_method_start(&mut self, kind: MethodKind) -> Result<()> {
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::Const)?;
		if matches!(kind, MethodKind::Get | MethodKind::Set)
			&& let Some(type_parameters) = type_parameters
		{
			return self.error(self.start_of(type_parameters), Code::AccessorTypeParameters);
		}
		let element = *self.ts.state.elements.last().unwrap();
		let key = element.key.unwrap();
		if matches!(self.kind(key), NodeKind::PrivateIdentifier { .. }) {
			if let Some(accessibility) = element.extras.accessibility {
				let accessibility = self.intern(accessibility.as_str());
				return self.raise(SyntaxError::new(element.start, Code::PrivateModifier).arg(accessibility));
			}
		} else if let Some(type_parameters) = type_parameters
			&& !element.extras.is_static
			&& !element.computed
			&& matches!(self.kind(key), NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } if self.str(name) == "constructor")
		{
			return self.error(self.start_of(type_parameters), Code::ConstructorTypeParameters);
		}
		self.ts.state.elements.last_mut().unwrap().extras.type_parameters = type_parameters;
		Ok(())
	}

	/// After the key of a class field, before its initializer.
	pub(crate) fn class_field_annotation(&mut self) -> Result<()> {
		let element = *self.ts.state.elements.last().unwrap();
		let mut definite = false;
		let mut optional = element.extras.optional;
		if !optional {
			if self.is(TokenKind::Bang) && !self.tok.newline_before {
				self.next()?;
				definite = true;
			} else {
				optional = self.eat(TokenKind::Question)?;
			}
		}
		let type_annotation = self.try_parse_type_annotation()?;
		let extras = &mut self.ts.state.elements.last_mut().unwrap().extras;
		extras.definite = definite;
		extras.optional = optional;
		extras.type_annotation = type_annotation;
		let private = element
			.key
			.is_some_and(|key| matches!(self.kind(key), NodeKind::PrivateIdentifier { .. }));
		if private {
			if element.extras.is_abstract {
				return self.error_arg(element.start, Code::PrivateModifier, "abstract");
			}
			if let Some(accessibility) = element.extras.accessibility {
				let accessibility = self.intern(accessibility.as_str());
				return self.raise(SyntaxError::new(element.start, Code::PrivateModifier).arg(accessibility));
			}
		} else if self.is(TokenKind::Eq) {
			if self.ts.state.ambient && !(element.extras.readonly && type_annotation.is_none()) {
				return self.error(self.tok.start, Code::InitializerInAmbient);
			}
			if element.extras.is_abstract {
				let name = self.element_name(element);
				return self.error_name(self.tok.start, Code::AbstractWithInitializer, name);
			}
		}
		Ok(())
	}

	pub(crate) fn class_element_end(&mut self, node: NodeId) -> Result<()> {
		let frame = self.ts.state.elements.pop().unwrap();
		if let Some(outer) = frame.outer_ambient {
			self.ts.state.ambient = outer;
		}
		if frame.accessor
			&& let NodeKind::PropertyDefinition { kind, .. } = &mut self.ast.node_mut(node).kind
		{
			*kind = FieldKind::Accessor;
		}
		if let NodeKind::MethodDefinition {
			key,
			kind,
			computed: false,
			..
		} = self.kind(node)
			&& matches!(kind, MethodKind::Constructor | MethodKind::Method)
			&& matches!(self.kind(key), NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } if self.str(name) == "constructor")
		{
			if frame.extras.is_abstract {
				return self.error(frame.start, Code::AbstractPlacement);
			}
			let modifier = if frame.extras.is_static {
				Some("static")
			} else if frame.extras.is_override {
				Some("override")
			} else {
				None
			};
			if let Some(modifier) = modifier {
				return self.error_arg(frame.start, Code::ConstructorModifier, modifier);
			}
		}
		if let Some(decorators) = frame.extras.decorators {
			let start = self.start_of(self.ast.list(decorators)[0].unwrap());
			self.ast.node_mut(node).start = start;
			if let NodeKind::MethodDefinition { kind, value, .. } = self.kind(node) {
				if kind == MethodKind::Constructor {
					return self.error(start, Code::DecoratorOnConstructor);
				}
				if matches!(self.ts_kind(value), Some(TsKind::DeclareMethod { .. })) {
					return self.error(start, Code::DecoratorWithoutBody);
				}
			}
			if matches!(self.ts_kind(node), Some(TsKind::IndexSignature { .. })) {
				return self.error(start, Code::DecoratorPlacement);
			}
		}
		if let NodeKind::MethodDefinition { kind, value, .. } = self.kind(node) {
			let (params, has_body) = match self.kind(value) {
				NodeKind::FunctionExpression { function } => (function.params, true),
				_ => (self.function_params(value).unwrap(), false),
			};
			if !has_body || kind == MethodKind::Get || !self.ts.state.classes.last().unwrap().is_declaration {
				for &param in self.ast.list(params).iter().flatten() {
					let param = self.unwrap(param, Unwrap::InnerPattern).unwrap_or(param);
					if let Some(decorators) = self.ast.typescript.extras(param).and_then(|extras| extras.decorators) {
						return self.parameter_decorator_error(decorators);
					}
				}
			}
		}
		let mut extras = frame.extras;
		extras.is_static &= matches!(self.kind(node), NodeKind::Extension(_));
		if extras != Extras::default() {
			*self.extras_mut(node) = extras;
		}
		Ok(())
	}

	// Expressions

	#[cold]
	pub(crate) fn maybe_assign(&mut self, for_init: ForInit, _: &mut Errors) -> Result<Option<NodeId>> {
		if !self.starts_with_lt() {
			return Ok(None);
		}
		self.try_arrow_head(self.tok.start, self.tok.start, None, false, for_init, true)
	}

	/// After an arrow's parameters, before `=>`.
	pub(crate) fn arrow_return_type(&mut self) -> Result<()> {
		self.ts.state.arrow_return_type = if self.is(TokenKind::Colon) {
			Some(self.parse_type_or_type_predicate_annotation(TokenKind::Colon, true)?)
		} else {
			None
		};
		Ok(())
	}

	pub(crate) fn arrow_start(&mut self, start: u32) {
		self.ts.state.body_needs_colon = self.ts.state.needs_colon_at == Some(start);
	}

	pub(crate) fn conditional_branch(&mut self, start: u32, alternate: bool, for_init: ForInit) -> Result<NodeId> {
		let old = self.ts.state.needs_colon_at;
		if !alternate || old == Some(start) {
			self.ts.state.needs_colon_at = Some(self.tok.start);
		}
		let branch = self.parse_maybe_assign(if alternate { for_init } else { ForInit::No }, &mut None);
		self.ts.state.needs_colon_at = old;
		branch
	}

	/// Whether the current token, one of the host's stop tokens, is one TypeScript reads after an
	/// expression; it then decides where the host's syntax starts.
	pub(crate) fn reads_stop(&self) -> bool {
		(self.is_contextual("as") || self.is_contextual("satisfies")) && self.forced_stop != Some(self.tok.start)
	}

	/// At an operator position; `Some` is the new left operand.
	#[cold]
	pub(crate) fn expr_op(&mut self, left: NodeId, left_start: u32) -> Result<Option<NodeId>> {
		let is_as = self.is_contextual("as");
		if !is_as && !self.is_contextual("satisfies") {
			return Ok(None);
		}
		if self.tok.stop {
			if self.forced_stop == Some(self.tok.start) {
				self.stop_here();
				return Ok(None);
			}
			// read as the assertion for now; the entry decides afterwards which one was the host's
			self.stop_word_at = Some(self.tok.start);
		}
		assertion(self, left, left_start, is_as).map(Some)
	}

	#[cold]
	pub(crate) fn subscript(
		&mut self,
		base: NodeId,
		start: u32,
		no_calls: bool,
		_maybe_async_arrow: bool,
		optional_chained: bool,
		for_init: ForInit,
	) -> Result<Option<(NodeId, bool)>> {
		if self.is(TokenKind::Bang) {
			if self.tok.newline_before {
				return Ok(None);
			}
			self.next()?;
			self.stop_after_operand();
			let node = self.ts(TsKind::NonNullExpression { expression: base }, start);
			return Ok(Some((node, false)));
		}
		let optional_call = self.is(TokenKind::QuestionDot);
		if optional_call && self.peek_char().0 != Some('<') {
			return Ok(None);
		}
		if optional_call && no_calls {
			return Ok(Some((base, false)));
		}
		let subscript = self.attempt(|p| {
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
		if optional_chained && let NodeKind::TaggedTemplateExpression { quasi, .. } = self.kind(node) {
			let at = self.start_of(quasi);
			return Err(Box::new(
				SyntaxError::new(at, Code::OptionalChainInTaggedTemplate).to(at + 1),
			));
		}
		if matches!(self.ts_kind(node), Some(TsKind::InstantiationExpression { .. }))
			&& (self.is(TokenKind::Dot) || (self.is(TokenKind::QuestionDot) && self.peek_char().0 != Some('(')))
		{
			return self.error(self.tok.start, Code::PropertyAfterInstantiation);
		}
		Ok(Some((node, is_optional_call)))
	}

	pub(crate) fn new_expression(&mut self, node: NodeId) {
		let NodeKind::NewExpression { callee, arguments } = self.kind(node) else {
			unreachable!()
		};
		if let Some(TsKind::InstantiationExpression {
			expression,
			type_arguments,
		}) = self.ts_kind(callee)
		{
			self.ast.node_mut(node).kind = NodeKind::NewExpression {
				callee: expression,
				arguments,
			};
			self.extras_mut(node).type_arguments = Some(type_arguments);
		}
	}

	/// An object property whose value starts unexpectedly for JavaScript.
	#[cold]
	pub(crate) fn property_value(
		&mut self,
		start: u32,
		key: NodeId,
		computed: bool,
		is_pattern: bool,
		generator: bool,
		is_async: bool,
	) -> Result<NodeId> {
		if is_pattern {
			return self.unexpected();
		}
		let value = self.parse_method(generator, is_async, false, false, MethodKind::Method)?;
		Ok(self.add(
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

	/// Reinterprets a TypeScript node as a pattern; the result replaces it.
	pub(crate) fn ts_make_pattern(&mut self, id: NodeId, errors: &mut Errors) -> Result<Option<NodeId>> {
		match self.ts_kind(id) {
			Some(
				TsKind::AsExpression { expression, .. }
				| TsKind::SatisfiesExpression { expression, .. }
				| TsKind::NonNullExpression { expression }
				| TsKind::TypeAssertion { expression, .. },
			) => self.make_pattern(expression, errors).map(Some),
			_ => Ok(None),
		}
	}

	/// The plain node a TypeScript wrapper stands for in a check, if any.
	pub(crate) fn unwrap(&self, id: NodeId, context: Unwrap) -> Option<NodeId> {
		match (context, self.ts_kind(id)?) {
			(Unwrap::Simple, TsKind::NonNullExpression { expression } | TsKind::AsExpression { expression, .. }) => {
				Some(expression)
			}
			(Unwrap::InnerPattern, TsKind::ParameterProperty { parameter: inner }) => Some(inner),
			_ => None,
		}
	}
}

/// `left as T` or `left satisfies T`, the word being the current token.
fn assertion<const TS: bool>(p: &mut Parser<'_, TS>, left: NodeId, left_start: u32, is_as: bool) -> Result<NodeId> {
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
