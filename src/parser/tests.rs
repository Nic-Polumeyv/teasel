use crate::ast::{Ast, NodeId, NodeKind};
use crate::{Options, parse, parse_expression_at, parse_params_at, parse_pattern_at, parse_statement_at};

/// Renders a node as its Debug form with ids, strings and lists expanded inline; `extension`
/// renders the nodes an extension owns.
pub(crate) fn dump<X>(ast: &Ast<X>, id: NodeId, extension: &dyn Fn(&Ast<X>, NodeId, u32) -> String) -> String {
	let node = ast.node(id);
	match node.kind {
		NodeKind::Extension(index) => extension(ast, id, index),
		kind => expand(ast, &format!("{kind:?}"), extension),
	}
}

/// Expands the ids inside a Debug rendering.
pub(crate) fn expand<X>(ast: &Ast<X>, text: &str, extension: &dyn Fn(&Ast<X>, NodeId, u32) -> String) -> String {
	let mut out = String::new();
	let mut rest = text;
	while let Some(i) = rest.find(['N', 'S', 'L']) {
		out.push_str(&rest[..i]);
		let tail = &rest[i..];
		if let Some(r) = tail.strip_prefix("NodeId(") {
			let end = r.find(')').unwrap();
			let child = NodeId(r[..end].parse().unwrap());
			out.push_str(&dump(ast, child, extension));
			rest = &r[end + 1..];
		} else if let Some(r) = tail.strip_prefix("StrId(") {
			let end = r.find(')').unwrap();
			let s = ast.str(crate::interner::StrId(r[..end].parse().unwrap()));
			out.push_str(&format!("{s:?}"));
			rest = &r[end + 1..];
		} else if let Some(r) = tail.strip_prefix("List { start: ") {
			let end = r.find(" }").unwrap();
			let (start, len) = r[..end].split_once(", len: ").unwrap();
			let list = crate::ast::List {
				start: start.parse().unwrap(),
				len: len.parse().unwrap(),
			};
			let items: Vec<String> = ast
				.list(list)
				.iter()
				.map(|c| c.map(|c| dump(ast, c, extension)).unwrap_or_else(|| "hole".into()))
				.collect();
			out.push('[');
			out.push_str(&items.join(", "));
			out.push(']');
			rest = &r[end + 2..];
		} else {
			out.push_str(&tail[..1]);
			rest = &tail[1..];
		}
	}
	out.push_str(rest);
	out
}

fn plain(_: &Ast, _: NodeId, _: u32) -> String {
	unreachable!("the JavaScript parser adds no extension nodes")
}

fn expr(src: &str) -> String {
	let (ast, id, _) = parse_expression_at(src, 0, Options::default()).unwrap_or_else(|e| panic!("{src}: {e}"));
	dump(&ast, id, &plain)
}

fn module(src: &str) -> String {
	let ast = parse(
		src,
		Options {
			module: true,
			..Options::default()
		},
	)
	.unwrap_or_else(|e| panic!("{src}: {e}"));
	let root = ast.last();
	let NodeKind::Program { body, .. } = ast.node(root).kind else {
		panic!()
	};
	ast.list(body)
		.iter()
		.map(|s| dump(&ast, s.unwrap(), &plain))
		.collect::<Vec<_>>()
		.join("\n")
}

fn script(src: &str) -> String {
	let ast = parse(src, Options::default()).unwrap_or_else(|e| panic!("{src}: {e}"));
	let root = ast.last();
	let NodeKind::Program { body, .. } = ast.node(root).kind else {
		panic!()
	};
	ast.list(body)
		.iter()
		.map(|s| dump(&ast, s.unwrap(), &plain))
		.collect::<Vec<_>>()
		.join("\n")
}

fn module_error(src: &str) -> String {
	match parse(
		src,
		Options {
			module: true,
			..Options::default()
		},
	) {
		Ok(_) => panic!("no error for {src:?}"),
		Err(e) => format!("{} ({})", e.message, e.pos),
	}
}

