// The rest of `ts.rs`: what TypeScript reads around JavaScript's own grammar, and its declarations.

impl<'a, const COUNT: bool, const TS: bool> Parser<'a, COUNT, TS> {
	fn check_parameter_list(&mut self, params: List, signature: bool) {
		let mut seen_optional = false;
		for i in 0..params.len {
			let Some(item) = self.ast.nth(params, i) else { continue };
			let param = self.unwrap_inner_pattern(item).unwrap_or(item);
			let binding = match self.kind_of(param) {
				NodeKind::AssignmentPattern { left, .. } => left,
				_ => param,
			};
			if self.ident_is(binding, "this") {
				continue;
			}
			let optional = self.ast.extras.get(binding).is_some_and(|extras| extras.optional);
			let rest = matches!(self.kind_of(param), NodeKind::RestElement { .. });
			let at = self.start_of(param);
			if rest && optional {
				self.raise(at, Code::OptionalRest);
				return;
			}
			let initialized = matches!(self.kind_of(param), NodeKind::AssignmentPattern { .. });
			if optional && initialized {
				self.raise(at, Code::OptionalWithInitializer);
				return;
			}
			if optional
				&& !signature
				&& !self.scratch.ts.state.ambient
				&& matches!(
					self.kind_of(binding),
					NodeKind::ObjectPattern { .. } | NodeKind::ArrayPattern { .. }
				) {
				self.raise(at, Code::OptionalPatternParameter);
				return;
			}
			if signature && let Some(default) = self.first_default(param) {
				let at = if default == param { item } else { default };
				let at = self.start_of(at);
				self.raise(at, Code::SignatureParameterDefault);
				return;
			}
			if seen_optional && !optional && !initialized && !rest {
				self.raise(at, Code::RequiredAfterOptional);
				return;
			}
			seen_optional |= optional;
		}
	}

