use super::*;

impl Walker<'_> {
	pub(super) fn native_property(&mut self, node: NodeId, field_name: Key) -> Datum {
		match self.tree().node(node).kind {
			NodeKind::Program { body, module } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeProgram.id()),
				Key::Body => Datum::Nodes(body),
				Key::SourceType => Datum::Text(if module {
					Symbol::WordModule.id()
				} else {
					Symbol::WordScript.id()
				}),
				_ => Datum::Missing,
			},
			NodeKind::Identifier { name } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeIdentifier.id()),
				Key::Name => Datum::Interned(name),
				_ => Datum::Missing,
			},
			NodeKind::PrivateIdentifier { name } => match field_name {
				Key::Type => Datum::Text(Symbol::TypePrivateIdentifier.id()),
				Key::Name => Datum::Interned(name),
				_ => Datum::Missing,
			},
			NodeKind::NumberLiteral { value } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLiteral.id()),
				Key::Value => Datum::Number(self.tree().numbers[value as usize]),
				_ => Datum::Missing,
			},
			NodeKind::BigIntLiteral => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLiteral.id()),
				Key::Value => Datum::Null,
				Key::Bigint => {
					let n = self.tree().node(node);
					let text = crate::estree::bigint_decimal(&self.src[n.start as usize..n.end as usize - 1]);
					Datum::Interned(self.intern(&text))
				}
				_ => Datum::Missing,
			},
			NodeKind::StringLiteral { value } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLiteral.id()),
				Key::Value => Datum::Interned(value),
				_ => Datum::Missing,
			},
			NodeKind::BooleanLiteral { value } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLiteral.id()),
				Key::Value => Datum::Bool(value),
				_ => Datum::Missing,
			},
			NodeKind::NullLiteral => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLiteral.id()),
				Key::Value => Datum::Null,
				_ => Datum::Missing,
			},
			NodeKind::RegExpLiteral { pattern, flags } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLiteral.id()),
				Key::Value => Datum::Null,
				Key::Regex => Datum::Regex(pattern, flags),
				Key::Pattern => Datum::Interned(pattern),
				Key::Flags => Datum::Interned(flags),
				_ => Datum::Missing,
			},
			NodeKind::TemplateLiteral { quasis, expressions } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeTemplateLiteral.id()),
				Key::Quasis => Datum::Nodes(quasis),
				Key::Expressions => Datum::Nodes(expressions),
				_ => Datum::Missing,
			},
			NodeKind::TemplateElement { cooked, raw, tail } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeTemplateElement.id()),
				Key::Value => Datum::Template(raw, cooked),
				Key::Cooked => cooked.map_or(Datum::Null, Datum::Interned),
				Key::Raw => Datum::Interned(raw),
				Key::Tail => Datum::Bool(tail),
				_ => Datum::Missing,
			},
			NodeKind::TaggedTemplateExpression { tag, quasi } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeTaggedTemplateExpression.id()),
				Key::Tag => Datum::Node(tag),
				Key::Quasi => Datum::Node(quasi),
				_ => Datum::Missing,
			},
			NodeKind::ThisExpression => match field_name {
				Key::Type => Datum::Text(Symbol::TypeThisExpression.id()),

				_ => Datum::Missing,
			},
			NodeKind::Super => match field_name {
				Key::Type => Datum::Text(Symbol::TypeSuper.id()),

				_ => Datum::Missing,
			},
			NodeKind::ArrayExpression { elements } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeArrayExpression.id()),
				Key::Elements => Datum::Nodes(elements),
				_ => Datum::Missing,
			},
			NodeKind::ObjectExpression { properties } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeObjectExpression.id()),
				Key::Properties => Datum::Nodes(properties),
				_ => Datum::Missing,
			},
			NodeKind::Property {
				key,
				value,
				kind,
				computed,
				method,
				shorthand,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeProperty.id()),
				Key::Key => Datum::Node(key),
				Key::Value => Datum::Node(value),
				Key::Kind => Datum::Text(match kind {
					crate::ast::PropertyKind::Init => Symbol::WordInit.id(),
					crate::ast::PropertyKind::Get => Symbol::WordGet.id(),
					crate::ast::PropertyKind::Set => Symbol::WordSet.id(),
				}),
				Key::Computed => Datum::Bool(computed),
				Key::Method => Datum::Bool(method),
				Key::Shorthand => Datum::Bool(shorthand),
				_ => Datum::Missing,
			},
			NodeKind::SpreadElement { argument } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeSpreadElement.id()),
				Key::Argument => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::UnaryExpression { operator, argument } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeUnaryExpression.id()),
				Key::Operator => Datum::Interned(self.intern(operator.name().text)),
				Key::Argument => Datum::Node(argument),
				Key::Prefix => Datum::Bool(true),
				_ => Datum::Missing,
			},
			NodeKind::UpdateExpression {
				operator,
				prefix,
				argument,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeUpdateExpression.id()),
				Key::Operator => Datum::Interned(self.intern(operator.name().text)),
				Key::Prefix => Datum::Bool(prefix),
				Key::Argument => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::BinaryExpression { operator, left, right } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeBinaryExpression.id()),
				Key::Operator => Datum::Interned(self.intern(operator.name().text)),
				Key::Left => Datum::Node(left),
				Key::Right => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::LogicalExpression { operator, left, right } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLogicalExpression.id()),
				Key::Operator => Datum::Interned(self.intern(operator.name().text)),
				Key::Left => Datum::Node(left),
				Key::Right => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::AssignmentExpression { operator, left, right } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeAssignmentExpression.id()),
				Key::Operator => Datum::Interned(self.intern(operator.name().text)),
				Key::Left => Datum::Node(left),
				Key::Right => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::ConditionalExpression {
				test,
				consequent,
				alternate,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeConditionalExpression.id()),
				Key::Test => Datum::Node(test),
				Key::Consequent => Datum::Node(consequent),
				Key::Alternate => Datum::Node(alternate),
				_ => Datum::Missing,
			},
			NodeKind::MemberExpression {
				object,
				property,
				computed,
				optional,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeMemberExpression.id()),
				Key::Object => Datum::Node(object),
				Key::Property => Datum::Node(property),
				Key::Computed => Datum::Bool(computed),
				Key::Optional => Datum::Bool(optional),
				_ => Datum::Missing,
			},
			NodeKind::CallExpression {
				callee,
				arguments,
				optional,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeCallExpression.id()),
				Key::Callee => Datum::Node(callee),
				Key::Arguments => Datum::Nodes(arguments),
				Key::Optional => Datum::Bool(optional),
				_ => Datum::Missing,
			},
			NodeKind::ChainExpression { expression } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeChainExpression.id()),
				Key::Expression => Datum::Node(expression),
				_ => Datum::Missing,
			},
			NodeKind::NewExpression { callee, arguments } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeNewExpression.id()),
				Key::Callee => Datum::Node(callee),
				Key::Arguments => Datum::Nodes(arguments),
				_ => Datum::Missing,
			},
			NodeKind::SequenceExpression { expressions } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeSequenceExpression.id()),
				Key::Expressions => Datum::Nodes(expressions),
				_ => Datum::Missing,
			},
			NodeKind::ArrowFunctionExpression {
				params,
				body,
				expression,
				is_async,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeArrowFunctionExpression.id()),
				Key::Params => Datum::Nodes(params),
				Key::Body => Datum::Node(body),
				Key::Expression => Datum::Bool(expression),
				Key::Async => Datum::Bool(is_async),
				_ => Datum::Missing,
			},
			NodeKind::FunctionExpression { function } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeFunctionExpression.id()),
				Key::Id => function.id.map_or(Datum::Null, Datum::Node),
				Key::Body => Datum::Node(function.body),
				Key::Params => Datum::Nodes(function.params),
				Key::Async => Datum::Bool(function.is_async),
				Key::Generator => Datum::Bool(function.generator),
				Key::Expression => Datum::Bool(false),
				_ => Datum::Missing,
			},
			NodeKind::FunctionDeclaration { function } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeFunctionDeclaration.id()),
				Key::Id => function.id.map_or(Datum::Null, Datum::Node),
				Key::Body => Datum::Node(function.body),
				Key::Params => Datum::Nodes(function.params),
				Key::Async => Datum::Bool(function.is_async),
				Key::Generator => Datum::Bool(function.generator),
				Key::Expression => Datum::Bool(false),
				_ => Datum::Missing,
			},
			NodeKind::ClassExpression { class } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeClassExpression.id()),
				Key::Id => class.id.map_or(Datum::Null, Datum::Node),
				Key::Body => Datum::Node(class.body),
				Key::SuperClass => class.super_class.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ClassDeclaration { class } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeClassDeclaration.id()),
				Key::Id => class.id.map_or(Datum::Null, Datum::Node),
				Key::Body => Datum::Node(class.body),
				Key::SuperClass => class.super_class.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ClassBody { body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeClassBody.id()),
				Key::Body => Datum::Nodes(body),
				_ => Datum::Missing,
			},
			NodeKind::MethodDefinition {
				key,
				value,
				kind,
				computed,
				is_static,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeMethodDefinition.id()),
				Key::Key => Datum::Node(key),
				Key::Value => Datum::Node(value),
				Key::Kind => Datum::Text(match kind {
					crate::ast::MethodKind::Constructor => Symbol::WordConstructor.id(),
					crate::ast::MethodKind::Method => Symbol::WordMethod.id(),
					crate::ast::MethodKind::Get => Symbol::WordGet.id(),
					crate::ast::MethodKind::Set => Symbol::WordSet.id(),
				}),
				Key::Computed => Datum::Bool(computed),
				Key::Static => Datum::Bool(is_static),
				_ => Datum::Missing,
			},
			NodeKind::PropertyDefinition {
				key,
				value,
				computed,
				is_static,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypePropertyDefinition.id()),
				Key::Key => Datum::Node(key),
				Key::Value => value.map_or(Datum::Null, Datum::Node),
				Key::Computed => Datum::Bool(computed),
				Key::Static => Datum::Bool(is_static),
				_ => Datum::Missing,
			},
			NodeKind::StaticBlock { body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeStaticBlock.id()),
				Key::Body => Datum::Nodes(body),
				_ => Datum::Missing,
			},
			NodeKind::YieldExpression { argument, delegate } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeYieldExpression.id()),
				Key::Argument => argument.map_or(Datum::Null, Datum::Node),
				Key::Delegate => Datum::Bool(delegate),
				_ => Datum::Missing,
			},
			NodeKind::AwaitExpression { argument } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeAwaitExpression.id()),
				Key::Argument => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::MetaProperty { meta, property } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeMetaProperty.id()),
				Key::Meta => Datum::Node(meta),
				Key::Property => Datum::Node(property),
				_ => Datum::Missing,
			},
			NodeKind::ImportExpression { source, options } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeImportExpression.id()),
				Key::Source => Datum::Node(source),
				Key::Options => options.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ObjectPattern { properties } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeObjectPattern.id()),
				Key::Properties => Datum::Nodes(properties),
				_ => Datum::Missing,
			},
			NodeKind::ArrayPattern { elements } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeArrayPattern.id()),
				Key::Elements => Datum::Nodes(elements),
				_ => Datum::Missing,
			},
			NodeKind::RestElement { argument } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeRestElement.id()),
				Key::Argument => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::AssignmentPattern { left, right } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeAssignmentPattern.id()),
				Key::Left => Datum::Node(left),
				Key::Right => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::ExpressionStatement { expression, directive } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeExpressionStatement.id()),
				Key::Expression => Datum::Node(expression),
				Key::Directive => directive.map_or(Datum::Null, Datum::Interned),
				_ => Datum::Missing,
			},
			NodeKind::BlockStatement { body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeBlockStatement.id()),
				Key::Body => Datum::Nodes(body),
				_ => Datum::Missing,
			},
			NodeKind::EmptyStatement => match field_name {
				Key::Type => Datum::Text(Symbol::TypeEmptyStatement.id()),

				_ => Datum::Missing,
			},
			NodeKind::DebuggerStatement => match field_name {
				Key::Type => Datum::Text(Symbol::TypeDebuggerStatement.id()),

				_ => Datum::Missing,
			},
			NodeKind::WithStatement { object, body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeWithStatement.id()),
				Key::Object => Datum::Node(object),
				Key::Body => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::ReturnStatement { argument } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeReturnStatement.id()),
				Key::Argument => argument.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::LabeledStatement { label, body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeLabeledStatement.id()),
				Key::Label => Datum::Node(label),
				Key::Body => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::BreakStatement { label } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeBreakStatement.id()),
				Key::Label => label.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ContinueStatement { label } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeContinueStatement.id()),
				Key::Label => label.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::IfStatement {
				test,
				consequent,
				alternate,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeIfStatement.id()),
				Key::Test => Datum::Node(test),
				Key::Consequent => Datum::Node(consequent),
				Key::Alternate => alternate.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::SwitchStatement { discriminant, cases } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeSwitchStatement.id()),
				Key::Discriminant => Datum::Node(discriminant),
				Key::Cases => Datum::Nodes(cases),
				_ => Datum::Missing,
			},
			NodeKind::SwitchCase { test, consequent } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeSwitchCase.id()),
				Key::Test => test.map_or(Datum::Null, Datum::Node),
				Key::Consequent => Datum::Nodes(consequent),
				_ => Datum::Missing,
			},
			NodeKind::ThrowStatement { argument } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeThrowStatement.id()),
				Key::Argument => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::TryStatement {
				block,
				handler,
				finalizer,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeTryStatement.id()),
				Key::Block => Datum::Node(block),
				Key::Handler => handler.map_or(Datum::Null, Datum::Node),
				Key::Finalizer => finalizer.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::CatchClause { param, body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeCatchClause.id()),
				Key::Param => param.map_or(Datum::Null, Datum::Node),
				Key::Body => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::WhileStatement { test, body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeWhileStatement.id()),
				Key::Test => Datum::Node(test),
				Key::Body => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::DoWhileStatement { body, test } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeDoWhileStatement.id()),
				Key::Body => Datum::Node(body),
				Key::Test => Datum::Node(test),
				_ => Datum::Missing,
			},
			NodeKind::ForStatement {
				init,
				test,
				update,
				body,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeForStatement.id()),
				Key::Init => init.map_or(Datum::Null, Datum::Node),
				Key::Test => test.map_or(Datum::Null, Datum::Node),
				Key::Update => update.map_or(Datum::Null, Datum::Node),
				Key::Body => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::ForInStatement { left, right, body } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeForInStatement.id()),
				Key::Left => Datum::Node(left),
				Key::Right => Datum::Node(right),
				Key::Body => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::ForOfStatement {
				left,
				right,
				body,
				is_await,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeForOfStatement.id()),
				Key::Left => Datum::Node(left),
				Key::Right => Datum::Node(right),
				Key::Body => Datum::Node(body),
				Key::IsAwait => Datum::Bool(is_await),
				_ => Datum::Missing,
			},
			NodeKind::VariableDeclaration { declarations, kind } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeVariableDeclaration.id()),
				Key::Declarations => Datum::Nodes(declarations),
				Key::Kind => Datum::Interned(self.intern(kind.name().text)),
				_ => Datum::Missing,
			},
			NodeKind::VariableDeclarator { id, init } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeVariableDeclarator.id()),
				Key::Id => Datum::Node(id),
				Key::Init => init.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ImportDeclaration {
				specifiers,
				source,
				attributes,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeImportDeclaration.id()),
				Key::Specifiers => Datum::Nodes(specifiers),
				Key::Source => Datum::Node(source),
				Key::Attributes => Datum::Nodes(attributes),
				_ => Datum::Missing,
			},
			NodeKind::ImportSpecifier { imported, local } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeImportSpecifier.id()),
				Key::Imported => Datum::Node(imported),
				Key::Local => Datum::Node(local),
				_ => Datum::Missing,
			},
			NodeKind::ImportDefaultSpecifier { local } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeImportDefaultSpecifier.id()),
				Key::Local => Datum::Node(local),
				_ => Datum::Missing,
			},
			NodeKind::ImportNamespaceSpecifier { local } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeImportNamespaceSpecifier.id()),
				Key::Local => Datum::Node(local),
				_ => Datum::Missing,
			},
			NodeKind::ImportAttribute { key, value } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeImportAttribute.id()),
				Key::Key => Datum::Node(key),
				Key::Value => Datum::Node(value),
				_ => Datum::Missing,
			},
			NodeKind::ExportDeclaration { declaration } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeExportNamedDeclaration.id()),
				Key::Declaration => Datum::Node(declaration),
				_ => Datum::Missing,
			},
			NodeKind::ExportNamedDeclaration {
				specifiers,
				source,
				attributes,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeExportNamedDeclaration.id()),
				Key::Specifiers => Datum::Nodes(specifiers),
				Key::Source => source.map_or(Datum::Null, Datum::Node),
				Key::Attributes => Datum::Nodes(attributes),
				_ => Datum::Missing,
			},
			NodeKind::ExportSpecifier { local, exported } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeExportSpecifier.id()),
				Key::Local => Datum::Node(local),
				Key::Exported => Datum::Node(exported),
				_ => Datum::Missing,
			},
			NodeKind::ExportDefaultDeclaration { declaration } => match field_name {
				Key::Type => Datum::Text(Symbol::TypeExportDefaultDeclaration.id()),
				Key::Declaration => Datum::Node(declaration),
				_ => Datum::Missing,
			},
			NodeKind::ExportAllDeclaration {
				exported,
				source,
				attributes,
			} => match field_name {
				Key::Type => Datum::Text(Symbol::TypeExportAllDeclaration.id()),
				Key::Exported => exported.map_or(Datum::Null, Datum::Node),
				Key::Source => Datum::Node(source),
				Key::Attributes => Datum::Nodes(attributes),
				_ => Datum::Missing,
			},
			_ => Datum::Missing,
		}
	}

	pub(super) fn native_construct(
		&mut self,
		ty: &str,
		start: u32,
		end: u32,
		fields: &[(&str, Datum)],
	) -> Result<NodeKind> {
		let get = |name: &str| {
			fields
				.iter()
				.find(|(key, _)| *key == name)
				.map_or(&Datum::Missing, |(_, value)| value)
		};
		let optional = |name: &str| {
			if let Datum::Node(id) = get(name) {
				Some(*id)
			} else {
				None
			}
		};
		let node = |name: &str| optional(name).ok_or_else(|| error(start, end, Code::Expected, Some(name)));
		let string = |this: &mut Self, name: &str| -> Result<StrId> {
			this.text_of(get(name))
				.ok_or_else(|| error(start, end, Code::Expected, Some(name)))?;
			Ok(this.string(*get(name)))
		};
		let list = |this: &mut Self, name: &str| {
			let value = get(name);
			if matches!(value, Datum::Missing | Datum::Null) {
				return Ok(List::EMPTY);
			}
			let Value::Nodes(list) = this.output(value)? else {
				return fail(start, end, Code::Expected, Some(name));
			};
			Ok(list)
		};
		Ok(match ty {
			"js.Identifier" => NodeKind::Identifier {
				name: string(self, "name")?,
			},
			"js.PrivateIdentifier" => NodeKind::PrivateIdentifier {
				name: string(self, "name")?,
			},
			"js.TemplateLiteral" => NodeKind::TemplateLiteral {
				quasis: list(self, "quasis")?,
				expressions: list(self, "expressions")?,
			},
			"js.TaggedTemplateExpression" => NodeKind::TaggedTemplateExpression {
				tag: node("tag")?,
				quasi: node("quasi")?,
			},
			"js.ThisExpression" => NodeKind::ThisExpression,
			"js.Super" => NodeKind::Super,
			"js.ArrayExpression" => NodeKind::ArrayExpression {
				elements: list(self, "elements")?,
			},
			"js.ObjectExpression" => NodeKind::ObjectExpression {
				properties: list(self, "properties")?,
			},
			"js.Property" => NodeKind::Property {
				key: node("key")?,
				value: node("value")?,
				kind: match self.text_of(get("kind")).unwrap_or("") {
					"init" => crate::ast::PropertyKind::Init,
					"get" => crate::ast::PropertyKind::Get,
					"set" => crate::ast::PropertyKind::Set,
					_ => return fail(start, end, Code::Expected, Some("kind")),
				},
				computed: get("computed").yes(),
				method: get("method").yes(),
				shorthand: get("shorthand").yes(),
			},
			"js.SpreadElement" => NodeKind::SpreadElement {
				argument: node("argument")?,
			},
			"js.UnaryExpression" => NodeKind::UnaryExpression {
				operator: match self.text_of(get("operator")).unwrap_or("") {
					"-" => crate::ast::UnaryOperator::Minus,
					"+" => crate::ast::UnaryOperator::Plus,
					"!" => crate::ast::UnaryOperator::Not,
					"~" => crate::ast::UnaryOperator::BitNot,
					"typeof" => crate::ast::UnaryOperator::Typeof,
					"void" => crate::ast::UnaryOperator::Void,
					"delete" => crate::ast::UnaryOperator::Delete,
					_ => return fail(start, end, Code::Expected, Some("operator")),
				},
				argument: node("argument")?,
			},
			"js.UpdateExpression" => NodeKind::UpdateExpression {
				operator: match self.text_of(get("operator")).unwrap_or("") {
					"++" => crate::ast::UpdateOperator::Increment,
					"--" => crate::ast::UpdateOperator::Decrement,
					_ => return fail(start, end, Code::Expected, Some("operator")),
				},
				prefix: get("prefix").yes(),
				argument: node("argument")?,
			},
			"js.BinaryExpression" => NodeKind::BinaryExpression {
				operator: match self.text_of(get("operator")).unwrap_or("") {
					"==" => crate::ast::BinaryOperator::Eq,
					"!=" => crate::ast::BinaryOperator::NotEq,
					"===" => crate::ast::BinaryOperator::StrictEq,
					"!==" => crate::ast::BinaryOperator::StrictNotEq,
					"<" => crate::ast::BinaryOperator::Lt,
					"<=" => crate::ast::BinaryOperator::LtEq,
					">" => crate::ast::BinaryOperator::Gt,
					">=" => crate::ast::BinaryOperator::GtEq,
					"<<" => crate::ast::BinaryOperator::Shl,
					">>" => crate::ast::BinaryOperator::Shr,
					">>>" => crate::ast::BinaryOperator::UShr,
					"+" => crate::ast::BinaryOperator::Add,
					"-" => crate::ast::BinaryOperator::Sub,
					"*" => crate::ast::BinaryOperator::Mul,
					"/" => crate::ast::BinaryOperator::Div,
					"%" => crate::ast::BinaryOperator::Mod,
					"**" => crate::ast::BinaryOperator::Exp,
					"|" => crate::ast::BinaryOperator::BitOr,
					"^" => crate::ast::BinaryOperator::BitXor,
					"&" => crate::ast::BinaryOperator::BitAnd,
					"in" => crate::ast::BinaryOperator::In,
					"instanceof" => crate::ast::BinaryOperator::Instanceof,
					_ => return fail(start, end, Code::Expected, Some("operator")),
				},
				left: node("left")?,
				right: node("right")?,
			},
			"js.LogicalExpression" => NodeKind::LogicalExpression {
				operator: match self.text_of(get("operator")).unwrap_or("") {
					"||" => crate::ast::LogicalOperator::Or,
					"&&" => crate::ast::LogicalOperator::And,
					"??" => crate::ast::LogicalOperator::Nullish,
					_ => return fail(start, end, Code::Expected, Some("operator")),
				},
				left: node("left")?,
				right: node("right")?,
			},
			"js.AssignmentExpression" => NodeKind::AssignmentExpression {
				operator: match self.text_of(get("operator")).unwrap_or("") {
					"=" => crate::ast::AssignmentOperator::Assign,
					"+=" => crate::ast::AssignmentOperator::Add,
					"-=" => crate::ast::AssignmentOperator::Sub,
					"*=" => crate::ast::AssignmentOperator::Mul,
					"/=" => crate::ast::AssignmentOperator::Div,
					"%=" => crate::ast::AssignmentOperator::Mod,
					"**=" => crate::ast::AssignmentOperator::Exp,
					"<<=" => crate::ast::AssignmentOperator::Shl,
					">>=" => crate::ast::AssignmentOperator::Shr,
					">>>=" => crate::ast::AssignmentOperator::UShr,
					"|=" => crate::ast::AssignmentOperator::BitOr,
					"^=" => crate::ast::AssignmentOperator::BitXor,
					"&=" => crate::ast::AssignmentOperator::BitAnd,
					"||=" => crate::ast::AssignmentOperator::Or,
					"&&=" => crate::ast::AssignmentOperator::And,
					"??=" => crate::ast::AssignmentOperator::Nullish,
					_ => return fail(start, end, Code::Expected, Some("operator")),
				},
				left: node("left")?,
				right: node("right")?,
			},
			"js.ConditionalExpression" => NodeKind::ConditionalExpression {
				test: node("test")?,
				consequent: node("consequent")?,
				alternate: node("alternate")?,
			},
			"js.MemberExpression" => NodeKind::MemberExpression {
				object: node("object")?,
				property: node("property")?,
				computed: get("computed").yes(),
				optional: get("optional").yes(),
			},
			"js.CallExpression" => NodeKind::CallExpression {
				callee: node("callee")?,
				arguments: list(self, "arguments")?,
				optional: get("optional").yes(),
			},
			"js.ChainExpression" => NodeKind::ChainExpression {
				expression: node("expression")?,
			},
			"js.NewExpression" => NodeKind::NewExpression {
				callee: node("callee")?,
				arguments: list(self, "arguments")?,
			},
			"js.SequenceExpression" => NodeKind::SequenceExpression {
				expressions: list(self, "expressions")?,
			},
			"js.ArrowFunctionExpression" => NodeKind::ArrowFunctionExpression {
				params: list(self, "params")?,
				body: node("body")?,
				expression: get("expression").yes(),
				is_async: get("async").yes(),
			},
			"js.ClassBody" => NodeKind::ClassBody {
				body: list(self, "body")?,
			},
			"js.MethodDefinition" => NodeKind::MethodDefinition {
				key: node("key")?,
				value: node("value")?,
				kind: match self.text_of(get("kind")).unwrap_or("") {
					"constructor" => crate::ast::MethodKind::Constructor,
					"method" => crate::ast::MethodKind::Method,
					"get" => crate::ast::MethodKind::Get,
					"set" => crate::ast::MethodKind::Set,
					_ => return fail(start, end, Code::Expected, Some("kind")),
				},
				computed: get("computed").yes(),
				is_static: get("static").yes(),
			},
			"js.PropertyDefinition" => NodeKind::PropertyDefinition {
				key: node("key")?,
				value: optional("value"),
				computed: get("computed").yes(),
				is_static: get("static").yes(),
			},
			"js.StaticBlock" => NodeKind::StaticBlock {
				body: list(self, "body")?,
			},
			"js.YieldExpression" => NodeKind::YieldExpression {
				argument: optional("argument"),
				delegate: get("delegate").yes(),
			},
			"js.AwaitExpression" => NodeKind::AwaitExpression {
				argument: node("argument")?,
			},
			"js.MetaProperty" => NodeKind::MetaProperty {
				meta: node("meta")?,
				property: node("property")?,
			},
			"js.ImportExpression" => NodeKind::ImportExpression {
				source: node("source")?,
				options: optional("options"),
			},
			"js.ObjectPattern" => NodeKind::ObjectPattern {
				properties: list(self, "properties")?,
			},
			"js.ArrayPattern" => NodeKind::ArrayPattern {
				elements: list(self, "elements")?,
			},
			"js.RestElement" => NodeKind::RestElement {
				argument: node("argument")?,
			},
			"js.AssignmentPattern" => NodeKind::AssignmentPattern {
				left: node("left")?,
				right: node("right")?,
			},
			"js.ExpressionStatement" => NodeKind::ExpressionStatement {
				expression: node("expression")?,
				directive: if matches!(get("directive"), Datum::Missing | Datum::Null) {
					None
				} else {
					Some(string(self, "directive")?)
				},
			},
			"js.BlockStatement" => NodeKind::BlockStatement {
				body: list(self, "body")?,
			},
			"js.EmptyStatement" => NodeKind::EmptyStatement,
			"js.DebuggerStatement" => NodeKind::DebuggerStatement,
			"js.WithStatement" => NodeKind::WithStatement {
				object: node("object")?,
				body: node("body")?,
			},
			"js.ReturnStatement" => NodeKind::ReturnStatement {
				argument: optional("argument"),
			},
			"js.LabeledStatement" => NodeKind::LabeledStatement {
				label: node("label")?,
				body: node("body")?,
			},
			"js.BreakStatement" => NodeKind::BreakStatement {
				label: optional("label"),
			},
			"js.ContinueStatement" => NodeKind::ContinueStatement {
				label: optional("label"),
			},
			"js.IfStatement" => NodeKind::IfStatement {
				test: node("test")?,
				consequent: node("consequent")?,
				alternate: optional("alternate"),
			},
			"js.SwitchStatement" => NodeKind::SwitchStatement {
				discriminant: node("discriminant")?,
				cases: list(self, "cases")?,
			},
			"js.SwitchCase" => NodeKind::SwitchCase {
				test: optional("test"),
				consequent: list(self, "consequent")?,
			},
			"js.ThrowStatement" => NodeKind::ThrowStatement {
				argument: node("argument")?,
			},
			"js.TryStatement" => NodeKind::TryStatement {
				block: node("block")?,
				handler: optional("handler"),
				finalizer: optional("finalizer"),
			},
			"js.CatchClause" => NodeKind::CatchClause {
				param: optional("param"),
				body: node("body")?,
			},
			"js.WhileStatement" => NodeKind::WhileStatement {
				test: node("test")?,
				body: node("body")?,
			},
			"js.DoWhileStatement" => NodeKind::DoWhileStatement {
				body: node("body")?,
				test: node("test")?,
			},
			"js.ForStatement" => NodeKind::ForStatement {
				init: optional("init"),
				test: optional("test"),
				update: optional("update"),
				body: node("body")?,
			},
			"js.ForInStatement" => NodeKind::ForInStatement {
				left: node("left")?,
				right: node("right")?,
				body: node("body")?,
			},
			"js.ForOfStatement" => NodeKind::ForOfStatement {
				left: node("left")?,
				right: node("right")?,
				body: node("body")?,
				is_await: get("await").yes(),
			},
			"js.VariableDeclaration" => NodeKind::VariableDeclaration {
				declarations: list(self, "declarations")?,
				kind: match self.text_of(get("kind")).unwrap_or("") {
					"var" => crate::ast::VariableKind::Var,
					"let" => crate::ast::VariableKind::Let,
					"const" => crate::ast::VariableKind::Const,
					"using" => crate::ast::VariableKind::Using,
					"await using" => crate::ast::VariableKind::AwaitUsing,
					_ => return fail(start, end, Code::Expected, Some("kind")),
				},
			},
			"js.VariableDeclarator" => NodeKind::VariableDeclarator {
				id: node("id")?,
				init: optional("init"),
			},
			"js.ImportDeclaration" => NodeKind::ImportDeclaration {
				specifiers: list(self, "specifiers")?,
				source: node("source")?,
				attributes: list(self, "attributes")?,
			},
			"js.ImportSpecifier" => NodeKind::ImportSpecifier {
				imported: node("imported")?,
				local: node("local")?,
			},
			"js.ImportDefaultSpecifier" => NodeKind::ImportDefaultSpecifier { local: node("local")? },
			"js.ImportNamespaceSpecifier" => NodeKind::ImportNamespaceSpecifier { local: node("local")? },
			"js.ImportAttribute" => NodeKind::ImportAttribute {
				key: node("key")?,
				value: node("value")?,
			},
			"js.ExportSpecifier" => NodeKind::ExportSpecifier {
				local: node("local")?,
				exported: node("exported")?,
			},
			"js.ExportDefaultDeclaration" => NodeKind::ExportDefaultDeclaration {
				declaration: node("declaration")?,
			},
			"js.ExportAllDeclaration" => NodeKind::ExportAllDeclaration {
				exported: optional("exported"),
				source: node("source")?,
				attributes: list(self, "attributes")?,
			},
			"js.Program" => NodeKind::Program {
				body: list(self, "body")?,
				module: self.text_of(get("sourceType")) == Some("module"),
			},
			"js.FunctionExpression" | "js.FunctionDeclaration" => {
				let function = crate::ast::Function {
					id: optional("id"),
					params: list(self, "params")?,
					body: node("body")?,
					generator: get("generator").yes(),
					is_async: get("async").yes(),
				};
				if ty == "js.FunctionExpression" {
					NodeKind::FunctionExpression { function }
				} else {
					NodeKind::FunctionDeclaration { function }
				}
			}
			"js.ClassExpression" | "js.ClassDeclaration" => {
				let class = crate::ast::Class {
					id: optional("id"),
					super_class: optional("superClass"),
					body: node("body")?,
				};
				if ty == "js.ClassExpression" {
					NodeKind::ClassExpression { class }
				} else {
					NodeKind::ClassDeclaration { class }
				}
			}
			"js.ExportNamedDeclaration" => {
				if let Some(declaration) = optional("declaration") {
					NodeKind::ExportDeclaration { declaration }
				} else {
					NodeKind::ExportNamedDeclaration {
						specifiers: list(self, "specifiers")?,
						source: optional("source"),
						attributes: list(self, "attributes")?,
					}
				}
			}
			"js.TemplateElement" => {
				let value = get("value");
				let raw = self.property(
					*value,
					&Path::Name(Property {
						key: Key::Raw,
						name: Symbol::WordRaw.id(),
					}),
				);
				let raw = self
					.text_of(&raw)
					.ok_or_else(|| error(start, end, Code::Expected, Some("raw")))?
					.to_owned();
				let cooked = self.property(
					*value,
					&Path::Name(Property {
						key: Key::Cooked,
						name: Symbol::WordCooked.id(),
					}),
				);
				let cooked = self.text_of(&cooked).map(str::to_owned).map(|s| self.intern(&s));
				NodeKind::TemplateElement {
					raw: self.intern(&raw),
					cooked,
					tail: get("tail").yes(),
				}
			}
			"js.Literal" => {
				if !matches!(get("regex"), Datum::Missing | Datum::Null) {
					let value = get("regex");
					let pattern = self.property(
						*value,
						&Path::Name(Property {
							key: Key::Pattern,
							name: Symbol::WordPattern.id(),
						}),
					);
					let flags = self.property(
						*value,
						&Path::Name(Property {
							key: Key::Flags,
							name: Symbol::WordFlags.id(),
						}),
					);
					let pattern = self
						.text_of(&pattern)
						.ok_or_else(|| error(start, end, Code::Expected, Some("pattern")))?
						.to_owned();
					let flags = self
						.text_of(&flags)
						.ok_or_else(|| error(start, end, Code::Expected, Some("flags")))?
						.to_owned();
					NodeKind::RegExpLiteral {
						pattern: self.intern(&pattern),
						flags: self.intern(&flags),
					}
				} else if !matches!(get("bigint"), Datum::Missing | Datum::Null) {
					if start == end
						|| !self
							.src
							.get(start as usize..end as usize)
							.is_some_and(|s| s.ends_with('n'))
					{
						return fail(start, end, Code::Expected, Some("a bigint source span"));
					}
					NodeKind::BigIntLiteral
				} else {
					match get("value") {
						Datum::Null => NodeKind::NullLiteral,
						Datum::Bool(value) => NodeKind::BooleanLiteral { value: *value },
						Datum::Number(value) => {
							let index = self.tree().numbers.len() as u32;
							self.ast().numbers.push(*value);
							NodeKind::NumberLiteral { value: index }
						}
						value if self.text_of(value).is_some() => NodeKind::StringLiteral {
							value: string(self, "value")?,
						},
						_ => return fail(start, end, Code::Expected, Some("a literal value")),
					}
				}
			}
			_ => return fail(start, end, Code::Expected, Some("a native constructor")),
		})
	}
}