#[test]
fn consumed_end() {
	let options = Options {
		module: true,
		..Options::default()
	};
	let end = |src: &str, at: u32| parse_expression_at(src, at, options).unwrap().2;
	assert_eq!(end("{a /* c */ }", 1), 10);
	assert_eq!(end("{(a) }", 1), 4);
	assert_eq!(end("{a} /* c */", 1), 2);
	assert_eq!(parse_statement_at("{@const x = 1}", 2, options).unwrap().2, 13);
	assert_eq!(parse_params_at("{#snippet s(a) /* c */}", 11, options).unwrap().2, 22);
}

fn span(src: &str) -> (u32, u32) {
	let (ast, id, _) = parse_expression_at(src, 0, Options::default()).unwrap();
	(ast.node(id).start, ast.node(id).end)
}

#[test]
fn literals() {
	assert_eq!(expr("42"), "NumberLiteral { value: 42.0 }");
	assert_eq!(expr("'hi'"), r#"StringLiteral { value: "hi" }"#);
	assert_eq!(expr("true"), "BooleanLiteral { value: true }");
	assert_eq!(expr("null"), "NullLiteral");
	assert_eq!(expr("10n"), "BigIntLiteral");
	assert_eq!(expr("/a+/gi"), r#"RegExpLiteral { pattern: "a+", flags: "gi" }"#);
	assert_eq!(expr("this"), "ThisExpression");
}

#[test]
fn binary_precedence() {
	assert_eq!(
		expr("a + b * c"),
		r#"BinaryExpression { operator: Add, left: Identifier { name: "a" }, right: BinaryExpression { operator: Mul, left: Identifier { name: "b" }, right: Identifier { name: "c" } } }"#
	);
	assert_eq!(
		expr("a ** b ** c"),
		r#"BinaryExpression { operator: Exp, left: Identifier { name: "a" }, right: BinaryExpression { operator: Exp, left: Identifier { name: "b" }, right: Identifier { name: "c" } } }"#
	);
	assert_eq!(
		expr("a || b && c"),
		r#"LogicalExpression { operator: Or, left: Identifier { name: "a" }, right: LogicalExpression { operator: And, left: Identifier { name: "b" }, right: Identifier { name: "c" } } }"#
	);
	assert_eq!(
		expr("a ?? b"),
		r#"LogicalExpression { operator: Nullish, left: Identifier { name: "a" }, right: Identifier { name: "b" } }"#
	);
	assert_eq!(
		expr("a - b - c"),
		r#"BinaryExpression { operator: Sub, left: BinaryExpression { operator: Sub, left: Identifier { name: "a" }, right: Identifier { name: "b" } }, right: Identifier { name: "c" } }"#
	);
}

#[test]
fn unary_and_update() {
	assert_eq!(
		expr("-a"),
		r#"UnaryExpression { operator: Minus, argument: Identifier { name: "a" } }"#
	);
	assert_eq!(
		expr("typeof a"),
		r#"UnaryExpression { operator: Typeof, argument: Identifier { name: "a" } }"#
	);
	assert_eq!(
		expr("a++"),
		r#"UpdateExpression { operator: Increment, prefix: false, argument: Identifier { name: "a" } }"#
	);
	assert_eq!(
		expr("--a"),
		r#"UpdateExpression { operator: Decrement, prefix: true, argument: Identifier { name: "a" } }"#
	);
	assert!(!expr("(-a) ** 2").contains("ParenthesizedExpression"));
}

#[test]
fn members_and_calls() {
	assert_eq!(
		expr("a.b[c](d)"),
		r#"CallExpression { callee: MemberExpression { object: MemberExpression { object: Identifier { name: "a" }, property: Identifier { name: "b" }, computed: false, optional: false }, property: Identifier { name: "c" }, computed: true, optional: false }, arguments: [Identifier { name: "d" }], optional: false }"#
	);
	assert_eq!(
		expr("a?.b"),
		r#"ChainExpression { expression: MemberExpression { object: Identifier { name: "a" }, property: Identifier { name: "b" }, computed: false, optional: true } }"#
	);
	assert_eq!(
		expr("new A(1)"),
		r#"NewExpression { callee: Identifier { name: "A" }, arguments: [NumberLiteral { value: 1.0 }] }"#
	);
	assert_eq!(
		expr("new A"),
		r#"NewExpression { callee: Identifier { name: "A" }, arguments: [] }"#
	);
	assert_eq!(
		expr("f(...a, b)"),
		r#"CallExpression { callee: Identifier { name: "f" }, arguments: [SpreadElement { argument: Identifier { name: "a" } }, Identifier { name: "b" }], optional: false }"#
	);
}

#[test]
fn arrows() {
	assert_eq!(
		expr("x => x"),
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: false }"#
	);
	assert_eq!(
		expr("(a, b = 1, ...c) => {}"),
		r#"ArrowFunctionExpression { params: [Identifier { name: "a" }, AssignmentPattern { left: Identifier { name: "b" }, right: NumberLiteral { value: 1.0 } }, RestElement { argument: Identifier { name: "c" } }], body: BlockStatement { body: [] }, expression: false, is_async: false }"#
	);
	assert_eq!(
		expr("async x => await x"),
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: AwaitExpression { argument: Identifier { name: "x" } }, expression: true, is_async: true }"#
	);
	assert_eq!(
		expr("async (x) => x"),
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: true }"#
	);
	assert_eq!(
		expr("async(x)"),
		r#"CallExpression { callee: Identifier { name: "async" }, arguments: [Identifier { name: "x" }], optional: false }"#
	);
}