	fn first_default(&self, pattern: NodeId) -> Option<NodeId> {
		match self.kind_of(pattern) {
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

	pub(super) fn unwrap_simple(&self, id: NodeId) -> Option<NodeId> {
		match self.kind_of(id) {
			NodeKind::NonNullExpression { expression } | NodeKind::AsExpression { expression, .. } => Some(expression),
			_ => None,
		}
	}

	pub(super) fn unwrap_inner_pattern(&self, id: NodeId) -> Option<NodeId> {
		match self.kind_of(id) {
			NodeKind::ParameterProperty { parameter } => Some(parameter),
			_ => None,
		}
	}

	/// The expression a TypeScript wrapper holds, where a pattern is made of it.
	pub(super) fn ts_wrapped(&self, id: NodeId) -> Option<NodeId> {
		match self.kind_of(id) {
			NodeKind::AsExpression { expression, .. }
			| NodeKind::SatisfiesExpression { expression, .. }
			| NodeKind::NonNullExpression { expression }
			| NodeKind::TypeAssertion { expression, .. } => Some(expression),
			_ => None,
		}
	}

	// ── decorators

	fn parse_decorator(&mut self) -> NodeId {
		let start = self.tok.start;
		self.bump(true);
		self.scratch.ts.state.decorators.push(Vec::new());
		let expression_start = self.tok.start;
		let mut expression = if self.tok.kind == Kind::ParenL {
			self.bump(true);
			let no_in = std::mem::replace(&mut self.ctx.no_in, false);
			let inner = self.sequence();
			self.ctx.no_in = no_in;
			self.expect(Kind::ParenR, false);
			inner
		} else {
			let mut expression = self.ident();
			while !self.error && self.eat(Kind::Dot) {
				let property = self.name();
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
		if self.error {
			return expression;
		}
		if self.tok.kind == Kind::ParenL {
			self.bump(true);
			let items = self.arguments();
			let arguments = self.list(items);
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
		self.scratch.ts.state.decorators.pop();
		self.add(NodeKind::Decorator { expression }, start)
	}

	#[cold]
	pub(super) fn parse_decorators(&mut self, allow_export: bool) {
		if self
			.scratch
			.ts
			.state
			.decorators
			.last()
			.is_some_and(|decorators| !decorators.is_empty())
		{
			self.raise(self.tok.start, Code::DecoratorPlacement);
			return;
		}
		let mut decorators = Vec::new();
		while self.tok.kind == Kind::At {
			let decorator = self.parse_decorator();
			if self.error {
				return;
			}
			decorators.push(decorator);
		}
		if self.tok.kind == Kind::Export && !allow_export {
			self.fail();
			return;
		}
		if !self.peek_tokens(|p| p.decorated_class_follows()) {
			if !self.failed() {
				self.raise(self.tok.start, Code::DecoratorPlacement);
			}
			return;
		}
		self.scratch.ts.state.decorators_before_export = self.tok.kind == Kind::Export;
		if self.scratch.ts.state.decorators.is_empty() {
			self.scratch.ts.state.decorators.push(Vec::new());
		}
		self.scratch.ts.state.decorators.last_mut().unwrap().extend(decorators);
	}

	fn decorated_class_follows(&mut self) -> bool {
		if self.tok.kind == Kind::Export {
			self.tnext();
			if self.tok.kind == Kind::Default {
				self.tnext();
			}
			if self.tok.kind == Kind::At {
				return true;
			}
		}
		while matches!(self.head(), Some(Head::Abstract | Head::Declare)) {
			self.tnext();
			if self.newline() {
				return false;
			}
		}
		self.tok.kind == Kind::Class
	}

	fn take_decorators(&mut self) -> Option<List> {
		let pending = self.scratch.ts.state.decorators.last_mut()?;
		if pending.is_empty() {
			return None;
		}
		let decorators: Vec<Option<NodeId>> = std::mem::take(pending).into_iter().map(Some).collect();
		Some(self.ast.add_list(&decorators))
	}

	fn parameter_decorator_error(&mut self, decorators: List) {
		let first = self.ast.nth(decorators, 0).unwrap();
		let at = self.start_of(first);
		self.raise(at, Code::DecoratorPlacement);
	}

	fn declare_type(&mut self, id: NodeId, unique: bool) {
		let NodeKind::Identifier { name } = self.kind_of(id) else {
			return;
		};
		if unique && self.scratch.checks.current_has_type(name) {
			let at = self.start_of(id);
			self.raise_name(at, Code::TypeRedeclaration, name);
			return;
		}
		if self.scratch.ts.state.global_depth > 0 {
			self.scratch.checks.declare_global(name);
		}
		self.scratch.checks.declare_here_type(name);
	}

	fn declare_export_only(&mut self, id: NodeId) {
		let NodeKind::Identifier { name } = self.kind_of(id) else {
			return;
		};
		self.scratch.checks.declare_here_export_only(name);
	}

	fn element_name(&mut self, element: ElementFrame) -> StrId {
		match element.key {
			Some(key) if !element.computed && matches!(self.kind_of(key), NodeKind::Identifier { .. }) => {
				let NodeKind::Identifier { name } = self.kind_of(key) else {
					unreachable!()
				};
				name
			}
			Some(key) => {
				let source = &self.src[self.start_of(key) as usize..self.end_of(key) as usize];
				self.ast.strings.intern(&format!("[{source}]"))
			}
			None => self.ast.strings.intern(""),
		}
	}

	// ── expressions

	fn try_next_parse_constant_context(&mut self) -> Option<NodeId> {
		if self.peek_tok().kind != Kind::Const {
			return None;
		}
		self.tnext();
		let reference = self.parse_type_reference(true);
		if self.error {
			return Some(reference);
		}
		if let NodeKind::TypeReference {
			type_arguments: Some(_),
			type_name,
		} = self.kind_of(reference)
		{
			let at = self.start_of(type_name);
			return Some(self.raise(at, Code::InvalidConst));
		}
		Some(reference)
	}

	fn check_const_assertion(&mut self, expression: NodeId, type_annotation: NodeId) {
		if !matches!(self.kind_of(type_annotation), NodeKind::TypeReference { type_name, .. } if self.ident_is(type_name, "const"))
		{
			return;
		}
		let valid = match self.kind_of(expression) {
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
				self.kind_of(argument),
				NodeKind::NumberLiteral { .. } | NodeKind::BigIntLiteral
			),
			_ => false,
		};
		if !valid {
			let at = self.start_of(expression);
			self.raise(at, Code::ConstAssertionTarget);
		}
	}

	/// `<T>expression`, or the type parameters of a generic arrow function, its `<` at the cursor.
	#[cold]
	pub(super) fn parse_type_assertion(&mut self) -> NodeId {
		let start = self.tok.start;
		let assertion = self.attempt(|p| {
			p.nest = p.nest.wrapping_add(1);
			let type_annotation = match p.try_next_parse_constant_context() {
				Some(constant) => constant,
				None => p.next_then_parse_type(),
			};
			p.nest = p.nest.wrapping_sub(1);
			p.expect(Kind::Gt, true);
			let expression = p.unary_for_ts();
			p.add(
				NodeKind::TypeAssertion {
					type_annotation,
					expression,
				},
				start,
			)
		});
		if self.error {
			return NodeId::at(0);
		}
		match assertion {
			Some(node) => {
				let NodeKind::TypeAssertion {
					expression,
					type_annotation,
				} = self.kind_of(node)
				else {
					unreachable!()
				};
				self.check_const_assertion(expression, type_annotation);
				node
			}
			None => {
				let type_parameters = self.parse_type_parameters(TypeParameterModifiers::Const);
				if !self.error && self.tok.kind != Kind::ParenL {
					return self.fail();
				}
				type_parameters
			}
		}
	}

	fn at_possible_async_arrow(&self, base: NodeId) -> bool {
		self.ident_is(base, "async")
			&& self.prev_end == self.end_of(base)
			&& !self.can_insert_semi()
			&& self.end_of(base) - self.start_of(base) == 5
			&& self.start_of(base) == self.arrow_at
	}

	/// An arrow's head tried at `<` (`generic`) or after a list's `(` at `at`.
	pub(super) fn try_arrow_head(
		&mut self,
		start: u32,
		at: u32,
		colon: Option<u32>,
		is_async: bool,
		generic: bool,
	) -> Option<NodeId> {
		let spine = self.scratch.ts.state.needs_colon_at == Some(start);
		if self.scratch.ts.not_arrow.contains(&(at, spine)) {
			return None;
		}
		if let Some(colon) = colon
			&& !self.arrow_after_type(colon, is_async, spine)
		{
			if self.error {
				return None;
			}
			self.scratch.ts.not_arrow.insert((at, spine));
			return None;
		}
		let before = spine.then(|| self.snapshot());
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		let mut head = self.attempt(|p| {
			if !generic {
				return (None, p.read_arrow_params(is_async));
			}
			let type_parameters = p.parse_type_parameters(TypeParameterModifiers::Const);
			p.expect(Kind::ParenL, true);
			(Some(type_parameters), p.read_arrow_params(is_async))
		});
		if self.error {
			return None;
		}
		if head.is_some()
			&& self.scratch.ts.state.arrow_return_type.is_some()
			&& let Some(before) = before
		{
			if !self.colon_after_body(is_async) {
				if self.error {
					return None;
				}
				if let Some((_, base)) = head {
					self.scratch.items.truncate(base);
				}
				self.restore(before);
				head = None;
			}
		}
		let Some((type_parameters, base)) = head else {
			self.restore_yield_await(outer);
			self.scratch.ts.not_arrow.insert((at, spine));
			return None;
		};
		self.ts_check_arrow_head(base, is_async, outer);
		if self.error {
			return None;
		}
		let arrow = self.ts_arrow(start, base, is_async);
		if self.error {
			return Some(arrow);
		}
		if type_parameters.is_some() {
			self.extras_mut(arrow).type_parameters = type_parameters;
		}
		Some(arrow)
	}

	fn arrow_after_type(&mut self, colon: u32, is_async: bool, spine: bool) -> bool {
		self.lookahead_soft(|p| {
			p.lexer.rewind(colon as usize);
			p.tok = p.lexer.next(false);
			if p.in_type {
				p.tok = p.split_angle(p.tok);
			}
			p.parse_type_or_type_predicate_annotation(Kind::Colon, true);
			if p.failed() {
				return false;
			}
			if p.can_insert_semi() || !p.eat(Kind::Arrow) {
				return false;
			}
			!spine || p.colon_after_body(is_async)
		})
	}

	fn colon_after_body(&mut self, is_async: bool) -> bool {
		let at = self.tok.start;
		if let Some(&colon) = self.scratch.ts.colon_after_body.get(&at) {
			return colon;
		}
		let colon = self.lookahead_soft(|p| {
			let old = p.take_yield_await();
			p.enter_scope(function_flags(is_async, false) | SCOPE_ARROW);
			p.scratch.ts.state.body_needs_colon = true;
			p.function_start(FunctionKind::Arrow);
			let outer = p.ctx;
			let expression = p.tok.kind != Kind::BraceL;
			p.ctx = Ctx {
				function: true,
				is_async,
				generator: false,
				can_await: is_async,
				no_in: false,
				for_await: false,
			};
			let _ = expression;
			p.function_body(at, None, List::EMPTY, true, false);
			p.ctx = outer;
			p.restore_yield_await(old);
			p.tok.kind == Kind::Colon
		});
		if self.error {
			return false;
		}
		self.scratch.ts.colon_after_body.insert(at, colon);
		colon
	}

	/// An arrow's parameters after their `(`, through `=>`, read as bindings: where they start
	/// among the items.
	fn read_arrow_params(&mut self, is_async: bool) -> usize {
		(self.yield_pos, self.await_pos) = (0, 0);
		if is_async {
			self.await_ident_pos = 0;
		}
		let held = !matches!(self.tok.kind, Kind::ParenR | Kind::Ellipsis);
		if held && !self.enter() {
			return self.scratch.items.len();
		}
		let base = self.scratch.items.len();
		self.ts_binding_items(Kind::ParenR, false, false);
		if held {
			self.depth -= 1;
		}
		if self.error {
			return base;
		}
		self.close_arrow_head();
		base
	}

	fn close_arrow_head(&mut self) {
		self.arrow_return_type();
		if self.error {
			return;
		}
		if self.can_insert_semi() {
			self.fail();
			return;
		}
		self.expect(Kind::Arrow, true);
	}

	fn ts_check_arrow_head(&mut self, base: usize, is_async: bool, (yield_pos, await_pos, await_ident_pos): (u32, u32, u32)) {
		self.check_yield_await_in_default_params();
		if self.error {
			return;
		}
		if is_async && self.await_ident_pos > 0 {
			self.raise(self.await_ident_pos, Code::AwaitAsIdentifier);
			return;
		}
		(self.yield_pos, self.await_pos) = (yield_pos, await_pos);
		if is_async {
			self.await_ident_pos = await_ident_pos;
		}
		for i in base..self.scratch.items.len() {
			if let Some(param) = self.scratch.items[i]
				&& self.ident_is(param, "this")
			{
				let at = self.start_of(param);
				self.raise(at, Code::InvalidAssignmentTarget);
				return;
			}
		}
	}

	/// A list that may be an arrow's parameters, its `(` at `at` read: the arrow when it is one.
	pub(super) fn ts_decided_arrow(&mut self, start: u32, at: u32, is_async: bool) -> Option<NodeId> {
		let first = self.tok;
		let outer = (self.yield_pos, self.await_pos, self.await_ident_pos);
		let base = match self.list_role(at, first, is_async) {
			Role::Parameters => {
				let base = self.read_arrow_params(is_async);
				if self.error {
					return Some(NodeId::at(0));
				}
				base
			}
			Role::Expression => return None,
			Role::Typed(colon) => {
				let arrow = self.try_arrow_head(start, at, Some(colon), is_async, false);
				if self.error {
					return Some(NodeId::at(0));
				}
				return arrow;
			}
			Role::Unsure => {
				let arrow = self.try_arrow_head(start, at, None, is_async, false);
				if self.error {
					return Some(NodeId::at(0));
				}
				return arrow;
			}
		};
		self.ts_check_arrow_head(base, is_async, outer);
		if self.error {
			return Some(NodeId::at(0));
		}
		Some(self.ts_arrow(start, base, is_async))
	}

	/// A list in parentheses where an arrow function may start, its `(` at the cursor.
	pub(super) fn ts_paren(&mut self, start: u32) -> NodeId {
		let back = self.tok_snap();
		self.bump(true);
		// a list of names holds a level while it is read, before its role is asked
		if self.tok.kind == Kind::Ident && self.depth + 1 > MAX_DEPTH {
			return self.raise(self.tok.start, Code::NestingDepth);
		}
		let empty = self.tok.kind == Kind::ParenR;
		if let Some(arrow) = self.ts_decided_arrow(start, start, false) {
			return arrow;
		}
		if self.error {
			return NodeId::at(0);
		}
		self.restore_tokens(back);
		// `()` and a token that does not lex: the parser in use reads that token for the arrow
		if empty && !self.lexer.recover && !self.lexer.lexes_ahead(2) {
			self.bump(true);
			self.bump(false);
			return self.fail();
		}
		self.arrow_at = u32::MAX;
		self.paren(start)
	}

	fn ts_arrow(&mut self, start: u32, base: usize, is_async: bool) -> NodeId {
		self.arrow(start, base, is_async)
	}

	// ── the role of a list, told by its tokens

	fn look(&mut self, lexer: &mut Lexer<'a>) -> Option<Token> {
		let t = lexer.next(false);
		if lexer.error.is_some() {
			return None;
		}
		Some(t)
	}

	fn list_role(&mut self, open: u32, first: Token, call: bool) -> Role {
		let mut lexer = Lexer::new(self.src, self.module, false);
		lexer.strict = self.lexer.strict;
		lexer.at_sign = self.lexer.at_sign;
		lexer.set_pos(self.lexer.pos());
		match self.first_tokens(&mut lexer, first, call) {
			Some(Some(role)) => role,
			Some(None) => match self.scratch.ts.roles.get(&open) {
				Some(&role) => role,
				None => {
					lexer.set_pos(self.lexer.pos());
					self.scan(&mut lexer, open, first).unwrap_or(Role::Unsure)
				}
			},
			None => Role::Unsure,
		}
	}

	fn is_modifier_word(&self, t: Token) -> bool {
		t.kind == Kind::Ident
			&& matches!(
				&self.src[t.start as usize..t.end as usize],
				"public" | "private" | "protected" | "readonly" | "static" | "abstract" | "declare" | "override" | "accessor"
			)
	}

	/// What the first tokens tell; the outer none when the text does not lex.
	fn first_tokens(&mut self, lexer: &mut Lexer<'a>, first: Token, call: bool) -> Option<Option<Role>> {
		match first.kind {
			Kind::ParenR => {
				let token = self.look(lexer)?;
				return Some(Some(match token.kind {
					Kind::Arrow => Role::Parameters,
					Kind::Colon if call => Role::Typed(token.start),
					Kind::Colon | Kind::BraceL if !call => Role::Parameters,
					_ => Role::Expression,
				}));
			}
			Kind::Ellipsis if !call => return Some(Some(Role::Parameters)),
			Kind::Ellipsis | Kind::BracketL | Kind::BraceL => return Some(None),
			Kind::Ident | Kind::This => {}
			_ => return Some(Some(Role::Expression)),
		}
		let token = self.look(lexer)?;
		Some(match token.kind {
			Kind::ParenR => {
				let token = self.look(lexer)?;
				Some(Self::after_closer(token))
			}
			Kind::Comma | Kind::Eq => None,
			Kind::Colon => Some(Role::Parameters),
			Kind::Question => {
				let token = self.look(lexer)?;
				Some(match token.kind {
					Kind::Colon | Kind::Comma | Kind::Eq | Kind::ParenR => Role::Parameters,
					_ => Role::Expression,
				})
			}
			Kind::Ident if self.is_modifier_word(first) => {
				Some(match &self.src[token.start as usize..token.end as usize] {
					"as" | "satisfies" if token.flags & ESCAPED == 0 => Role::Expression,
					_ => Role::Parameters,
				})
			}
			_ => Some(Role::Expression),
		})
	}

	fn after_closer(follower: Token) -> Role {
		match follower.kind {
			Kind::Arrow if follower.flags & NEWLINE_BEFORE == 0 => Role::Parameters,
			Kind::Colon => Role::Typed(follower.start),
			_ => Role::Expression,
		}
	}

	fn scan(&mut self, lexer: &mut Lexer<'a>, open: u32, first: Token) -> Option<Role> {
		fn postfix(slash: Slash, token: Token) -> Slash {
			match slash {
				Slash::Divide if token.flags & NEWLINE_BEFORE == 0 => Slash::Divide,
				Slash::Unsure => Slash::Unsure,
				_ => Slash::Regex,
			}
		}
		// closer, opener, what a `/` is after the closer
		let mut frames: Vec<(Kind, u32, Slash)> = vec![(Kind::ParenR, open, Slash::Divide)];
		let mut slash = Slash::Regex;
		let mut closed: Option<u32> = None;
		let (mut prev, mut prev2) = (Kind::ParenL, Kind::Eof);
		let mut prev_word_await = false;
		let mut unread = Some(first);
		let role = loop {
			let mut token = match unread.take() {
				Some(first) => first,
				None => {
					let t = lexer.next(false);
					if lexer.error.is_some() {
						break None;
					}
					t
				}
			};
			if let Some(at) = closed.take() {
				let role = Self::after_closer(token);
				self.scratch.ts.roles.insert(at, role);
				if frames.is_empty() {
					break Some(role);
				}
			}
			let mut word_await = false;
			match token.kind {
				Kind::Slash | Kind::SlashEq => match slash {
					Slash::Regex => {
						token = lexer.regex(token);
						if lexer.error.is_some() {
							break None;
						}
						slash = Slash::Divide;
					}
					Slash::Divide => slash = Slash::Regex,
					Slash::Unsure => break None,
				},
				Kind::Template => {
					if token.flags & BROKEN != 0 {
						break None;
					}
					slash = Slash::Divide;
				}
				Kind::TemplateHead => {
					frames.push((Kind::Template, token.start, Slash::Divide));
					slash = Slash::Regex;
				}
				Kind::ParenL => {
					let statement = matches!(
						prev,
						Kind::If | Kind::While | Kind::For | Kind::With | Kind::Switch | Kind::Catch
					) && !matches!(prev2, Kind::Dot | Kind::QuestionDot)
						|| (prev2 == Kind::For && prev_word_await);
					frames.push((
						Kind::ParenR,
						token.start,
						if statement { Slash::Regex } else { Slash::Divide },
					));
					slash = Slash::Regex;
				}
				Kind::BracketL => {
					frames.push((Kind::BracketR, token.start, Slash::Divide));
					slash = Slash::Regex;
				}
				Kind::BraceL => {
					frames.push((Kind::BraceR, token.start, Slash::Unsure));
					slash = Slash::Regex;
				}
				Kind::ParenR | Kind::BracketR | Kind::BraceR => {
					let Some(frame) = frames.pop() else { break None };
					if frame.0 == Kind::Template && token.kind == Kind::BraceR {
						let chunk = lexer.template_continue(token);
						if chunk.flags & BROKEN != 0 || lexer.error.is_some() {
							break None;
						}
						if chunk.kind == Kind::TemplateTail {
							slash = Slash::Divide;
						} else {
							frames.push(frame);
							slash = Slash::Regex;
						}
					} else if frame.0 != token.kind {
						break None;
					} else {
						if token.kind == Kind::ParenR {
							closed = Some(frame.1);
						}
						slash = frame.2;
					}
				}
				Kind::Eof => break None,
				kind if (kind == Kind::Ident || kind.is_keyword()) && matches!(prev, Kind::Dot | Kind::QuestionDot) => {
					slash = Slash::Divide;
				}
				Kind::Void | Kind::Const => slash = Slash::Unsure,
				Kind::Ident => {
					let word = &self.src.as_bytes()[token.start as usize..token.end as usize];
					word_await = word == b"await" && token.flags & ESCAPED == 0;
					slash = match word {
						b"yield" | b"await" | b"of" | b"let" if token.flags & ESCAPED == 0 => Slash::Unsure,
						_ => Slash::Divide,
					};
				}
				Kind::PlusPlus | Kind::MinusMinus | Kind::Bang => slash = postfix(slash, token),
				Kind::Gt | Kind::GtGt | Kind::GtGtGt => slash = Slash::Unsure,
				kind => {
					let ends = matches!(
						kind,
						Kind::PrivateName
							| Kind::Number | Kind::BigInt
							| Kind::String | Kind::RegExp
							| Kind::This | Kind::Null
							| Kind::True | Kind::False
							| Kind::Super
					);
					slash = if ends { Slash::Divide } else { Slash::Regex };
				}
			}
			prev2 = prev;
			prev = token.kind;
			prev_word_await = word_await;
		};
		if role.is_none() {
			for frame in &frames {
				if frame.0 == Kind::ParenR {
					self.scratch.ts.roles.insert(frame.1, Role::Unsure);
				}
			}
		}
		role
	}

	/// Type arguments after an expression: a call, a tagged template, or an instantiation.
	/// The outer none means this is not one and the tokens go back.
	fn parse_type_arguments_subscript(
		&mut self,
		base: NodeId,
		start: u32,
		no_calls: bool,
		is_optional_call: bool,
	) -> Option<NodeId> {
		if !no_calls && self.at_possible_async_arrow(base) {
			let arrow = self.try_arrow_head(start, self.tok.start, None, true, true);
			if self.error {
				return Some(NodeId::at(0));
			}
			if let Some(arrow) = arrow {
				return Some(arrow);
			}
		}
		let Some(type_arguments) = self.parse_type_arguments_in_expression() else {
			return Some(base);
		};
		if self.error {
			return Some(NodeId::at(0));
		}
		if is_optional_call && self.tok.kind != Kind::ParenL {
			return None;
		}
		if matches!(self.tok.kind, Kind::Template | Kind::TemplateHead) {
			let quasi = self.template(true);
			let node = self.add(NodeKind::TaggedTemplateExpression { tag: base, quasi }, start);
			if !self.error {
				self.extras_mut(node).type_arguments = Some(type_arguments);
			}
			return Some(node);
		}
		if !no_calls && self.tok.kind == Kind::ParenL {
			self.bump(true);
			let items = self.arguments();
			let arguments = self.list(items);
			let node = self.add(
				NodeKind::CallExpression {
					callee: base,
					arguments,
					optional: is_optional_call,
				},
				start,
			);
			if !self.error {
				self.extras_mut(node).type_arguments = Some(type_arguments);
			}
			return Some(node);
		}
		if matches!(self.tok.kind, Kind::Gt | Kind::GtGt | Kind::GtGtGt | Kind::LtLt)
			|| (self.tok.kind != Kind::ParenL && self.starts_expression() && !self.newline())
		{
			return None;
		}
		Some(self.add(
			NodeKind::InstantiationExpression {
				expression: base,
				type_arguments,
			},
			start,
		))
	}

	/// The token at the cursor can start an expression, as the parser in use tells it.
	fn starts_expression(&self) -> bool {
		matches!(
			self.tok.kind,
			Kind::Ident
				| Kind::PrivateName
				| Kind::Number | Kind::BigInt
				| Kind::String | Kind::RegExp
				| Kind::ParenL | Kind::BracketL
				| Kind::BraceL | Kind::Template
				| Kind::TemplateHead
				| Kind::Slash | Kind::SlashEq
				| Kind::Bang | Kind::Tilde
				| Kind::Plus | Kind::Minus
				| Kind::PlusPlus
				| Kind::MinusMinus
				| Kind::This | Kind::Super
				| Kind::Function
				| Kind::Class | Kind::New
				| Kind::Typeof | Kind::Void
				| Kind::Delete | Kind::Import
				| Kind::Null | Kind::True
				| Kind::False
		)
	}

	// ── statements and modules

	/// First look at a statement; an answer replaces it entirely.
	#[cold]
	pub(super) fn ts_statement(&mut self) -> Option<NodeId> {
		if self.tok.kind == Kind::At {
			self.parse_decorators(true);
			if self.failed() {
				return Some(self.fail_if_clean());
			}
		}
		let start = self.tok.start;
		if self.head().is_some() || self.tok.kind == Kind::Const {
			if let Some(opened) = self.open_declaration() {
				return Some(self.parse_opened(start, opened));
			}
			if self.failed() {
				return Some(self.fail_if_clean());
			}
		}
		None
	}

	/// The parse has ended by the lexer's error and the grammar has none of its own yet: the
	/// grammar's is whatever stands at the end.
	fn fail_if_clean(&mut self) -> NodeId {
		if !self.error {
			self.fail();
		}
		NodeId::at(0)
	}

	/// Right after `export`; an answer is a whole export statement.
	pub(super) fn export_head(&mut self, start: u32) -> Option<NodeId> {
		if self.tok.kind == Kind::Import {
			self.tnext();
			if self.is_word(b"type") && self.lexer.peek_byte() != Some(b'=') {
				self.scratch.ts.state.outer_kind = Some(ImportKind::Type);
				self.tnext();
			} else {
				self.scratch.ts.state.outer_kind = Some(ImportKind::Value);
			}
			let node = self.parse_import_equals_declaration(start, true);
			self.scratch.ts.state.outer_kind = None;
			return Some(node);
		}
		if self.tok.kind == Kind::Eq {
			self.bump(true);
			if self.scratch.ts.state.in_namespace {
				return Some(self.raise(start, Code::ExportAssignmentInNamespace));
			}
			let expression = self.sequence();
			self.tsemicolon();
			return Some(self.add(NodeKind::ExportAssignment { expression }, start));
		}
		if self.eat_word(b"as") {
			self.expect_contextual(b"namespace");
			let id = self.ident();
			self.tsemicolon();
			return Some(self.add(NodeKind::NamespaceExportDeclaration { id }, start));
		}
		if self.is_word(b"type") && matches!(self.peek_tok().kind, Kind::BraceL | Kind::Star) {
			self.tnext();
			self.scratch.ts.state.outer_kind = Some(ImportKind::Type);
			self.scratch.ts.state.export_kind = Some(ImportKind::Type);
		} else {
			if self.failed() {
				return Some(self.fail_if_clean());
			}
			self.scratch.ts.state.outer_kind = Some(ImportKind::Value);
		}
		None
	}

	pub(super) fn export_declaration_ts(&mut self) -> Option<NodeId> {
		if self.tok.kind == Kind::At {
			return Some(self.statement(1));
		}
		let start = self.tok.start;
		if self.head().is_none_or(|head| head == Head::Global) {
			return None;
		}
		let Some(opened) = self.open_declaration() else {
			if self.failed() {
				return Some(self.fail_if_clean());
			}
			if self.head() == Some(Head::Declare) && self.peek_tok().flags & NEWLINE_BEFORE == 0 {
				if self.failed() {
					return Some(self.fail_if_clean());
				}
				self.tnext();
				return Some(self.raise(self.tok.start, Code::ExportDeclareWithoutDeclaration));
			}
			if self.failed() {
				return Some(self.fail_if_clean());
			}
			return None;
		};
		let declaration = self.parse_opened(start, opened);
		if self.error {
			return Some(declaration);
		}
		if let NodeKind::ModuleDeclaration { global: true, id, .. } = self.kind_of(declaration) {
			let at = self.start_of(id);
			return Some(self.raise(at, Code::ExportDeclareWithoutDeclaration));
		}
		if matches!(
			self.kind_of(declaration),
			NodeKind::InterfaceDeclaration { .. } | NodeKind::TypeAliasDeclaration { .. }
		) {
			self.scratch.ts.state.export_kind = Some(ImportKind::Type);
		}
		Some(declaration)
	}

	pub(super) fn export_default_ts(&mut self) -> Option<NodeId> {
		if self.tok.kind == Kind::At {
			self.parse_decorators(false);
			if self.failed() {
				return Some(self.fail_if_clean());
			}
		}
		let start = self.tok.start;
		if self.head() == Some(Head::Abstract) {
			let next = self.peek_tok();
			if self.failed() {
				return Some(self.fail_if_clean());
			}
			if next.kind == Kind::Class && next.flags & NEWLINE_BEFORE == 0 {
				self.tnext();
				self.scratch.ts.state.next_class = ClassFrame {
					is_abstract: true,
					start: Some(start),
					..ClassFrame::default()
				};
				return Some(self.class(true, true));
			}
		}
		if self.head() == Some(Head::Interface) {
			self.tnext();
			return Some(self.parse_interface_declaration(start));
		}
		None
	}

	/// With `erase`: an export of a name the module declares only as a type is an export of a type.
	pub(super) fn mark_type_exports(&mut self, base: usize) {
		for i in base..self.scratch.items.len() {
			let Some(statement) = self.scratch.items[i] else { continue };
			match self.kind_of(statement) {
				NodeKind::ExportNamedDeclaration {
					specifiers,
					source: None,
					..
				} => {
					for j in 0..specifiers.len {
						let Some(spec) = self.ast.nth(specifiers, j) else { continue };
						let NodeKind::ExportSpecifier { local, .. } = self.kind_of(spec) else {
							unreachable!()
						};
						if let NodeKind::Identifier { name } = self.kind_of(local)
							&& self.scratch.checks.declares_only_types(name)
						{
							self.extras_mut(spec).export_kind = Some(ImportKind::Type);
						}
					}
				}
				NodeKind::ExportDefaultDeclaration { declaration } => {
					if let NodeKind::Identifier { name } = self.kind_of(declaration)
						&& self.scratch.checks.declares_only_types(name)
					{
						self.extras_mut(statement).export_kind = Some(ImportKind::Type);
					}
				}
				_ => {}
			}
		}
	}

	pub(super) fn export_end(&mut self, node: NodeId) {
		if self.error {
			return;
		}
		if let Some(kind) = self.scratch.ts.state.export_kind.take() {
			self.extras_mut(node).export_kind = Some(kind);
		}
		self.scratch.ts.state.outer_kind = None;
	}

	pub(super) fn export_specifier_ts(&mut self) -> Option<NodeId> {
		if self.is_word(b"type") {
			return Some(self.parse_type_only_specifier(false));
		}
		None
	}

	/// Right after `import`; an answer is a whole import statement.
	pub(super) fn import_head(&mut self, start: u32) -> Option<NodeId> {
		self.scratch.ts.state.outer_kind = Some(ImportKind::Value);
		if (self.is_ident() || matches!(self.tok.kind, Kind::Star | Kind::BraceL))
			&& (self.is_word(b"type") || matches!(self.next_start(), None | Some(b'=' | b'\\')))
		{
			let mut ahead = self.peek_tok();
			if self.failed() {
				return Some(self.fail_if_clean());
			}
			let ahead_is_from = ahead.kind == Kind::Ident && self.word_of(ahead) == "from";
			if self.is_word(b"type") && ahead.kind != Kind::Comma && !ahead_is_from && ahead.kind != Kind::Eq {
				self.tnext();
				self.scratch.ts.state.outer_kind = Some(ImportKind::Type);
				ahead = self.peek_tok();
				if self.failed() {
					return Some(self.fail_if_clean());
				}
			}
			if self.is_ident() && ahead.kind == Kind::Eq {
				let node = self.parse_import_equals_declaration(start, false);
				self.scratch.ts.state.outer_kind = None;
				return Some(node);
			}
		}
		None
	}

	pub(super) fn import_end(&mut self, node: NodeId) {
		if self.error {
			return;
		}
		let kind = self.scratch.ts.state.outer_kind.take().unwrap_or(ImportKind::Value);
		self.extras_mut(node).import_kind = Some(kind);
		let NodeKind::ImportDeclaration { specifiers, .. } = self.kind_of(node) else {
			unreachable!()
		};
		if kind == ImportKind::Type {
			for i in 0..specifiers.len {
				let Some(specifier) = self.ast.nth(specifiers, i) else { continue };
				let (NodeKind::ImportSpecifier { local, .. }
				| NodeKind::ImportDefaultSpecifier { local }
				| NodeKind::ImportNamespaceSpecifier { local }) = self.kind_of(specifier)
				else {
					continue;
				};
				self.declare_export_only(local);
			}
		}
		if kind == ImportKind::Type
			&& specifiers.len > 1
			&& matches!(
				self.kind_of(self.ast.nth(specifiers, 0).unwrap()),
				NodeKind::ImportDefaultSpecifier { .. }
			) {
			let at = self.start_of(node);
			self.raise(at, Code::TypeImportDefaultAndNamed);
		}
	}

	pub(super) fn import_specifier_ts(&mut self) -> Option<NodeId> {
		if self.is_word(b"type") {
			return Some(self.parse_type_only_specifier(true));
		}
		None
	}

	pub(super) fn import_binding(&self) -> Binding {
		if self.scratch.ts.state.outer_kind == Some(ImportKind::Type) {
			Binding::None
		} else {
			Binding::Lexical
		}
	}

	pub(super) fn exports_in_script(&self) -> bool {
		self.scratch.ts.state.module_blocks > 0
	}

	pub(super) fn in_ambient(&self) -> bool {
		self.scratch.ts.state.ambient
	}

	// ── bindings

	/// After the id of a variable declarator; whether it is definite, `!`.
	pub(super) fn var_id(&mut self, id: NodeId) -> bool {
		let mut definite = false;
		if matches!(self.kind_of(id), NodeKind::Identifier { .. }) && !self.newline() && self.tok.kind == Kind::Bang {
			self.tnext();
			definite = true;
		}
		if self.tok.kind == Kind::Colon {
			let annotation = self.parse_type_annotation(true, None);
			if self.error {
				return definite;
			}
			self.extras_mut(id).type_annotation = Some(annotation);
			self.ast.node_mut(id).end = self.prev_end;
		}
		definite
	}

	pub(super) fn var_declarator(&mut self, node: NodeId, kind: VariableKind, definite: bool) {
		if self.error {
			return;
		}
		if definite {
			self.extras_mut(node).definite = true;
			if let NodeKind::VariableDeclarator { init: Some(_), .. } = self.kind_of(node) {
				let at = self.start_of(node);
				self.raise(at, Code::DefiniteWithInitializer);
				return;
			}
		}
		if self.scratch.ts.state.ambient {
			self.check_ambient_initializer(node, kind);
		}
	}

	/// A whole parameter of a parameter list; `allow_modifiers` inside class methods.
	#[cold]
	pub(super) fn parameter(&mut self, allow_modifiers: bool) -> NodeId {
		let mut decorators = Vec::new();
		while self.tok.kind == Kind::At {
			let decorator = self.parse_decorator();
			if self.error {
				return decorator;
			}
			decorators.push(Some(decorator));
		}
		let decorators = (!decorators.is_empty()).then(|| self.ast.add_list(&decorators));
		let start = self.tok.start;
		let mut property = None;
		if allow_modifiers {
			let extras = self
				.parse_modifiers(
					&["public", "private", "protected", "override", "readonly"],
					&[],
					false,
					None,
				)
				.extras;
			if self.failed() {
				return self.fail_if_clean();
			}
			if extras.accessibility.is_some() || extras.readonly || extras.is_override {
				property = Some(extras);
			}
		}
		let this = self.tok.kind == Kind::This;
		if !allow_modifiers
			&& !this && let Some(decorators) = decorators
		{
			self.parameter_decorator_error(decorators);
			return NodeId::at(0);
		}
		let left = if this {
			let this = self.name();
			if decorators.is_some() || property.is_some() {
				let at = self.start_of(this);
				return self.raise(at, Code::ThisParameterModifiers);
			}
			this
		} else if self.tok.kind == Kind::Ellipsis {
			let start = self.tok.start;
			self.tnext();
			let argument = self.binding_atom();
			self.add(NodeKind::RestElement { argument }, start)
		} else {
			self.binding_atom()
		};
		if self.error {
			return left;
		}
		let rest = matches!(self.kind_of(left), NodeKind::RestElement { .. });
		if !this && self.eat(Kind::Question) {
			self.extras_mut(left).optional = true;
		}
		if self.tok.kind == Kind::Colon {
			let annotation = self.parse_type_annotation(true, None);
			if self.error {
				return left;
			}
			self.extras_mut(left).type_annotation = Some(annotation);
		}
		self.ast.node_mut(left).end = self.prev_end;
		if rest && self.scratch.ts.state.ambient && self.tok.kind == Kind::Comma && self.lexer.peek_byte() == Some(b')') {
			self.tnext();
		}
		let item = if rest || this {
			left
		} else if self.tok.kind == Kind::Eq {
			let left_start = self.start_of(left);
			self.bump(true);
			let right = self.assign();
			self.add(NodeKind::AssignmentPattern { left, right }, left_start)
		} else {
			left
		};
		if self.error {
			return item;
		}
		if self.tok.kind == Kind::Colon && item != left {
			return self.raise(self.tok.start, Code::TypeAnnotationAfterDefault);
		}
		if decorators.is_some() {
			self.extras_mut(item).decorators = decorators;
		}
		let Some(extras) = property else {
			return item;
		};
		if !matches!(self.kind_of(left), NodeKind::Identifier { .. }) {
			return self.raise(start, Code::ParameterPropertyPattern);
		}
		let node = self.add(NodeKind::ParameterProperty { parameter: item }, start);
		*self.extras_mut(node) = extras;
		node
	}

	pub(super) fn catch_param(&mut self, param: NodeId) {
		if self.error {
			return;
		}
		if self.tok.kind == Kind::Colon {
			let annotation = self.parse_type_annotation(true, None);
			if self.error {
				return;
			}
			let NodeKind::TypeAnnotation { type_annotation } = self.kind_of(annotation) else {
				unreachable!()
			};
			if !matches!(
				self.kind_of(type_annotation),
				NodeKind::Keyword(TsKeyword::Any | TsKeyword::Unknown)
			) {
				let at = self.start_of(type_annotation);
				self.raise(at, Code::CatchClauseType);
				return;
			}
			self.extras_mut(param).type_annotation = Some(annotation);
			self.ast.node_mut(param).end = self.prev_end;
		}
	}

	// ── functions and classes

	/// Before the parameters of a function, method or arrow.
	pub(super) fn function_start(&mut self, kind: FunctionKind) -> FunctionFrame {
		let mut frame = FunctionFrame {
			in_class_method: matches!(kind, FunctionKind::Method { in_class: true, .. }),
			..FunctionFrame::default()
		};
		match kind {
			FunctionKind::Arrow => {
				frame.return_type = self.scratch.ts.state.arrow_return_type.take();
				if std::mem::take(&mut self.scratch.ts.state.body_needs_colon) && self.tok.kind != Kind::BraceL {
					self.scratch.ts.state.needs_colon_at = Some(self.tok.start);
				}
			}
			FunctionKind::Method { in_class: true, .. } => {}
			_ => frame.type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::Const),
		}
		if self.error {
			return frame;
		}
		if matches!(
			kind,
			FunctionKind::Method {
				kind: MethodKind::Get | MethodKind::Set,
				..
			}
		) && let Some(type_parameters) = frame.type_parameters
		{
			let at = self.start_of(type_parameters);
			self.raise(at, Code::AccessorTypeParameters);
		}
		frame
	}

	/// After the parameters, before the body; an answer is a function without a body.
	#[expect(clippy::too_many_arguments)]
	pub(super) fn function_body_ts(
		&mut self,
		frame: &mut FunctionFrame,
		start: u32,
		id: Option<NodeId>,
		params: List,
		is_async: bool,
		generator: bool,
		kind: FunctionKind,
	) -> Option<NodeId> {
		if self.error {
			return None;
		}
		if matches!(
			kind,
			FunctionKind::Method {
				kind: MethodKind::Set,
				..
			}
		) {
			for i in 0..params.len {
				let Some(param) = self.ast.nth(params, i) else { continue };
				if self.ident_is(param, "this") {
					continue;
				}
				if self.ast.extras.get(param).is_some_and(|extras| extras.optional) {
					let at = self.start_of(param);
					self.raise(at, Code::SetterOptionalParameter);
					return None;
				}
				if matches!(self.kind_of(param), NodeKind::AssignmentPattern { .. }) {
					let at = self.start_of(param);
					self.raise(at, Code::SetterParameterInitializer);
					return None;
				}
			}
		}
		if self.tok.kind == Kind::Colon {
			let allow_predicate = !matches!(
				kind,
				FunctionKind::Method {
					kind: MethodKind::Get,
					..
				}
			);
			let return_type = self.parse_type_or_type_predicate_annotation(Kind::Colon, allow_predicate);
			if self.error {
				return None;
			}
			match kind {
				FunctionKind::Method {
					kind: MethodKind::Set, ..
				} => {
					let at = self.start_of(return_type);
					self.raise(at, Code::SetterReturnType);
					return None;
				}
				FunctionKind::Method {
					kind: MethodKind::Constructor,
					..
				} => {
					let at = self.start_of(return_type);
					self.raise(at, Code::ConstructorReturnType);
					return None;
				}
				_ => {}
			}
			frame.return_type = Some(return_type);
		}
		let bodiless = match kind {
			FunctionKind::Declaration => Some(NodeKind::DeclareFunction {
				id,
				params,
				is_async,
				generator,
			}),
			FunctionKind::Method { in_class: true, .. } => Some(NodeKind::DeclareMethod {
				params,
				is_async,
				generator,
			}),
			_ => None,
		};
		if generator && self.scratch.ts.state.ambient && bodiless.is_some() {
			self.raise(start, Code::GeneratorInAmbient);
			return None;
		}
		let signature = bodiless.is_some() && self.tok.kind != Kind::BraceL && self.is_line_terminator();
		self.check_parameter_list(params, signature);
		if self.error {
			return None;
		}
		if signature && let Some(kind) = bodiless {
			if generator {
				self.raise(start, Code::GeneratorSignature);
				return None;
			}
			return Some(self.add(kind, start));
		}
		if self.scratch.ts.state.ambient && bodiless.is_some() {
			self.raise(start, Code::ImplementationInAmbient);
		}
		None
	}

	pub(super) fn function_end(&mut self, node: NodeId, frame: FunctionFrame) {
		if self.error {
			return;
		}
		if let NodeKind::ArrowFunctionExpression { params, .. } = self.kind_of(node) {
			self.check_parameter_list(params, false);
			if self.error {
				return;
			}
		}
		if let NodeKind::DeclareFunction { id: Some(id), .. } = self.kind_of(node) {
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
			&& matches!(self.kind_of(node), NodeKind::FunctionExpression { .. })
			&& let Some(element) = self.scratch.ts.state.elements.last()
			&& element.extras.is_abstract
		{
			let (start, element) = (element.start, *element);
			let name = self.element_name(element);
			self.raise_name(start, Code::AbstractWithImplementation, name);
		}
	}

	pub(super) fn accessor_this_param(&self, params: List) -> bool {
		self.ast
			.list(params)
			.first()
			.is_some_and(|param| param.is_some_and(|param| self.ident_is(param, "this")))
	}

	pub(super) fn function_params(&self, node: NodeId) -> Option<List> {
		match self.kind_of(node) {
			NodeKind::DeclareFunction { params, .. } | NodeKind::DeclareMethod { params, .. } => Some(params),
			_ => None,
		}
	}

	pub(super) fn class_start(&mut self, kind: ClassKind) {
		let mut frame = std::mem::take(&mut self.scratch.ts.state.next_class);
		frame.is_declaration = kind != ClassKind::Expression;
		if let Some(decorators) = self.take_decorators() {
			frame.decorators = Some(decorators);
			if !std::mem::take(&mut self.scratch.ts.state.decorators_before_export) {
				let first = self.ast.nth(decorators, 0).unwrap();
				frame.start = Some(self.start_of(first));
			}
		}
		frame.outer_abstract = self.scratch.ts.state.in_abstract_class;
		self.scratch.ts.state.in_abstract_class = frame.is_abstract;
		self.scratch.ts.state.classes.push(frame);
	}

	pub(super) fn starts_class_heritage(&mut self) -> bool {
		self.is_word(b"implements")
	}

	pub(super) fn class_type_parameters(&mut self) {
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::Class);
		if let Some(frame) = self.scratch.ts.state.classes.last_mut() {
			frame.type_parameters = type_parameters;
		}
	}

	pub(super) fn class_heritage(&mut self, has_super: bool) {
		if self.error {
			return;
		}
		let mut super_type_arguments = None;
		if has_super && matches!(self.tok.kind, Kind::Lt | Kind::LtLt) {
			super_type_arguments = self.parse_type_arguments_in_expression();
		}
		let implements = if self.eat_word(b"implements") {
			let base = self.parse_heritage_clause("implements");
			Some(self.list(base))
		} else {
			None
		};
		let frame = self.scratch.ts.state.classes.last_mut().unwrap();
		frame.has_super = has_super;
		frame.super_type_arguments = super_type_arguments;
		frame.implements = implements;
	}

	pub(super) fn class_end(&mut self, node: NodeId) {
		if self.error {
			return;
		}
		let frame = self.scratch.ts.state.classes.pop().unwrap();
		self.scratch.ts.state.in_abstract_class = frame.outer_abstract;
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

	/// Modifiers before a class element; whether `static` was among them.
	fn class_modifiers(&mut self) -> bool {
		let mut decorators = Vec::new();
		while self.tok.kind == Kind::At {
			let decorator = self.parse_decorator();
			if self.error {
				return false;
			}
			decorators.push(Some(decorator));
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
		);
		if self.failed() {
			self.fail_if_clean();
			return false;
		}
		if !decorators.is_empty() {
			if self.tok.kind == Kind::BraceR {
				let at = self.lexer.pos() as u32;
				self.raise(at, Code::DecoratorPlacement);
				return false;
			}
			modifiers.extras.decorators = Some(self.ast.add_list(&decorators));
		}
		if modifiers != Modifiers::default() && self.is_word(b"static") && self.lexer.peek_byte() == Some(b'{') {
			let snapshot = self.tok_snap();
			self.tnext();
			self.tnext();
			if self.failed() {
				self.fail_if_clean();
				return false;
			}
			let inside = self.tok.start;
			self.restore_tokens(snapshot);
			self.raise(inside, Code::StaticBlockModifier);
			return false;
		}
		let outer_ambient = modifiers
			.extras
			.declare
			.then(|| std::mem::replace(&mut self.scratch.ts.state.ambient, true));
		self.scratch.ts.state.elements.push(ElementFrame {
			start,
			extras: modifiers.extras,
			key: None,
			computed: false,
			outer_ambient,
			accessor: modifiers.accessor,
		});
		modifiers.extras.is_static
	}

	fn class_index_signature(&mut self, start: u32) -> Option<NodeId> {
		let Some(signature) = self.try_parse_index_signature(start) else {
			if self.failed() {
				self.fail_if_clean();
				return None;
			}
			let has_super = self.scratch.ts.state.classes.last().is_some_and(|c| c.has_super);
			let extras = self.scratch.ts.state.elements.last().unwrap().extras;
			if !self.scratch.ts.state.in_abstract_class && extras.is_abstract {
				self.raise(start, Code::AbstractOutsideAbstractClass);
				return None;
			}
			if extras.is_override && !has_super {
				self.raise(start, Code::OverrideWithoutExtends);
			}
			return None;
		};
		if self.error {
			return Some(signature);
		}
		let ElementFrame { extras, accessor, .. } = *self.scratch.ts.state.elements.last().unwrap();
		if extras.is_abstract {
			return Some(self.raise_arg(start, Code::IndexSignatureModifier, "abstract"));
		}
		if let Some(accessibility) = extras.accessibility {
			return Some(self.raise_arg(start, Code::IndexSignatureModifier, accessibility.as_str()));
		}
		if extras.declare {
			return Some(self.raise_arg(start, Code::IndexSignatureModifier, "declare"));
		}
		if extras.is_override {
			return Some(self.raise_arg(start, Code::IndexSignatureModifier, "override"));
		}
		if accessor {
			return Some(self.raise_arg(start, Code::IndexSignatureModifier, "accessor"));
		}
		Some(signature)
	}

	fn class_key_end(&mut self, key: NodeId, computed: bool) {
		let element = self.scratch.ts.state.elements.last_mut().unwrap();
		element.key = Some(key);
		element.computed = computed;
		let start = element.start;
		let extras = element.extras;
		let accessor = element.accessor;
		if self.eat(Kind::Question) {
			self.scratch.ts.state.elements.last_mut().unwrap().extras.optional = true;
		}
		let method = matches!(self.tok.kind, Kind::ParenL | Kind::Lt);
		if extras.readonly && method {
			self.raise(start, Code::ReadonlyOnMethod);
			return;
		}
		if extras.declare && method {
			self.raise(start, Code::DeclareOnMethod);
			return;
		}
		if accessor && method {
			self.raise(start, Code::AccessorPlacement);
		}
	}

	fn class_method_start(&mut self, kind: MethodKind) {
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::Const);
		if self.error {
			return;
		}
		if matches!(kind, MethodKind::Get | MethodKind::Set)
			&& let Some(type_parameters) = type_parameters
		{
			let at = self.start_of(type_parameters);
			self.raise(at, Code::AccessorTypeParameters);
			return;
		}
		let element = *self.scratch.ts.state.elements.last().unwrap();
		let key = element.key.unwrap();
		if matches!(self.kind_of(key), NodeKind::PrivateIdentifier { .. }) {
			if let Some(accessibility) = element.extras.accessibility {
				self.raise_arg(element.start, Code::PrivateModifier, accessibility.as_str());
				return;
			}
		} else if let Some(type_parameters) = type_parameters
			&& !element.extras.is_static
			&& !element.computed
			&& matches!(self.kind_of(key), NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } if self.ast.strings.is(name, "constructor"))
		{
			let at = self.start_of(type_parameters);
			self.raise(at, Code::ConstructorTypeParameters);
			return;
		}
		self.scratch.ts.state.elements.last_mut().unwrap().extras.type_parameters = type_parameters;
	}

	fn class_field_annotation(&mut self) {
		let element = *self.scratch.ts.state.elements.last().unwrap();
		let mut definite = false;
		let mut optional = element.extras.optional;
		if !optional {
			if self.tok.kind == Kind::Bang && !self.newline() {
				self.tnext();
				definite = true;
			} else {
				optional = self.eat(Kind::Question);
			}
		}
		let type_annotation = self.try_parse_type_annotation();
		if self.error {
			return;
		}
		let extras = &mut self.scratch.ts.state.elements.last_mut().unwrap().extras;
		extras.definite = definite;
		extras.optional = optional;
		extras.type_annotation = type_annotation;
		let private = element
			.key
			.is_some_and(|key| matches!(self.kind_of(key), NodeKind::PrivateIdentifier { .. }));
		if private {
			if element.extras.is_abstract {
				self.raise_arg(element.start, Code::PrivateModifier, "abstract");
				return;
			}
			if let Some(accessibility) = element.extras.accessibility {
				self.raise_arg(element.start, Code::PrivateModifier, accessibility.as_str());
			}
		} else if self.tok.kind == Kind::Eq {
			if self.scratch.ts.state.ambient && !(element.extras.readonly && type_annotation.is_none()) {
				self.raise(self.tok.start, Code::InitializerInAmbient);
				return;
			}
			if element.extras.is_abstract {
				let name = self.element_name(element);
				self.raise_name(self.tok.start, Code::AbstractWithInitializer, name);
			}
		}
	}

	fn class_element_end(&mut self, node: NodeId) {
		if self.error {
			return;
		}
		let frame = self.scratch.ts.state.elements.pop().unwrap();
		if let Some(outer) = frame.outer_ambient {
			self.scratch.ts.state.ambient = outer;
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
		} = self.kind_of(node)
			&& matches!(kind, MethodKind::Constructor | MethodKind::Method)
			&& matches!(self.kind_of(key), NodeKind::Identifier { name } | NodeKind::StringLiteral { value: name } if self.ast.strings.is(name, "constructor"))
		{
			if frame.extras.is_abstract {
				self.raise(frame.start, Code::AbstractPlacement);
				return;
			}
			let modifier = if frame.extras.is_static {
				Some("static")
			} else if frame.extras.is_override {
				Some("override")
			} else {
				None
			};
			if let Some(modifier) = modifier {
				self.raise_arg(frame.start, Code::ConstructorModifier, modifier);
				return;
			}
		}
		if let Some(decorators) = frame.extras.decorators {
			let first = self.ast.nth(decorators, 0).unwrap();
			let start = self.start_of(first);
			self.ast.node_mut(node).start = start;
			if let NodeKind::MethodDefinition { kind, value, .. } = self.kind_of(node) {
				if kind == MethodKind::Constructor {
					self.raise(start, Code::DecoratorOnConstructor);
					return;
				}
				if matches!(self.kind_of(value), NodeKind::DeclareMethod { .. }) {
					self.raise(start, Code::DecoratorWithoutBody);
					return;
				}
			}
			if matches!(self.kind_of(node), NodeKind::IndexSignature { .. }) {
				self.raise(start, Code::DecoratorPlacement);
				return;
			}
		}
		if let NodeKind::MethodDefinition { kind, value, .. } = self.kind_of(node) {
			let (params, has_body) = match self.kind_of(value) {
				NodeKind::FunctionExpression { function } => (function.params, true),
				_ => (self.function_params(value).unwrap(), false),
			};
			if !has_body || kind == MethodKind::Get || !self.scratch.ts.state.classes.last().unwrap().is_declaration {
				for i in 0..params.len {
					let Some(param) = self.ast.nth(params, i) else { continue };
					let param = self.unwrap_inner_pattern(param).unwrap_or(param);
					if let Some(decorators) = self.ast.extras.get(param).and_then(|extras| extras.decorators) {
						self.parameter_decorator_error(decorators);
						return;
					}
				}
			}
		}
		let mut extras = frame.extras;
		extras.is_static &= matches!(self.kind_of(node), NodeKind::IndexSignature { .. });
		if extras != Extras::default() {
			*self.extras_mut(node) = extras;
		}
	}

	/// One element of a class body, as the parser in use reads it with TypeScript on; none
	/// for a `;`.
	pub(super) fn ts_class_element(&mut self, constructor_allows_super: bool) -> Option<NodeId> {
		if self.eat(Kind::Semi) {
			return None;
		}
		let start = self.tok.start;
		let mut key_name: Option<&'static str> = None;
		let mut generator = false;
		let mut is_async = false;
		let mut kind = MethodKind::Method;
		let mut is_static = self.class_modifiers();
		if self.error {
			return Some(NodeId::at(0));
		}
		if let Some(signature) = self.class_index_signature(start) {
			self.class_element_end(signature);
			return Some(signature);
		}
		if self.error {
			return Some(NodeId::at(0));
		}
		if !is_static && self.is_word(b"static") && self.lexer.peek_byte() == Some(b'{') && self.eat_word(b"static") {
			if self.tok.kind == Kind::BraceL {
				self.bump(true);
				let block = self.static_block(start);
				self.class_element_end(block);
				return Some(block);
			}
			if self.at_class_name() || self.tok.kind == Kind::Star {
				is_static = true;
			} else {
				key_name = Some("static");
			}
		}
		if key_name.is_none() && self.eat_word(b"async") {
			if (self.at_class_name() || self.tok.kind == Kind::Star) && !self.can_insert_semi() {
				is_async = true;
			} else {
				key_name = Some("async");
			}
		}
		if key_name.is_none() && self.eat(Kind::Star) {
			generator = true;
		}
		if key_name.is_none() && !is_async && !generator {
			let accessor = if self.is_word(b"get") {
				Some(("get", MethodKind::Get))
			} else if self.is_word(b"set") {
				Some(("set", MethodKind::Set))
			} else {
				None
			};
			if let Some((name, accessor_kind)) = accessor {
				self.tnext();
				if self.at_class_name() {
					kind = accessor_kind;
				} else {
					key_name = Some(name);
				}
			}
		}
		let (key, computed) = match key_name {
			Some(name) => {
				let key_start = self.prev_end - name.len() as u32;
				let name = self.ast.strings.intern(name);
				(self.ast.add(NodeKind::Identifier { name }, key_start, self.prev_end), false)
			}
			None if self.tok.kind == Kind::PrivateName => {
				let t = self.tok;
				if t.flags & ESCAPED == 0 && &self.src[t.start as usize + 1..t.end as usize] == "constructor" {
					return Some(self.raise(t.start, Code::PrivateConstructor));
				}
				(self.private_name(), false)
			}
			None => self.property_name(),
		};
		if self.error {
			return Some(key);
		}
		self.class_key_end(key, computed);
		if self.error {
			return Some(key);
		}
		let key_start = self.start_of(key);
		if matches!(self.tok.kind, Kind::ParenL | Kind::Lt) || kind != MethodKind::Method || generator || is_async {
			let is_constructor = !is_static && self.key_is(key, computed, "constructor");
			let allows_direct_super = is_constructor && constructor_allows_super;
			if is_constructor && kind != MethodKind::Method {
				return Some(self.raise(key_start, Code::AccessorConstructor));
			}
			if is_constructor {
				kind = MethodKind::Constructor;
			}
			if kind == MethodKind::Constructor {
				if generator {
					return Some(self.raise(key_start, Code::GeneratorConstructor));
				}
				if is_async {
					return Some(self.raise(key_start, Code::AsyncConstructor));
				}
			} else if is_static && !self.in_ambient() && self.key_is(key, computed, "prototype") {
				return Some(self.raise(key_start, Code::StaticPrototype));
			}
			self.class_method_start(kind);
			if self.error {
				return Some(key);
			}
			let value = self.method_ts(generator, is_async, allows_direct_super, true, kind);
			if self.error {
				return Some(value);
			}
			if kind == MethodKind::Get || kind == MethodKind::Set {
				self.check_accessor_params_ts(value, kind == MethodKind::Get, true);
				if self.error {
					return Some(value);
				}
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
			self.class_element_end(node);
			return Some(node);
		}
		if self.key_is(key, computed, "constructor") {
			return Some(self.raise(key_start, Code::ConstructorField));
		}
		if is_static && !self.in_ambient() && self.key_is(key, computed, "prototype") {
			return Some(self.raise(key_start, Code::StaticPrototype));
		}
		self.class_field_annotation();
		if self.error {
			return Some(key);
		}
		let value = if self.tok.kind == Kind::Eq {
			self.bump(true);
			self.enter_scope(super::checks::SCOPE_CLASS_FIELD_INIT | super::checks::SCOPE_SUPER);
			let outer = std::mem::replace(&mut self.ctx, Ctx::default());
			let value = self.assign();
			self.ctx = outer;
			self.exit_scope();
			Some(value)
		} else {
			None
		};
		self.class_semicolon();
		let node = self.add(
			NodeKind::PropertyDefinition {
				key,
				value,
				computed,
				is_static,
				kind: FieldKind::Property,
			},
			start,
		);
		self.class_element_end(node);
		Some(node)
	}

	pub(super) fn check_accessor_params_ts(&mut self, func: NodeId, is_getter: bool, in_class: bool) {
		if self.error {
			return;
		}
		let params = match self.kind_of(func) {
			NodeKind::FunctionExpression { function } => function.params,
			_ => match self.function_params(func) {
				Some(params) => params,
				None => return,
			},
		};
		let this_param = !in_class && self.accessor_this_param(params);
		let expected = if is_getter { 0 } else { 1 } + u32::from(this_param);
		if params.len != expected {
			let start = self.start_of(func);
			self.raise(start, if is_getter { Code::GetterParams } else { Code::SetterParams });
			return;
		}
		if !is_getter {
			let param = self.ast.nth(params, 0).unwrap();
			if matches!(self.kind_of(param), NodeKind::RestElement { .. }) {
				let at = self.start_of(param);
				self.raise(at, Code::SetterRestParam);
			}
		}
	}

	// ── expressions the grammar meets

	/// Where an assignment expression starts with `<`: a generic arrow function, tried.
	#[cold]
	pub(super) fn maybe_assign_ts(&mut self) -> Option<NodeId> {
		if !self.starts_with_lt() {
			return None;
		}
		self.try_arrow_head(self.tok.start, self.tok.start, None, false, true)
	}

	fn arrow_return_type(&mut self) {
		self.scratch.ts.state.arrow_return_type = if self.tok.kind == Kind::Colon {
			Some(self.parse_type_or_type_predicate_annotation(Kind::Colon, true))
		} else {
			None
		};
	}

	pub(super) fn arrow_start(&mut self, start: u32) {
		self.scratch.ts.state.body_needs_colon = self.scratch.ts.state.needs_colon_at == Some(start);
	}

	/// A branch of a conditional: an arrow function in it must leave the `:` the conditional
	/// needs.
	pub(super) fn conditional_branch(&mut self, start: u32, alternate: bool) -> NodeId {
		let old = self.scratch.ts.state.needs_colon_at;
		if !alternate || old == Some(start) {
			self.scratch.ts.state.needs_colon_at = Some(self.tok.start);
		}
		let branch = self.assign();
		self.scratch.ts.state.needs_colon_at = old;
		branch
	}

	/// `left as T` or `left satisfies T`, the word at the cursor.
	#[cold]
	pub(super) fn assertion(&mut self, left: NodeId, left_start: u32, is_as: bool) -> NodeId {
		let type_annotation = match self.try_next_parse_constant_context() {
			Some(constant) => constant,
			None => {
				if self.failed() {
					return self.fail_if_clean();
				}
				self.next_then_parse_type()
			}
		};
		if self.error {
			return NodeId::at(0);
		}
		let kind = if is_as {
			self.check_const_assertion(left, type_annotation);
			if self.error {
				return NodeId::at(0);
			}
			NodeKind::AsExpression {
				expression: left,
				type_annotation,
			}
		} else {
			NodeKind::SatisfiesExpression {
				expression: left,
				type_annotation,
			}
		};
		let node = self.add(kind, left_start);
		self.rescan_lt_gt();
		node
	}

	/// What TypeScript reads after an operand; none where it reads nothing, and the pair is
	/// the new operand with whether it is an optional call.
	#[cold]
	pub(super) fn subscript_ts(
		&mut self,
		base: NodeId,
		start: u32,
		no_calls: bool,
		optional_chained: bool,
	) -> Option<(NodeId, bool)> {
		if self.tok.kind == Kind::Bang {
			if self.newline() {
				return None;
			}
			self.tnext();
			if self.track {
				self.stop_after_operand();
			}
			let node = self.add(NodeKind::NonNullExpression { expression: base }, start);
			return Some((node, false));
		}
		let optional_call = self.tok.kind == Kind::QuestionDot;
		if optional_call && self.lexer.peek_byte() != Some(b'<') {
			return None;
		}
		if optional_call && no_calls {
			return Some((base, false));
		}
		let subscript = self.attempt(|p| {
			if optional_call {
				p.tnext();
			}
			match p.parse_type_arguments_subscript(base, start, no_calls, optional_call) {
				Some(node) => (node, optional_call),
				None => {
					p.fail();
					(NodeId::at(0), false)
				}
			}
		});
		if self.error {
			return Some((NodeId::at(0), false));
		}
		let (node, is_optional_call) = subscript?;
		if optional_chained && let NodeKind::TaggedTemplateExpression { quasi, .. } = self.kind_of(node) {
			let at = self.start_of(quasi);
			self.record(SyntaxError::new(at, Code::OptionalChainInTaggedTemplate).to(at + 1));
			return Some((node, false));
		}
		if matches!(self.kind_of(node), NodeKind::InstantiationExpression { .. })
			&& (self.tok.kind == Kind::Dot
				|| (self.tok.kind == Kind::QuestionDot && self.lexer.peek_byte() != Some(b'(')))
		{
			self.raise(self.tok.start, Code::PropertyAfterInstantiation);
			return Some((node, false));
		}
		Some((node, is_optional_call))
	}

	pub(super) fn new_expression_ts(&mut self, node: NodeId) {
		if self.error {
			return;
		}
		let NodeKind::NewExpression { callee, arguments } = self.kind_of(node) else {
			return;
		};
		if let NodeKind::InstantiationExpression {
			expression,
			type_arguments,
		} = self.kind_of(callee)
		{
			self.ast.node_mut(node).kind = NodeKind::NewExpression {
				callee: expression,
				arguments,
			};
			self.extras_mut(node).type_arguments = Some(type_arguments);
		}
	}

	// ── declarations

	fn parse_enum_declaration(&mut self, start: u32, is_const: bool) -> NodeId {
		self.expect_contextual(b"enum");
		let id = self.ident();
		self.check_lval_simple(id, Binding::None, false);
		if self.error {
			return id;
		}
		self.declare_export_only(id);
		self.expect(Kind::BraceL, false);
		let base = self.parse_delimited_list(ListKind::EnumMembers, |p| p.parse_enum_member());
		self.expect(Kind::BraceR, false);
		let members = self.list(base);
		self.add(NodeKind::EnumDeclaration { id, members, is_const }, start)
	}

	fn parse_enum_member(&mut self) -> NodeId {
		let start = self.tok.start;
		let id = if self.tok.kind == Kind::String {
			self.atom(&mut None, false)
		} else {
			self.name()
		};
		if self.error {
			return id;
		}
		let initializer = if self.tok.kind == Kind::Eq {
			self.bump(true);
			self.enter_scope(SCOPE_FUNCTION);
			let outer = std::mem::replace(
				&mut self.ctx,
				Ctx {
					function: true,
					..Ctx::default()
				},
			);
			let initializer = self.assign();
			self.ctx = outer;
			self.exit_scope();
			Some(initializer)
		} else {
			None
		};
		self.add(NodeKind::EnumMember { id, initializer }, start)
	}

	fn parse_module_body(&mut self, namespace: bool) -> NodeId {
		self.enter_scope(SCOPE_TS_MODULE);
		let body = self.parse_module_block(namespace);
		self.exit_scope();
		body
	}

	fn parse_global_body(&mut self) -> NodeId {
		self.enter_scope(SCOPE_TS_MODULE);
		self.scratch.ts.state.global_depth += 1;
		let body = self.parse_module_block(false);
		if self.error {
			return body;
		}
		self.scratch.ts.state.global_depth -= 1;
		let declared = self.scratch.checks.current_declared();
		for name in declared {
			self.scratch.checks.declare_global(name);
		}
		self.exit_scope();
		body
	}

	fn is_declaration(&self, statement: NodeId) -> bool {
		matches!(
			self.kind_of(statement),
			NodeKind::VariableDeclaration { .. }
				| NodeKind::FunctionDeclaration { .. }
				| NodeKind::ClassDeclaration { .. }
				| NodeKind::ImportDeclaration { .. }
				| NodeKind::ExportDeclaration { .. }
				| NodeKind::ExportNamedDeclaration { .. }
				| NodeKind::ExportDefaultDeclaration { .. }
				| NodeKind::ExportAllDeclaration { .. }
				| NodeKind::InterfaceDeclaration { .. }
				| NodeKind::TypeAliasDeclaration { .. }
				| NodeKind::EnumDeclaration { .. }
				| NodeKind::ModuleDeclaration { .. }
				| NodeKind::DeclareFunction { .. }
				| NodeKind::ImportEqualsDeclaration { .. }
				| NodeKind::ExportAssignment { .. }
				| NodeKind::NamespaceExportDeclaration { .. }
		)
	}

	fn parse_module_block(&mut self, namespace: bool) -> NodeId {
		let start = self.tok.start;
		self.enter_scope(0);
		self.scratch.ts.state.module_blocks += 1;
		let in_namespace = std::mem::replace(&mut self.scratch.ts.state.in_namespace, namespace);
		self.expect(Kind::BraceL, true);
		let base = self.scratch.items.len();
		let outer_exports = std::mem::take(&mut self.scratch.checks.exports);
		let outer_ctx = self.ctx;
		while self.tok.kind != Kind::BraceR && !self.error {
			let at = self.tok.start;
			if self.recover {
				// each statement recovered from, as a program's are
				if let Some(statement) = self.statement_recovered(0) {
					if self.scratch.ts.state.ambient && !self.is_declaration(statement) {
						self.raise(at, Code::StatementInAmbient);
						break;
					}
					self.scratch.items.push(Some(statement));
				}
				if self.error || !self.ensure_progress(at) {
					break;
				}
				continue;
			}
			if self.tok.kind == Kind::Eof {
				self.fail();
				break;
			}
			let statement = self.statement(0);
			if self.error {
				break;
			}
			if self.scratch.ts.state.ambient && !self.is_declaration(statement) {
				self.raise(at, Code::StatementInAmbient);
				break;
			}
			self.scratch.items.push(Some(statement));
		}
		self.ctx = outer_ctx;
		self.scratch.checks.exports = outer_exports;
		if self.error {
			return NodeId::at(0);
		}
		self.tnext();
		self.scratch.ts.state.module_blocks -= 1;
		self.scratch.ts.state.in_namespace = in_namespace;
		self.exit_scope();
		let body = self.list(base);
		self.add(NodeKind::ModuleBlock { body }, start)
	}

	fn parse_ambient_external_module_declaration(&mut self, start: u32) -> NodeId {
		let global = self.is_word(b"global");
		let id = if global {
			self.ident()
		} else if self.tok.kind == Kind::String {
			if !self.scratch.ts.state.ambient {
				return self.raise(self.tok.start, Code::QuotedModuleName);
			}
			self.atom(&mut None, false)
		} else {
			return self.fail();
		};
		if self.error {
			return id;
		}
		let body = if self.tok.kind != Kind::BraceL {
			self.tsemicolon();
			None
		} else if global {
			Some(self.parse_global_body())
		} else {
			Some(self.parse_module_body(false))
		};
		self.add(NodeKind::ModuleDeclaration { id, body, global }, start)
	}

	fn parse_module_or_namespace_declaration(&mut self, start: u32, nested: bool) -> NodeId {
		if !self.enter() {
			return NodeId::at(0);
		}
		let result = self.parse_module_or_namespace_declaration_inner(start, nested);
		self.depth -= 1;
		result
	}

	fn parse_module_or_namespace_declaration_inner(&mut self, start: u32, nested: bool) -> NodeId {
		let id = self.ident_or_name(nested);
		if self.error {
			return id;
		}
		if !nested {
			self.check_lval_simple(id, Binding::None, false);
			if self.error {
				return id;
			}
			self.declare_export_only(id);
		} else if let NodeKind::Identifier { name } = self.kind_of(id)
			&& self.strict
			&& self.word_flags(name) & crate::parser::word::STRICT != 0
		{
			let at = self.start_of(id);
			return self.raise_name(at, Code::ReservedWord, name);
		}
		let body = if self.eat(Kind::Dot) {
			let inner_start = self.tok.start;
			self.parse_module_or_namespace_declaration(inner_start, true)
		} else {
			self.parse_module_body(true)
		};
		self.add(
			NodeKind::ModuleDeclaration {
				id,
				body: Some(body),
				global: false,
			},
			start,
		)
	}

	fn parse_type_alias_declaration(&mut self, start: u32) -> NodeId {
		let id = self.ident();
		self.check_lval_simple(id, Binding::None, false);
		if self.error {
			return id;
		}
		self.declare_type(id, true);
		if self.error {
			return id;
		}
		let old = std::mem::replace(&mut self.in_type, true);
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::InOut);
		self.expect(Kind::Eq, false);
		if self.error {
			return id;
		}
		let type_annotation = if self.is_word(b"intrinsic") && self.peek_tok().kind != Kind::Dot {
			if self.failed() {
				return self.fail_if_clean();
			}
			let keyword_start = self.tok.start;
			self.tnext();
			self.add(NodeKind::Keyword(TsKeyword::Intrinsic), keyword_start)
		} else {
			if self.failed() {
				return self.fail_if_clean();
			}
			self.parse_type()
		};
		self.in_type = old;
		self.tsemicolon();
		self.add(
			NodeKind::TypeAliasDeclaration {
				id,
				type_parameters,
				type_annotation,
			},
			start,
		)
	}

	fn parse_interface_declaration(&mut self, start: u32) -> NodeId {
		if !self.is_ident() {
			return self.raise(self.tok.start, Code::InterfaceWithoutName);
		}
		let id = self.ident();
		self.check_lval_simple(id, Binding::None, false);
		if self.error {
			return id;
		}
		self.declare_type(id, false);
		let type_parameters = self.try_parse_type_parameters(TypeParameterModifiers::InOut);
		if self.error {
			return id;
		}
		let extends = if self.eat(Kind::Extends) {
			let base = self.parse_heritage_clause("extends");
			self.list(base)
		} else {
			List::EMPTY
		};
		if self.error {
			return id;
		}
		let body_start = self.tok.start;
		self.expect(Kind::BraceL, false);
		let old = std::mem::replace(&mut self.in_type, true);
		let base = self.parse_list(ListKind::TypeMembers, |p| p.parse_type_member());
		self.in_type = old;
		self.expect(Kind::BraceR, false);
		let members = self.list(base);
		let body = self.add(NodeKind::InterfaceBody { body: members }, body_start);
		self.add(
			NodeKind::InterfaceDeclaration {
				id,
				type_parameters,
				extends,
				body,
			},
			start,
		)
	}

	fn check_ambient_initializer(&mut self, declarator: NodeId, kind: VariableKind) {
		let NodeKind::VariableDeclarator { id, init: Some(init) } = self.kind_of(declarator) else {
			return;
		};
		let annotated = self.ast.extras.get(id).is_some_and(|e| e.type_annotation.is_some());
		let at = self.start_of(init);
		if kind != VariableKind::Const || annotated {
			self.raise(at, Code::InitializerInAmbient);
			return;
		}
		let literal = match self.kind_of(init) {
			NodeKind::StringLiteral { .. }
			| NodeKind::NumberLiteral { .. }
			| NodeKind::BigIntLiteral
			| NodeKind::BooleanLiteral { .. } => true,
			NodeKind::UnaryExpression {
				operator: UnaryOperator::Minus,
				argument,
			} => matches!(
				self.kind_of(argument),
				NodeKind::NumberLiteral { .. } | NodeKind::BigIntLiteral
			),
			NodeKind::TemplateLiteral { expressions, .. } => expressions.len == 0,
			NodeKind::MemberExpression { .. } => self.is_possibly_literal_enum(init),
			_ => false,
		};
		if !literal {
			self.raise(at, Code::AmbientConstInitializer);
		}
	}

	fn is_possibly_literal_enum(&self, id: NodeId) -> bool {
		let NodeKind::MemberExpression {
			object,
			property,
			computed,
			..
		} = self.kind_of(id)
		else {
			return false;
		};
		if computed
			&& !matches!(
				self.kind_of(property),
				NodeKind::StringLiteral { .. }
					| NodeKind::NumberLiteral { .. }
					| NodeKind::TemplateLiteral {
						expressions: List { len: 0, .. },
						..
					}
			) {
			return false;
		}
		self.is_uncomputed_member_chain(object)
	}

	fn is_uncomputed_member_chain(&self, id: NodeId) -> bool {
		match self.kind_of(id) {
			NodeKind::Identifier { .. } => true,
			NodeKind::MemberExpression {
				object,
				computed: false,
				..
			} => self.is_uncomputed_member_chain(object),
			_ => false,
		}
	}

	fn head(&self) -> Option<Head> {
		if self.tok.kind != Kind::Ident || self.tok.flags & ESCAPED != 0 {
			return None;
		}
		Some(match self.tok_text() {
			"abstract" => Head::Abstract,
			"declare" => Head::Declare,
			"enum" => Head::Enum,
			"global" => Head::Global,
			"interface" => Head::Interface,
			"module" => Head::Module,
			"namespace" => Head::Namespace,
			"type" => Head::Type,
			_ => return None,
		})
	}

	fn open_declaration(&mut self) -> Option<Opened> {
		let snapshot = self.tok_snap();
		let mut opened = Opened::default();
		loop {
			let at = self.tok.start;
			let Some(head) = self.head() else {
				let javascript =
					matches!(self.tok.kind, Kind::Var | Kind::Const | Kind::Function | Kind::Class) || self.is_word(b"let");
				let modified = opened.declare.is_some() || opened.is_abstract;
				if javascript && (modified || self.tok.kind == Kind::Const && self.peek_is_contextual("enum")) {
					return Some(opened);
				}
				if self.failed() {
					return None;
				}
				break;
			};
			match head {
				Head::Enum => {
					opened.head = Some(head);
					return Some(opened);
				}
				Head::Global => {
					let next = self.peek_tok().kind;
					if self.failed() {
						return None;
					}
					let bodiless = opened.declare.is_some() && matches!(next, Kind::Ident | Kind::Export);
					if next != Kind::BraceL && !bodiless {
						break;
					}
					opened.head = Some(head);
					return Some(opened);
				}
				_ => {}
			}
			self.tnext();
			if self.failed() {
				return None;
			}
			if self.newline() {
				break;
			}
			let named = match head {
				Head::Declare if opened.declare.is_none() => {
					opened.declare = Some(at);
					continue;
				}
				Head::Abstract if !opened.is_abstract => {
					opened.is_abstract = true;
					continue;
				}
				Head::Interface => true,
				Head::Type => self.is_ident(),
				Head::Namespace | Head::Module => self.is_ident() || self.tok.kind == Kind::String,
				_ => false,
			};
			if !named {
				break;
			}
			opened.head = Some(head);
			return Some(opened);
		}
		self.restore_tokens(snapshot);
		None
	}

	fn parse_opened(&mut self, start: u32, opened: Opened) -> NodeId {
		let Some(at) = opened.declare else {
			return self.parse_opened_declaration(start, opened);
		};
		if self.scratch.ts.state.ambient {
			return self.raise(at, Code::DeclareInAmbient);
		}
		let old = std::mem::replace(&mut self.scratch.ts.state.ambient, true);
		let node = self.parse_opened_declaration(start, opened);
		self.scratch.ts.state.ambient = old;
		if !self.error {
			self.extras_mut(node).declare = true;
		}
		node
	}

	fn parse_opened_declaration(&mut self, start: u32, opened: Opened) -> NodeId {
		if opened.is_abstract || opened.head.is_none() && self.tok.kind == Kind::Class {
			if opened.head.is_some() || self.tok.kind != Kind::Class {
				return self.raise(start, Code::AbstractPlacement);
			}
			self.scratch.ts.state.next_class = ClassFrame {
				is_abstract: opened.is_abstract,
				declare: opened.declare.is_some(),
				start: Some(start),
				..ClassFrame::default()
			};
			return self.class(true, false);
		}
		let Some(head) = opened.head else {
			let const_enum = self.tok.kind == Kind::Const && self.peek_is_contextual("enum");
			if self.failed() {
				return self.fail_if_clean();
			}
			return match self.tok.kind {
				Kind::Function => {
					self.tnext();
					self.function(start, false, true, false, false)
				}
				Kind::Const if const_enum => {
					self.tnext();
					self.parse_enum_declaration(start, true)
				}
				Kind::Const => self.var_statement(start, VariableKind::Const),
				Kind::Var => self.var_statement(start, VariableKind::Var),
				_ => self.var_statement(start, VariableKind::Let),
			};
		};
		match head {
			Head::Enum => self.parse_enum_declaration(start, false),
			Head::Global => self.parse_ambient_external_module_declaration(start),
			Head::Interface => self.parse_interface_declaration(start),
			Head::Type => self.parse_type_alias_declaration(start),
			Head::Module if self.tok.kind == Kind::String => self.parse_ambient_external_module_declaration(start),
			_ => self.parse_module_or_namespace_declaration(start, false),
		}
	}

	fn parse_import_equals_declaration(&mut self, start: u32, is_export: bool) -> NodeId {
		let id = self.ident();
		self.check_lval_simple(id, Binding::Lexical, false);
		self.expect(Kind::Eq, false);
		if self.error {
			return id;
		}
		let import_kind = self.scratch.ts.state.outer_kind.unwrap_or(ImportKind::Value);
		let module_reference = if self.is_word(b"require") && self.lexer.peek_byte() == Some(b'(') {
			let reference_start = self.tok.start;
			self.tnext();
			self.expect(Kind::ParenL, false);
			if self.error {
				return id;
			}
			if self.tok.kind != Kind::String {
				return self.fail();
			}
			let expression = self.atom(&mut None, false);
			self.expect(Kind::ParenR, false);
			self.add(NodeKind::ExternalModuleReference { expression }, reference_start)
		} else {
			let reference = self.parse_entity_name(false);
			if self.error {
				return reference;
			}
			if import_kind == ImportKind::Type {
				let at = self.start_of(reference);
				return self.raise(at, Code::ImportTypeAlias);
			}
			reference
		};
		self.tsemicolon();
		self.scratch.ts.state.outer_kind = None;
		self.add(
			NodeKind::ImportEqualsDeclaration {
				id,
				module_reference,
				is_export,
				import_kind,
			},
			start,
		)
	}

	fn parse_type_only_specifier(&mut self, is_import: bool) -> NodeId {
		let start = self.tok.start;
		let in_type_only = self.scratch.ts.state.outer_kind == Some(ImportKind::Type);
		let mut left = self.module_name();
		let mut right = None;
		let mut has_type_specifier = false;
		let mut can_parse_as = true;
		if self.is_word(b"as") {
			let first_as = self.ident();
			if self.is_word(b"as") {
				let second_as = self.ident();
				if self.is_ident_or_keyword() {
					has_type_specifier = true;
					left = first_as;
					right = Some(self.parse_specifier_name(is_import));
				} else {
					right = Some(second_as);
				}
				can_parse_as = false;
			} else if self.is_ident_or_keyword() {
				can_parse_as = false;
				right = Some(self.parse_specifier_name(is_import));
			} else {
				has_type_specifier = true;
				left = first_as;
			}
		} else if self.is_ident_or_keyword() {
			has_type_specifier = true;
			left = if is_import {
				let local = self.name();
				if !self.is_word(b"as") {
					self.check_unreserved(local);
				}
				local
			} else {
				self.module_name()
			};
		}
		if self.error {
			return left;
		}
		if has_type_specifier && in_type_only {
			return self.raise(start, Code::TypeModifierInTypeImport);
		}
		if can_parse_as && self.eat_word(b"as") {
			right = Some(self.parse_specifier_name(is_import));
		}
		if self.error {
			return left;
		}
		let right = match right {
			Some(right) => right,
			None => {
				let node = *self.ast.node(left);
				self.ast.add(node.kind, node.start, node.end)
			}
		};
		let kind = if has_type_specifier {
			ImportKind::Type
		} else {
			ImportKind::Value
		};
		if is_import {
			self.check_lval_simple(
				right,
				if in_type_only || has_type_specifier {
					Binding::None
				} else {
					Binding::Lexical
				},
				false,
			);
			if self.error {
				return left;
			}
			if has_type_specifier {
				self.declare_export_only(right);
			}
		}
		if is_import {
			let node = self.add(
				NodeKind::ImportSpecifier {
					imported: left,
					local: right,
				},
				start,
			);
			self.extras_mut(node).import_kind = Some(kind);
			node
		} else {
			let node = self.add(
				NodeKind::ExportSpecifier {
					local: left,
					exported: right,
				},
				start,
			);
			self.extras_mut(node).export_kind = Some(kind);
			node
		}
	}

	fn parse_specifier_name(&mut self, is_import: bool) -> NodeId {
		if is_import { self.ident() } else { self.module_name() }
	}
}
