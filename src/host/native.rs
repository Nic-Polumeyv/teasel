use super::*;

impl<E: Extension> Walker<'_, E> {
	pub(super) fn native_property(&self, node: NodeId, field_name: &str) -> Datum {
		match self.tree().node(node).kind {
			NodeKind::Program { body, module } => match field_name {
				"type" => Datum::Static("Program"),
				"body" => Datum::Nodes(body),
				"sourceType" => Datum::Text(if module { "module" } else { "script" }.into()),
				_ => Datum::Missing,
			},
			NodeKind::Identifier { name } => match field_name {
				"type" => Datum::Static("Identifier"),
				"name" => Datum::Interned(name),
				_ => Datum::Missing,
			},
			NodeKind::PrivateIdentifier { name } => match field_name {
				"type" => Datum::Static("PrivateIdentifier"),
				"name" => Datum::Interned(name),
				_ => Datum::Missing,
			},
			NodeKind::NumberLiteral { value } => match field_name {
				"type" => Datum::Static("Literal"),
				"value" => Datum::Number(self.tree().numbers[value as usize]),
				_ => Datum::Missing,
			},
			NodeKind::BigIntLiteral => match field_name {
				"type" => Datum::Static("Literal"),
				"value" => Datum::Null,
				"bigint" => Datum::Text(
					crate::estree::bigint_decimal(
						&self.src[self.tree().node(node).start as usize..self.tree().node(node).end as usize - 1],
					)
					.into(),
				),
				_ => Datum::Missing,
			},
			NodeKind::StringLiteral { value } => match field_name {
				"type" => Datum::Static("Literal"),
				"value" => Datum::Interned(value),
				_ => Datum::Missing,
			},
			NodeKind::BooleanLiteral { value } => match field_name {
				"type" => Datum::Static("Literal"),
				"value" => Datum::Bool(value),
				_ => Datum::Missing,
			},
			NodeKind::NullLiteral => match field_name {
				"type" => Datum::Static("Literal"),
				"value" => Datum::Null,
				_ => Datum::Missing,
			},
			NodeKind::RegExpLiteral { pattern, flags } => match field_name {
				"type" => Datum::Static("Literal"),
				"value" => Datum::Null,
				"regex" => Datum::facts([("pattern", Datum::Interned(pattern)), ("flags", Datum::Interned(flags))]),
				"pattern" => Datum::Interned(pattern),
				"flags" => Datum::Interned(flags),
				_ => Datum::Missing,
			},
			NodeKind::TemplateLiteral { quasis, expressions } => match field_name {
				"type" => Datum::Static("TemplateLiteral"),
				"quasis" => Datum::Nodes(quasis),
				"expressions" => Datum::Nodes(expressions),
				_ => Datum::Missing,
			},
			NodeKind::TemplateElement { cooked, raw, tail } => match field_name {
				"type" => Datum::Static("TemplateElement"),
				"value" => Datum::facts([
					("raw", Datum::Interned(raw)),
					("cooked", cooked.map_or(Datum::Null, Datum::Interned)),
				]),
				"cooked" => cooked.map_or(Datum::Null, Datum::Interned),
				"raw" => Datum::Interned(raw),
				"tail" => Datum::Bool(tail),
				_ => Datum::Missing,
			},
			NodeKind::TaggedTemplateExpression { tag, quasi } => match field_name {
				"type" => Datum::Static("TaggedTemplateExpression"),
				"tag" => Datum::Node(tag),
				"quasi" => Datum::Node(quasi),
				_ => Datum::Missing,
			},
			NodeKind::ThisExpression => match field_name {
				"type" => Datum::Static("ThisExpression"),

				_ => Datum::Missing,
			},
			NodeKind::Super => match field_name {
				"type" => Datum::Static("Super"),

				_ => Datum::Missing,
			},
			NodeKind::ArrayExpression { elements } => match field_name {
				"type" => Datum::Static("ArrayExpression"),
				"elements" => Datum::Nodes(elements),
				_ => Datum::Missing,
			},
			NodeKind::ObjectExpression { properties } => match field_name {
				"type" => Datum::Static("ObjectExpression"),
				"properties" => Datum::Nodes(properties),
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
				"type" => Datum::Static("Property"),
				"key" => Datum::Node(key),
				"value" => Datum::Node(value),
				"kind" => Datum::Text(
					match kind {
						crate::ast::PropertyKind::Init => "init",
						crate::ast::PropertyKind::Get => "get",
						crate::ast::PropertyKind::Set => "set",
					}
					.into(),
				),
				"computed" => Datum::Bool(computed),
				"method" => Datum::Bool(method),
				"shorthand" => Datum::Bool(shorthand),
				_ => Datum::Missing,
			},
			NodeKind::SpreadElement { argument } => match field_name {
				"type" => Datum::Static("SpreadElement"),
				"argument" => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::UnaryExpression { operator, argument } => match field_name {
				"type" => Datum::Static("UnaryExpression"),
				"operator" => Datum::Static(operator.name().text),
				"argument" => Datum::Node(argument),
				"prefix" => Datum::Bool(true),
				_ => Datum::Missing,
			},
			NodeKind::UpdateExpression {
				operator,
				prefix,
				argument,
			} => match field_name {
				"type" => Datum::Static("UpdateExpression"),
				"operator" => Datum::Static(operator.name().text),
				"prefix" => Datum::Bool(prefix),
				"argument" => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::BinaryExpression { operator, left, right } => match field_name {
				"type" => Datum::Static("BinaryExpression"),
				"operator" => Datum::Static(operator.name().text),
				"left" => Datum::Node(left),
				"right" => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::LogicalExpression { operator, left, right } => match field_name {
				"type" => Datum::Static("LogicalExpression"),
				"operator" => Datum::Static(operator.name().text),
				"left" => Datum::Node(left),
				"right" => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::AssignmentExpression { operator, left, right } => match field_name {
				"type" => Datum::Static("AssignmentExpression"),
				"operator" => Datum::Static(operator.name().text),
				"left" => Datum::Node(left),
				"right" => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::ConditionalExpression {
				test,
				consequent,
				alternate,
			} => match field_name {
				"type" => Datum::Static("ConditionalExpression"),
				"test" => Datum::Node(test),
				"consequent" => Datum::Node(consequent),
				"alternate" => Datum::Node(alternate),
				_ => Datum::Missing,
			},
			NodeKind::MemberExpression {
				object,
				property,
				computed,
				optional,
			} => match field_name {
				"type" => Datum::Static("MemberExpression"),
				"object" => Datum::Node(object),
				"property" => Datum::Node(property),
				"computed" => Datum::Bool(computed),
				"optional" => Datum::Bool(optional),
				_ => Datum::Missing,
			},
			NodeKind::CallExpression {
				callee,
				arguments,
				optional,
			} => match field_name {
				"type" => Datum::Static("CallExpression"),
				"callee" => Datum::Node(callee),
				"arguments" => Datum::Nodes(arguments),
				"optional" => Datum::Bool(optional),
				_ => Datum::Missing,
			},
			NodeKind::ChainExpression { expression } => match field_name {
				"type" => Datum::Static("ChainExpression"),
				"expression" => Datum::Node(expression),
				_ => Datum::Missing,
			},
			NodeKind::NewExpression { callee, arguments } => match field_name {
				"type" => Datum::Static("NewExpression"),
				"callee" => Datum::Node(callee),
				"arguments" => Datum::Nodes(arguments),
				_ => Datum::Missing,
			},
			NodeKind::SequenceExpression { expressions } => match field_name {
				"type" => Datum::Static("SequenceExpression"),
				"expressions" => Datum::Nodes(expressions),
				_ => Datum::Missing,
			},
			NodeKind::ArrowFunctionExpression {
				params,
				body,
				expression,
				is_async,
			} => match field_name {
				"type" => Datum::Static("ArrowFunctionExpression"),
				"params" => Datum::Nodes(params),
				"body" => Datum::Node(body),
				"expression" => Datum::Bool(expression),
				"async" => Datum::Bool(is_async),
				_ => Datum::Missing,
			},
			NodeKind::FunctionExpression { function } => match field_name {
				"type" => Datum::Static("FunctionExpression"),
				"id" => function.id.map_or(Datum::Null, Datum::Node),
				"body" => Datum::Node(function.body),
				"params" => Datum::Nodes(function.params),
				"async" => Datum::Bool(function.is_async),
				"generator" => Datum::Bool(function.generator),
				"expression" => Datum::Bool(false),
				_ => Datum::Missing,
			},
			NodeKind::FunctionDeclaration { function } => match field_name {
				"type" => Datum::Static("FunctionDeclaration"),
				"id" => function.id.map_or(Datum::Null, Datum::Node),
				"body" => Datum::Node(function.body),
				"params" => Datum::Nodes(function.params),
				"async" => Datum::Bool(function.is_async),
				"generator" => Datum::Bool(function.generator),
				"expression" => Datum::Bool(false),
				_ => Datum::Missing,
			},
			NodeKind::ClassExpression { class } => match field_name {
				"type" => Datum::Static("ClassExpression"),
				"id" => class.id.map_or(Datum::Null, Datum::Node),
				"body" => Datum::Node(class.body),
				"superClass" => class.super_class.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ClassDeclaration { class } => match field_name {
				"type" => Datum::Static("ClassDeclaration"),
				"id" => class.id.map_or(Datum::Null, Datum::Node),
				"body" => Datum::Node(class.body),
				"superClass" => class.super_class.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ClassBody { body } => match field_name {
				"type" => Datum::Static("ClassBody"),
				"body" => Datum::Nodes(body),
				_ => Datum::Missing,
			},
			NodeKind::MethodDefinition {
				key,
				value,
				kind,
				computed,
				is_static,
			} => match field_name {
				"type" => Datum::Static("MethodDefinition"),
				"key" => Datum::Node(key),
				"value" => Datum::Node(value),
				"kind" => Datum::Text(
					match kind {
						crate::ast::MethodKind::Constructor => "constructor",
						crate::ast::MethodKind::Method => "method",
						crate::ast::MethodKind::Get => "get",
						crate::ast::MethodKind::Set => "set",
					}
					.into(),
				),
				"computed" => Datum::Bool(computed),
				"static" => Datum::Bool(is_static),
				_ => Datum::Missing,
			},
			NodeKind::PropertyDefinition {
				key,
				value,
				computed,
				is_static,
			} => match field_name {
				"type" => Datum::Static("PropertyDefinition"),
				"key" => Datum::Node(key),
				"value" => value.map_or(Datum::Null, Datum::Node),
				"computed" => Datum::Bool(computed),
				"static" => Datum::Bool(is_static),
				_ => Datum::Missing,
			},
			NodeKind::StaticBlock { body } => match field_name {
				"type" => Datum::Static("StaticBlock"),
				"body" => Datum::Nodes(body),
				_ => Datum::Missing,
			},
			NodeKind::YieldExpression { argument, delegate } => match field_name {
				"type" => Datum::Static("YieldExpression"),
				"argument" => argument.map_or(Datum::Null, Datum::Node),
				"delegate" => Datum::Bool(delegate),
				_ => Datum::Missing,
			},
			NodeKind::AwaitExpression { argument } => match field_name {
				"type" => Datum::Static("AwaitExpression"),
				"argument" => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::MetaProperty { meta, property } => match field_name {
				"type" => Datum::Static("MetaProperty"),
				"meta" => Datum::Node(meta),
				"property" => Datum::Node(property),
				_ => Datum::Missing,
			},
			NodeKind::ImportExpression { source, options } => match field_name {
				"type" => Datum::Static("ImportExpression"),
				"source" => Datum::Node(source),
				"options" => options.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ObjectPattern { properties } => match field_name {
				"type" => Datum::Static("ObjectPattern"),
				"properties" => Datum::Nodes(properties),
				_ => Datum::Missing,
			},
			NodeKind::ArrayPattern { elements } => match field_name {
				"type" => Datum::Static("ArrayPattern"),
				"elements" => Datum::Nodes(elements),
				_ => Datum::Missing,
			},
			NodeKind::RestElement { argument } => match field_name {
				"type" => Datum::Static("RestElement"),
				"argument" => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::AssignmentPattern { left, right } => match field_name {
				"type" => Datum::Static("AssignmentPattern"),
				"left" => Datum::Node(left),
				"right" => Datum::Node(right),
				_ => Datum::Missing,
			},
			NodeKind::ExpressionStatement { expression, directive } => match field_name {
				"type" => Datum::Static("ExpressionStatement"),
				"expression" => Datum::Node(expression),
				"directive" => directive.map_or(Datum::Null, Datum::Interned),
				_ => Datum::Missing,
			},
			NodeKind::BlockStatement { body } => match field_name {
				"type" => Datum::Static("BlockStatement"),
				"body" => Datum::Nodes(body),
				_ => Datum::Missing,
			},
			NodeKind::EmptyStatement => match field_name {
				"type" => Datum::Static("EmptyStatement"),

				_ => Datum::Missing,
			},
			NodeKind::DebuggerStatement => match field_name {
				"type" => Datum::Static("DebuggerStatement"),

				_ => Datum::Missing,
			},
			NodeKind::WithStatement { object, body } => match field_name {
				"type" => Datum::Static("WithStatement"),
				"object" => Datum::Node(object),
				"body" => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::ReturnStatement { argument } => match field_name {
				"type" => Datum::Static("ReturnStatement"),
				"argument" => argument.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::LabeledStatement { label, body } => match field_name {
				"type" => Datum::Static("LabeledStatement"),
				"label" => Datum::Node(label),
				"body" => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::BreakStatement { label } => match field_name {
				"type" => Datum::Static("BreakStatement"),
				"label" => label.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ContinueStatement { label } => match field_name {
				"type" => Datum::Static("ContinueStatement"),
				"label" => label.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::IfStatement {
				test,
				consequent,
				alternate,
			} => match field_name {
				"type" => Datum::Static("IfStatement"),
				"test" => Datum::Node(test),
				"consequent" => Datum::Node(consequent),
				"alternate" => alternate.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::SwitchStatement { discriminant, cases } => match field_name {
				"type" => Datum::Static("SwitchStatement"),
				"discriminant" => Datum::Node(discriminant),
				"cases" => Datum::Nodes(cases),
				_ => Datum::Missing,
			},
			NodeKind::SwitchCase { test, consequent } => match field_name {
				"type" => Datum::Static("SwitchCase"),
				"test" => test.map_or(Datum::Null, Datum::Node),
				"consequent" => Datum::Nodes(consequent),
				_ => Datum::Missing,
			},
			NodeKind::ThrowStatement { argument } => match field_name {
				"type" => Datum::Static("ThrowStatement"),
				"argument" => Datum::Node(argument),
				_ => Datum::Missing,
			},
			NodeKind::TryStatement {
				block,
				handler,
				finalizer,
			} => match field_name {
				"type" => Datum::Static("TryStatement"),
				"block" => Datum::Node(block),
				"handler" => handler.map_or(Datum::Null, Datum::Node),
				"finalizer" => finalizer.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::CatchClause { param, body } => match field_name {
				"type" => Datum::Static("CatchClause"),
				"param" => param.map_or(Datum::Null, Datum::Node),
				"body" => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::WhileStatement { test, body } => match field_name {
				"type" => Datum::Static("WhileStatement"),
				"test" => Datum::Node(test),
				"body" => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::DoWhileStatement { body, test } => match field_name {
				"type" => Datum::Static("DoWhileStatement"),
				"body" => Datum::Node(body),
				"test" => Datum::Node(test),
				_ => Datum::Missing,
			},
			NodeKind::ForStatement {
				init,
				test,
				update,
				body,
			} => match field_name {
				"type" => Datum::Static("ForStatement"),
				"init" => init.map_or(Datum::Null, Datum::Node),
				"test" => test.map_or(Datum::Null, Datum::Node),
				"update" => update.map_or(Datum::Null, Datum::Node),
				"body" => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::ForInStatement { left, right, body } => match field_name {
				"type" => Datum::Static("ForInStatement"),
				"left" => Datum::Node(left),
				"right" => Datum::Node(right),
				"body" => Datum::Node(body),
				_ => Datum::Missing,
			},
			NodeKind::ForOfStatement {
				left,
				right,
				body,
				is_await,
			} => match field_name {
				"type" => Datum::Static("ForOfStatement"),
				"left" => Datum::Node(left),
				"right" => Datum::Node(right),
				"body" => Datum::Node(body),
				"is_await" => Datum::Bool(is_await),
				_ => Datum::Missing,
			},
			NodeKind::VariableDeclaration { declarations, kind } => match field_name {
				"type" => Datum::Static("VariableDeclaration"),
				"declarations" => Datum::Nodes(declarations),
				"kind" => Datum::Text(kind.name().text.into()),
				_ => Datum::Missing,
			},
			NodeKind::VariableDeclarator { id, init } => match field_name {
				"type" => Datum::Static("VariableDeclarator"),
				"id" => Datum::Node(id),
				"init" => init.map_or(Datum::Null, Datum::Node),
				_ => Datum::Missing,
			},
			NodeKind::ImportDeclaration {
				specifiers,
				source,
				attributes,
			} => match field_name {
				"type" => Datum::Static("ImportDeclaration"),
				"specifiers" => Datum::Nodes(specifiers),
				"source" => Datum::Node(source),
				"attributes" => Datum::Nodes(attributes),
				_ => Datum::Missing,
			},
			NodeKind::ImportSpecifier { imported, local } => match field_name {
				"type" => Datum::Static("ImportSpecifier"),
				"imported" => Datum::Node(imported),
				"local" => Datum::Node(local),
				_ => Datum::Missing,
			},
			NodeKind::ImportDefaultSpecifier { local } => match field_name {
				"type" => Datum::Static("ImportDefaultSpecifier"),
				"local" => Datum::Node(local),
				_ => Datum::Missing,
			},
			NodeKind::ImportNamespaceSpecifier { local } => match field_name {
				"type" => Datum::Static("ImportNamespaceSpecifier"),
				"local" => Datum::Node(local),
				_ => Datum::Missing,
			},
			NodeKind::ImportAttribute { key, value } => match field_name {
				"type" => Datum::Static("ImportAttribute"),
				"key" => Datum::Node(key),
				"value" => Datum::Node(value),
				_ => Datum::Missing,
			},
			NodeKind::ExportDeclaration { declaration } => match field_name {
				"type" => Datum::Static("ExportNamedDeclaration"),
				"declaration" => Datum::Node(declaration),
				_ => Datum::Missing,
			},
			NodeKind::ExportNamedDeclaration {
				specifiers,
				source,
				attributes,
			} => match field_name {
				"type" => Datum::Static("ExportNamedDeclaration"),
				"specifiers" => Datum::Nodes(specifiers),
				"source" => source.map_or(Datum::Null, Datum::Node),
				"attributes" => Datum::Nodes(attributes),
				_ => Datum::Missing,
			},
			NodeKind::ExportSpecifier { local, exported } => match field_name {
				"type" => Datum::Static("ExportSpecifier"),
				"local" => Datum::Node(local),
				"exported" => Datum::Node(exported),
				_ => Datum::Missing,
			},
			NodeKind::ExportDefaultDeclaration { declaration } => match field_name {
				"type" => Datum::Static("ExportDefaultDeclaration"),
				"declaration" => Datum::Node(declaration),
				_ => Datum::Missing,
			},
			NodeKind::ExportAllDeclaration {
				exported,
				source,
				attributes,
			} => match field_name {
				"type" => Datum::Static("ExportAllDeclaration"),
				"exported" => exported.map_or(Datum::Null, Datum::Node),
				"source" => Datum::Node(source),
				"attributes" => Datum::Nodes(attributes),
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
			let value = this
				.text_of(get(name))
				.ok_or_else(|| error(start, end, Code::Expected, Some(name)))?
				.to_owned();
			Ok(this.intern(&value))
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
				let raw = self.property(value.clone(), &Path::Name("raw".into()));
				let raw = self
					.text_of(&raw)
					.ok_or_else(|| error(start, end, Code::Expected, Some("raw")))?
					.to_owned();
				let cooked = self.property(value.clone(), &Path::Name("cooked".into()));
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
					let pattern = self.property(value.clone(), &Path::Name("pattern".into()));
					let flags = self.property(value.clone(), &Path::Name("flags".into()));
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