#[test]
fn objects_and_arrays() {
	assert_eq!(
		expr("{a, b: 1, [c]: 2, d() {}, get e() {}, ...f}"),
		r#"ObjectExpression { properties: [Property { key: Identifier { name: "a" }, value: Identifier { name: "a" }, kind: Init, computed: false, method: false, shorthand: true }, Property { key: Identifier { name: "b" }, value: NumberLiteral { value: 1.0 }, kind: Init, computed: false, method: false, shorthand: false }, Property { key: Identifier { name: "c" }, value: NumberLiteral { value: 2.0 }, kind: Init, computed: true, method: false, shorthand: false }, Property { key: Identifier { name: "d" }, value: FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [] }, is_async: false, generator: false } }, kind: Init, computed: false, method: true, shorthand: false }, Property { key: Identifier { name: "e" }, value: FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [] }, is_async: false, generator: false } }, kind: Get, computed: false, method: false, shorthand: false }, SpreadElement { argument: Identifier { name: "f" } }] }"#
	);
	assert_eq!(
		expr("[1, , 3]"),
		"ArrayExpression { elements: [NumberLiteral { value: 1.0 }, hole, NumberLiteral { value: 3.0 }] }"
	);
}

#[test]
fn destructuring_assignment() {
	assert_eq!(
		expr("[a, {b, c = 1}, ...d] = e"),
		r#"AssignmentExpression { operator: Assign, left: ArrayPattern { elements: [Identifier { name: "a" }, ObjectPattern { properties: [Property { key: Identifier { name: "b" }, value: Identifier { name: "b" }, kind: Init, computed: false, method: false, shorthand: true }, Property { key: Identifier { name: "c" }, value: AssignmentPattern { left: Identifier { name: "c" }, right: NumberLiteral { value: 1.0 } }, kind: Init, computed: false, method: false, shorthand: true }] }, RestElement { argument: Identifier { name: "d" } }] }, right: Identifier { name: "e" } }"#
	);
}

#[test]
fn templates() {
	assert_eq!(
		expr("`a${b}c`"),
		r#"TemplateLiteral { quasis: [TemplateElement { cooked: Some("a"), raw: "a", tail: false }, TemplateElement { cooked: Some("c"), raw: "c", tail: true }], expressions: [Identifier { name: "b" }] }"#
	);
	assert_eq!(
		expr("tag`x`"),
		r#"TaggedTemplateExpression { tag: Identifier { name: "tag" }, quasi: TemplateLiteral { quasis: [TemplateElement { cooked: Some("x"), raw: "x", tail: true }], expressions: [] } }"#
	);
	assert_eq!(span("`a${b}c` "), (0, 8));
}

#[test]
fn conditional_and_sequence() {
	assert_eq!(
		expr("a ? b : c"),
		r#"ConditionalExpression { test: Identifier { name: "a" }, consequent: Identifier { name: "b" }, alternate: Identifier { name: "c" } }"#
	);
	assert_eq!(
		expr("a, b"),
		r#"SequenceExpression { expressions: [Identifier { name: "a" }, Identifier { name: "b" }] }"#
	);
}

#[test]
fn expression_ends_where_it_ends() {
	assert_eq!(span("a + b }"), (0, 5));
	assert_eq!(span("  x"), (2, 3));
	let (ast, id, _) = parse_expression_at("{ a.b }", 2, Options::default()).unwrap();
	assert_eq!((ast.node(id).start, ast.node(id).end), (2, 5));
}

#[test]
fn preserve_parens() {
	let (ast, id, _) = parse_expression_at(
		"(a)",
		0,
		Options {
			preserve_parens: true,
			..Options::default()
		},
	)
	.unwrap();
	assert_eq!(
		dump(&ast, id, &plain),
		r#"ParenthesizedExpression { expression: Identifier { name: "a" } }"#
	);
}

#[test]
fn statements() {
	assert_eq!(
		script("var a = 1, b;"),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "a" }, init: Some(NumberLiteral { value: 1.0 }) }, VariableDeclarator { id: Identifier { name: "b" }, init: None }], kind: Var }"#
	);
	assert_eq!(
		script("if (a) b; else { c }"),
		r#"IfStatement { test: Identifier { name: "a" }, consequent: ExpressionStatement { expression: Identifier { name: "b" }, directive: None }, alternate: Some(BlockStatement { body: [ExpressionStatement { expression: Identifier { name: "c" }, directive: None }] }) }"#
	);
	assert_eq!(
		script("for (let i = 0; i < 1; i++) {}"),
		r#"ForStatement { init: Some(VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "i" }, init: Some(NumberLiteral { value: 0.0 }) }], kind: Let }), test: Some(BinaryExpression { operator: Lt, left: Identifier { name: "i" }, right: NumberLiteral { value: 1.0 } }), update: Some(UpdateExpression { operator: Increment, prefix: false, argument: Identifier { name: "i" } }), body: BlockStatement { body: [] } }"#
	);
	assert_eq!(
		script("for (const x of xs) ;"),
		r#"ForOfStatement { left: VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: None }], kind: Const }, right: Identifier { name: "xs" }, body: EmptyStatement, is_await: false }"#
	);
	assert_eq!(
		script("for (k in o) ;"),
		r#"ForInStatement { left: Identifier { name: "k" }, right: Identifier { name: "o" }, body: EmptyStatement }"#
	);
	assert_eq!(
		script("label: while (true) { break label; }"),
		r#"LabeledStatement { label: Identifier { name: "label" }, body: WhileStatement { test: BooleanLiteral { value: true }, body: BlockStatement { body: [BreakStatement { label: Some(Identifier { name: "label" }) }] } } }"#
	);
	assert_eq!(
		script("switch (a) { case 1: b; default: }"),
		r#"SwitchStatement { discriminant: Identifier { name: "a" }, cases: [SwitchCase { test: Some(NumberLiteral { value: 1.0 }), consequent: [ExpressionStatement { expression: Identifier { name: "b" }, directive: None }] }, SwitchCase { test: None, consequent: [] }] }"#
	);
	assert_eq!(
		script("try { a } catch (e) { b } finally { c }"),
		r#"TryStatement { block: BlockStatement { body: [ExpressionStatement { expression: Identifier { name: "a" }, directive: None }] }, handler: Some(CatchClause { param: Some(Identifier { name: "e" }), body: BlockStatement { body: [ExpressionStatement { expression: Identifier { name: "b" }, directive: None }] } }), finalizer: Some(BlockStatement { body: [ExpressionStatement { expression: Identifier { name: "c" }, directive: None }] }) }"#
	);
	assert_eq!(
		script("'use strict'"),
		r#"ExpressionStatement { expression: StringLiteral { value: "use strict" }, directive: Some("use strict") }"#
	);
}

#[test]
fn functions_and_classes() {
	assert_eq!(
		script("function f(a, b = 2) { return a }"),
		r#"FunctionDeclaration { function: Function { id: Some(Identifier { name: "f" }), params: [Identifier { name: "a" }, AssignmentPattern { left: Identifier { name: "b" }, right: NumberLiteral { value: 2.0 } }], body: BlockStatement { body: [ReturnStatement { argument: Some(Identifier { name: "a" }) }] }, is_async: false, generator: false } }"#
	);
	assert_eq!(
		script("async function* g() { yield 1; await 2 }"),
		r#"FunctionDeclaration { function: Function { id: Some(Identifier { name: "g" }), params: [], body: BlockStatement { body: [ExpressionStatement { expression: YieldExpression { argument: Some(NumberLiteral { value: 1.0 }), delegate: false }, directive: None }, ExpressionStatement { expression: AwaitExpression { argument: NumberLiteral { value: 2.0 } }, directive: None }] }, is_async: true, generator: true } }"#
	);
	assert_eq!(
		script(
			"class A extends B { #x = 1; static y; constructor() { super() } get z() { return this.#x } static { } }"
		),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: Some(Identifier { name: "B" }), body: ClassBody { body: [PropertyDefinition { key: PrivateIdentifier { name: "x" }, value: Some(NumberLiteral { value: 1.0 }), computed: false, is_static: false }, PropertyDefinition { key: Identifier { name: "y" }, value: None, computed: false, is_static: true }, MethodDefinition { key: Identifier { name: "constructor" }, value: FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [ExpressionStatement { expression: CallExpression { callee: Super, arguments: [], optional: false }, directive: None }] }, is_async: false, generator: false } }, kind: Constructor, computed: false, is_static: false }, MethodDefinition { key: Identifier { name: "z" }, value: FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [ReturnStatement { argument: Some(MemberExpression { object: ThisExpression, property: PrivateIdentifier { name: "x" }, computed: false, optional: false }) }] }, is_async: false, generator: false } }, kind: Get, computed: false, is_static: false }, StaticBlock { body: [] }] } } }"#
	);
}

#[test]
fn modules() {
	assert_eq!(
		module("import a, { b as c } from 'm';"),
		r#"ImportDeclaration { specifiers: [ImportDefaultSpecifier { local: Identifier { name: "a" } }, ImportSpecifier { imported: Identifier { name: "b" }, local: Identifier { name: "c" } }], source: StringLiteral { value: "m" }, attributes: [] }"#
	);
	assert_eq!(
		module("import * as ns from 'm' with { type: 'json' };"),
		r#"ImportDeclaration { specifiers: [ImportNamespaceSpecifier { local: Identifier { name: "ns" } }], source: StringLiteral { value: "m" }, attributes: [ImportAttribute { key: Identifier { name: "type" }, value: StringLiteral { value: "json" } }] }"#
	);
	assert_eq!(
		module("export const x = 1;"),
		r#"ExportNamedDeclaration { declaration: Some(VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NumberLiteral { value: 1.0 }) }], kind: Const }), specifiers: [], source: None, attributes: [] }"#
	);
	assert_eq!(
		module("let x; export { x as y };"),
		"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: \"x\" }, init: None }], kind: Let }\nExportNamedDeclaration { declaration: None, specifiers: [ExportSpecifier { local: Identifier { name: \"x\" }, exported: Identifier { name: \"y\" } }], source: None, attributes: [] }"
	);
	assert_eq!(
		module("export default 1;"),
		"ExportDefaultDeclaration { declaration: NumberLiteral { value: 1.0 } }"
	);
	assert_eq!(
		module("export * as all from 'm';"),
		r#"ExportAllDeclaration { exported: Some(Identifier { name: "all" }), source: StringLiteral { value: "m" }, attributes: [] }"#
	);
	assert_eq!(
		module("import.meta.url"),
		r#"ExpressionStatement { expression: MemberExpression { object: MetaProperty { meta: Identifier { name: "import" }, property: Identifier { name: "meta" } }, property: Identifier { name: "url" }, computed: false, optional: false }, directive: None }"#
	);
	assert_eq!(
		module("await 1;"),
		"ExpressionStatement { expression: AwaitExpression { argument: NumberLiteral { value: 1.0 } }, directive: None }"
	);
}

#[test]
fn errors() {
	assert_eq!(module_error("export { nope };"), "Export 'nope' is not defined (9)");
	assert_eq!(
		module_error("let a; let a;"),
		"Identifier 'a' has already been declared (11)"
	);
	assert_eq!(
		module_error("a ?? b || c"),
		"Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses (7)"
	);
	assert_eq!(module_error("1 = 2"), "Assigning to rvalue (0)");
	assert_eq!(
		module_error("({a = 1})"),
		"Shorthand property assignments are valid only in destructuring patterns (4)"
	);
	assert_eq!(module_error("return"), "'return' outside of function (0)");
	assert_eq!(module_error("break"), "Unsyntactic break (0)");
	assert_eq!(module_error("with (a) {}"), "'with' in strict mode (0)");
	assert_eq!(
		module_error("class A { constructor(){} constructor(){} }"),
		"Duplicate constructor in the same class (26)"
	);
	assert_eq!(
		module_error("a.#x"),
		"Private field '#x' must be declared in an enclosing class (2)"
	);
	assert_eq!(
		module_error("function f(a, a) { 'use strict' }"),
		"Argument name clash (14)"
	);
	assert_eq!(module_error("x = 1 +"), "Unexpected end of input (7)");
	assert_eq!(module_error("x = 1 + )"), "Unexpected token (8)");
}

fn script_error(src: &str) -> String {
	match parse(src, Options::default()) {
		Ok(_) => panic!("no error for {src:?}"),
		Err(e) => format!("{} ({})", e.message, e.pos),
	}
}

#[test]
fn review_fixes() {
	assert_eq!(module_error("x => {} ? 1 : 2"), "Unexpected token (8)");
	assert_eq!(module_error("() => {} ** 2"), "Unexpected token (9)");
	assert_eq!(script_error("'use strict'; with (x) {}"), "'with' in strict mode (14)");
	assert_eq!(script_error("'use strict'; 010"), "Invalid number (14)");
	assert!(parse("with (x) {}", Options::default()).is_ok());
	assert!(
		parse(
			"export {a}; { var a; }",
			Options {
				module: true,
				..Options::default()
			}
		)
		.is_ok()
	);
	assert!(
		parse(
			"export default class A {} export {A}",
			Options {
				module: true,
				..Options::default()
			}
		)
		.is_ok()
	);
	assert_eq!(
		module_error("class A {} export default class A {}"),
		"Identifier 'A' has already been declared (32)"
	);
	assert_eq!(
		script("function f() { 'use strict'; }"),
		r#"FunctionDeclaration { function: Function { id: Some(Identifier { name: "f" }), params: [], body: BlockStatement { body: [ExpressionStatement { expression: StringLiteral { value: "use strict" }, directive: Some("use strict") }] }, is_async: false, generator: false } }"#
	);
	assert_eq!(
		script("('use strict')"),
		r#"ExpressionStatement { expression: StringLiteral { value: "use strict" }, directive: None }"#
	);
	assert_eq!(
		module_error("class A extends B { constructor() { new super() } }"),
		"Invalid use of 'super' (40)"
	);
	assert_eq!(
		module_error("var await = 1"),
		"Cannot use keyword 'await' outside an async function (4)"
	);
	assert_eq!(module_error("({a}) = 1"), "Assigning to rvalue (0)");
	assert_eq!(
		module_error("export {a}; export {a as b};"),
		"Export 'a' is not defined (20)"
	);
	assert!(parse("({ \\u0069f: 1 }); x.\\u0074his; ({ i\\u0066: 1 })", Options::default()).is_ok());
	assert_eq!(script_error("\\u0069f (x) {}"), "Escape sequence in keyword if (0)");
	assert_eq!(
		script_error("import /* unterminated"),
		"'import' and 'export' may appear only with 'sourceType: module' (0)"
	);
	assert_eq!(script_error("if (x) let \\u0061 = 1"), "Unexpected token (7)");
	assert!(parse("function f(){ \u{2000}'use strict'; with(x){} }", Options::default()).is_err());
	assert_eq!(
		module_error("async function f(){ for await (-x of y); }"),
		"Unexpected token (31)"
	);
	std::thread::Builder::new()
		.stack_size(64 << 20)
		.spawn(|| {
			let deep = format!("{}1{}", "(".repeat(2000), ")".repeat(2000));
			assert_eq!(module_error(&deep).as_str(), "Maximum nesting depth exceeded (499)");
			assert!(parse(&format!("{}1{}", "(".repeat(400), ")".repeat(400)), Options::default()).is_ok());
			let chain = format!("x = 1{}", " + 1".repeat(9000));
			assert!(parse(&chain, Options::default()).is_ok());
			let chain = format!("x = 1{}", " + 1".repeat(20000));
			assert_eq!(module_error(&chain).as_str(), "Maximum nesting depth exceeded (40006)");
		})
		.unwrap()
		.join()
		.unwrap();
	assert_eq!(
		script_error("'use strict'; var s = 'abc\\012def';"),
		"Octal literal in strict mode (26)"
	);
}

#[test]
fn svelte_entry_points() {
	let options = Options {
		module: true,
		..Options::default()
	};
	let (ast, id, _) = parse_pattern_at("{#each items as {a, b = 1}, i}", 16, options).unwrap();
	assert_eq!(
		dump(&ast, id, &plain),
		r#"ObjectPattern { properties: [Property { key: Identifier { name: "a" }, value: Identifier { name: "a" }, kind: Init, computed: false, method: false, shorthand: true }, Property { key: Identifier { name: "b" }, value: AssignmentPattern { left: Identifier { name: "b" }, right: NumberLiteral { value: 1.0 } }, kind: Init, computed: false, method: false, shorthand: true }] }"#
	);
	assert_eq!(ast.node(id).end, 26);
	let (ast, id, _) = parse_pattern_at("{#each items as item (item.id)}", 16, options).unwrap();
	assert_eq!(dump(&ast, id, &plain), r#"Identifier { name: "item" }"#);
	assert_eq!(ast.node(id).end, 20);
	assert!(parse_pattern_at("{#each items as 1}", 16, options).is_err());

	let (ast, params, end) = parse_params_at("{#snippet row(a, {b}, ...rest)}", 13, options).unwrap();
	let dumped: Vec<String> = params.iter().map(|p| dump(&ast, *p, &plain)).collect();
	assert_eq!(
		dumped,
		[
			r#"Identifier { name: "a" }"#,
			r#"ObjectPattern { properties: [Property { key: Identifier { name: "b" }, value: Identifier { name: "b" }, kind: Init, computed: false, method: false, shorthand: true }] }"#,
			r#"RestElement { argument: Identifier { name: "rest" } }"#
		]
	);
	assert_eq!(end, 30);
	assert_eq!(
		parse_params_at("{#snippet row(a, a)}", 13, options)
			.unwrap_err()
			.message,
		"Argument name clash"
	);
	// Parameters are read as expressions first, so the errors are the ones acorn gives an arrow.
	let params_error = |src: &str| {
		let e = parse_params_at(src, 13, options).unwrap_err();
		(e.message, e.pos)
	};
	assert_eq!(params_error("{#snippet row(a.b)}"), ("Assigning to rvalue".into(), 14));
	assert_eq!(
		params_error("{#snippet row((a))}"),
		("Parenthesized pattern".into(), 14)
	);
	assert_eq!(
		params_error("{#snippet row(a = await x)}"),
		("Await expression cannot be a default value".into(), 18)
	);
	let deep = format!("({}a{})", "[".repeat(20_000), "]".repeat(20_000));
	assert!(parse_params_at(&deep, 0, options).is_err());

	let (ast, id, _) = parse_statement_at("{@const x = a + 1}", 2, options).unwrap();
	assert_eq!(
		dump(&ast, id, &plain),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(BinaryExpression { operator: Add, left: Identifier { name: "a" }, right: NumberLiteral { value: 1.0 } }) }], kind: Const }"#
	);
	assert_eq!(ast.node(id).end, 17);
}

#[test]
fn undeclared_exports_can_be_allowed() {
	let options = Options {
		module: true,
		allow_undeclared_exports: true,
		..Options::default()
	};
	assert!(parse("export { nope };", options).is_ok());
}

// TEASEL_BENCH=file cargo test --release phases -- --ignored --nocapture
#[test]
#[ignore]
fn phases() {
	use crate::estree::{Binary, Json, Output, Positions, program};
	use crate::lexer::Lexer;
	use crate::lexer::token::TokenKind;
	let Ok(path) = std::env::var("TEASEL_BENCH") else {
		return;
	};
	let source = std::fs::read_to_string(path).unwrap();
	let options = crate::Options {
		module: true,
		..Default::default()
	};
	for _ in 0..300 {
		let _ = crate::parser::parse_range::<()>(&source, 0, source.len() as u32, options).unwrap();
	}
	let mut best = |name: &str, f: &mut dyn FnMut()| {
		let mut m = f64::MAX;
		for _ in 0..300 {
			let t = std::time::Instant::now();
			f();
			m = m.min(t.elapsed().as_secs_f64() * 1e3);
		}
		eprintln!("{m:7.3} ms  {name}");
	};
	let mut tokens = 0;
	let mut reached = 0;
	best("lex every token", &mut || {
		let mut lexer = Lexer::new(&source);
		lexer.module = true;
		tokens = 0;
		while let Ok(token) = lexer.next_token() {
			tokens += 1;
			if token.kind == TokenKind::Eof {
				break;
			}
		}
		reached = lexer.pos();
	});
	eprintln!("          {tokens} tokens, lexer reached {reached} of {}", source.len());
	best("parse", &mut || {
		let _ = crate::parser::parse_range::<()>(&source, 0, source.len() as u32, options).unwrap();
	});
	let mean = |name: &str, f: &mut dyn FnMut()| {
		let t = std::time::Instant::now();
		for _ in 0..300 {
			f();
		}
		eprintln!(
			"{:7.3} ms  {name} (mean of 300)",
			t.elapsed().as_secs_f64() * 1e3 / 300.0
		);
	};
	mean("parse", &mut || {
		let _ = crate::parser::parse_range::<()>(&source, 0, source.len() as u32, options).unwrap();
	});
	best("parse + attach comments", &mut || {
		let mut ast = crate::parser::parse_range::<()>(&source, 0, source.len() as u32, options).unwrap();
		let root = ast.last();
		crate::comments::attach(&mut ast, &source, root, 0);
	});
	best("parse + scopes", &mut || {
		let mut ast = crate::parser::parse_range::<()>(&source, 0, source.len() as u32, options).unwrap();
		let root = ast.last();
		crate::scopes::analyze(&mut ast, root);
	});
	best("Positions::new, no lines", &mut || {
		let _ = Positions::new(&source, false);
	});
	best("Positions::new, lines", &mut || {
		let _ = Positions::new(&source, true);
	});
	let mut ast = crate::parser::parse_range::<()>(&source, 0, source.len() as u32, options).unwrap();
	let root = ast.last();
	crate::comments::attach(&mut ast, &source, root, 0);
	let output = Output {
		comments: true,
		scopes: false,
		pattern: false,
		erase: false,
	};
	let flat = Positions::new(&source, false);
	let lines = Positions::new(&source, true);
	best("Binary encode, no loc", &mut || {
		let _ = program(&ast, root, &source, &flat, output, Binary::new()).finish();
	});
	best("Binary encode, loc", &mut || {
		let _ = program(&ast, root, &source, &lines, output, Binary::new()).finish();
	});
	best("Json write, loc", &mut || {
		let _ = program(&ast, root, &source, &lines, output, Json::default()).finish();
	});
	eprintln!(
		"nodes {} lists {} strings {} comments {}",
		ast.nodes.len(),
		ast.lists.len(),
		ast.strings.len(),
		ast.comments.len()
	);
}
