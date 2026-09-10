use crate::ast::{Ast, List, NodeId, NodeKind, Walk};

/// How many allocations the test binary made, for the bench.
pub(crate) static ALLOCATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) struct Counting;

unsafe impl std::alloc::GlobalAlloc for Counting {
	unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
		ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		unsafe { std::alloc::System.alloc(layout) }
	}
	unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
		unsafe { std::alloc::System.dealloc(ptr, layout) }
	}
}

use crate::{Code, Entry, Options, SyntaxError};

/// One entry of one extension, as `parse_at` reads it.
pub(crate) type ParseAt<X> =
	fn(&str, u32, Option<u32>, Entry, Options, &str) -> Result<(Ast<X>, List, u32), SyntaxError>;

/// The roots of an answer as a vector.
pub(crate) fn roots<X>(
	result: Result<(Ast<X>, List, u32), SyntaxError>,
) -> Result<(Ast<X>, Vec<NodeId>, u32), SyntaxError> {
	result.map(|(ast, list, end)| {
		let roots = ast.list(list).iter().flatten().copied().collect();
		(ast, roots, end)
	})
}

/// The one root of an answer.
pub(crate) fn one<X>(result: Result<(Ast<X>, List, u32), SyntaxError>) -> Result<(Ast<X>, NodeId, u32), SyntaxError> {
	roots(result).map(|(ast, roots, end)| (ast, roots[0], end))
}

fn at(entry: Entry, src: &str, offset: u32, options: Options, stop: &str) -> Result<(Ast, NodeId, u32), SyntaxError> {
	one(crate::parse_at(src, offset, None, entry, options, stop))
}

fn params(src: &str, offset: u32, options: Options, stop: &str) -> Result<(Ast, Vec<NodeId>, u32), SyntaxError> {
	roots(crate::parse_at(src, offset, None, Entry::Params, options, stop))
}

fn program(src: &str, options: Options) -> Result<Ast, SyntaxError> {
	crate::parse_at(src, 0, None, Entry::Program, options, "").map(|(ast, _, _)| ast)
}

/// A whole source as a program, with the list holding its root.
fn whole(src: &str, options: Options) -> (Ast, List) {
	let (ast, roots, _) = crate::parse_at(src, 0, None, Entry::Program, options, "").unwrap();
	(ast, roots)
}

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
	let (ast, id, _) = at(Entry::Expression, src, 0, Options::default(), "").unwrap_or_else(|e| panic!("{src}: {e}"));
	dump(&ast, id, &plain)
}

fn module(src: &str) -> String {
	let ast = program(
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
	let ast = program(src, Options::default()).unwrap_or_else(|e| panic!("{src}: {e}"));
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
	match program(
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
	let end = |src: &str, offset: u32| at(Entry::Expression, src, offset, options, "").unwrap().2;
	assert_eq!(end("{a /* c */ }", 1), 10);
	assert_eq!(end("{(a) }", 1), 4);
	assert_eq!(end("{a} /* c */", 1), 2);
	assert_eq!(at(Entry::Statement, "{@const x = 1}", 2, options, "").unwrap().2, 13);
	assert_eq!(params("{#snippet s(a) /* c */}", 11, options, "").unwrap().2, 22);
}

fn span(src: &str) -> (u32, u32) {
	let (ast, id, _) = at(Entry::Expression, src, 0, Options::default(), "").unwrap();
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
	let (ast, id, _) = at(Entry::Expression, "{ a.b }", 2, Options::default(), "").unwrap();
	assert_eq!((ast.node(id).start, ast.node(id).end), (2, 5));
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
fn reserved_words_by_context() {
	use crate::Code;
	let code = |src: &str, module: bool| {
		program(
			src,
			Options {
				module,
				..Options::default()
			},
		)
		.err()
		.map(|e| e.code)
	};
	assert_eq!(
		code("function* g() { var yield; }", false),
		Some(Code::YieldAsIdentifier)
	);
	assert_eq!(
		code("async function f() { var await; }", false),
		Some(Code::AwaitAsIdentifier)
	);
	assert_eq!(
		code("class C { x = arguments; }", false),
		Some(Code::ArgumentsInFieldInitializer)
	);
	assert_eq!(
		code("class C { static { await; } }", false),
		Some(Code::InvalidInStaticBlock)
	);
	assert_eq!(code("var \\u0069f;", false), Some(Code::EscapeInKeyword));
	assert_eq!(code("var enum;", false), Some(Code::ReservedWord));
	assert_eq!(code("var \\u0065num;", false), Some(Code::ReservedWord));
	assert_eq!(code("var let;", true), Some(Code::ReservedWord));
	assert_eq!(code("var static;", true), Some(Code::ReservedWord));
	assert_eq!(code("var await;", true), Some(Code::AwaitOutsideAsync));
	assert_eq!(code("'use strict'; eval = 1;", false), Some(Code::StrictBinding));
	assert_eq!(
		code("var yield; var let; var static; var await; eval = 1;", false),
		None
	);
	assert_eq!(code("var implements;", false), None);
}

#[test]
fn a_var_may_redeclare_a_simple_catch_parameter() {
	assert!(module("try {} catch (e) { var b; var e; }").contains("VariableDeclaration"));
	assert_eq!(
		module_error("try {} catch (e) { let e; }"),
		"Identifier 'e' has already been declared (23)"
	);
}

#[test]
fn a_recycled_sink_writes_the_same_words() {
	use crate::json::{Prepared, Request};
	let mut request = Request::new(Entry::Program, 0);
	for flag in ["comments", "locations", "scopes"] {
		request.set(flag);
	}
	let source = "let x = /* a */ 1; function f(y) { return x + y; } // b";
	let prepared = Prepared::borrowed(source, request);
	let first = prepared.binary(Entry::Program, 0.0, None, "").unwrap();
	let _ = Prepared::borrowed("a + b", request).binary(Entry::Expression, 0.0, None, "");
	assert!(
		Prepared::borrowed("a +", request)
			.binary(Entry::Expression, 0.0, None, "")
			.is_err()
	);
	let again = prepared.binary(Entry::Program, 0.0, None, "").unwrap();
	// the header counts the constants and shapes known so far, which the parses between added to
	assert_eq!((&first[..4], &first[6..]), (&again[..4], &again[6..]));
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
	match program(src, Options::default()) {
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
	assert!(program("with (x) {}", Options::default()).is_ok());
	assert!(
		program(
			"export {a}; { var a; }",
			Options {
				module: true,
				..Options::default()
			}
		)
		.is_ok()
	);
	assert!(
		program(
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
	assert!(program("({ \\u0069f: 1 }); x.\\u0074his; ({ i\\u0066: 1 })", Options::default()).is_ok());
	assert_eq!(script_error("\\u0069f (x) {}"), "Escape sequence in keyword if (0)");
	assert_eq!(
		script_error("import /* unterminated"),
		"'import' and 'export' may appear only with 'sourceType: module' (0)"
	);
	assert_eq!(script_error("if (x) let \\u0061 = 1"), "Unexpected token (7)");
	assert!(program("function f(){ \u{2000}'use strict'; with(x){} }", Options::default()).is_err());
	assert_eq!(
		module_error("async function f(){ for await (-x of y); }"),
		"Unexpected token (31)"
	);
	std::thread::Builder::new()
		.stack_size(64 << 20)
		.spawn(|| {
			let deep = format!("{}1{}", "(".repeat(2000), ")".repeat(2000));
			assert_eq!(module_error(&deep).as_str(), "Maximum nesting depth exceeded (499)");
			assert!(program(&format!("{}1{}", "(".repeat(400), ")".repeat(400)), Options::default()).is_ok());
			let chain = format!("x = 1{}", " + 1".repeat(9000));
			assert!(program(&chain, Options::default()).is_ok());
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
	let (ast, id, _) = at(Entry::Pattern, "{#each items as {a, b = 1}, i}", 16, options, "").unwrap();
	assert_eq!(
		dump(&ast, id, &plain),
		r#"ObjectPattern { properties: [Property { key: Identifier { name: "a" }, value: Identifier { name: "a" }, kind: Init, computed: false, method: false, shorthand: true }, Property { key: Identifier { name: "b" }, value: AssignmentPattern { left: Identifier { name: "b" }, right: NumberLiteral { value: 1.0 } }, kind: Init, computed: false, method: false, shorthand: true }] }"#
	);
	assert_eq!(ast.node(id).end, 26);
	let (ast, id, _) = at(Entry::Pattern, "{#each items as item (item.id)}", 16, options, "").unwrap();
	assert_eq!(dump(&ast, id, &plain), r#"Identifier { name: "item" }"#);
	assert_eq!(ast.node(id).end, 20);
	assert!(at(Entry::Pattern, "{#each items as 1}", 16, options, "").is_err());

	let (ast, list, end) = params("{#snippet row(a, {b}, ...rest)}", 13, options, "").unwrap();
	let dumped: Vec<String> = list.iter().map(|p| dump(&ast, *p, &plain)).collect();
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
		params("{#snippet row(a, a)}", 13, options, "").unwrap_err().message,
		"Argument name clash"
	);
	// Parameters are read as expressions first, so the errors are the ones acorn gives an arrow.
	let params_error = |src: &str| {
		let e = params(src, 13, options, "").unwrap_err();
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
	// The nesting limit must fail before the stack does, on wasm's 1 MB; debug frames are far
	// bigger, so the check gets a stack to match there.
	let stack = if cfg!(debug_assertions) { 16 << 20 } else { 1 << 20 };
	std::thread::Builder::new()
		.stack_size(stack)
		.spawn(move || {
			let deep = format!("({}a{})", "[".repeat(20_000), "]".repeat(20_000));
			assert_eq!(params(&deep, 0, options, "").unwrap_err().code, Code::NestingDepth);
		})
		.unwrap()
		.join()
		.unwrap();

	let (ast, id, _) = at(Entry::Statement, "{@const x = a + 1}", 2, options, "").unwrap();
	assert_eq!(
		dump(&ast, id, &plain),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(BinaryExpression { operator: Add, left: Identifier { name: "a" }, right: NumberLiteral { value: 1.0 } }) }], kind: Const }"#
	);
	assert_eq!(ast.node(id).end, 17);
}

/// One extension's parse, with the checks tests make of recovery.
struct Api<X>(ParseAt<X>);

const JS: Api<()> = Api(crate::parse_at);
const TS: Api<crate::typescript::ast::Data> = Api(crate::typescript::parse_at);

impl<X: Walk> Api<X> {
	fn run(
		&self,
		src: &str,
		offset: u32,
		entry: Entry,
		options: Options,
		stop: &str,
	) -> Result<(Ast<X>, Vec<NodeId>, u32), SyntaxError> {
		roots((self.0)(src, offset, None, entry, options, stop))
	}

	/// Parses under recovery and checks what the recovered tree promises: it never fails, its
	/// roots end within what was consumed, a parent contains its children, a placeholder has no
	/// width, and there are errors exactly when the strict parse throws.
	fn recovered(
		&self,
		src: &str,
		offset: u32,
		entry: Entry,
		options: Options,
		stop: &str,
	) -> (Ast<X>, Vec<NodeId>, u32) {
		let throws = self.run(src, offset, entry, options, stop).is_err();
		let recovering = Options {
			error_recovery: true,
			..options
		};
		let (ast, roots, end) = self
			.run(src, offset, entry, recovering, stop)
			.unwrap_or_else(|e| panic!("{src:?}: {e}"));
		assert_eq!(!ast.errors.is_empty(), throws, "{src:?}: errors {:?}", ast.errors);
		let mut stack = roots.clone();
		let mut children = Vec::new();
		for &root in &roots {
			assert!(
				ast.node(root).end <= end,
				"{src:?}: root ends at {} after {end}",
				ast.node(root).end
			);
		}
		while let Some(id) = stack.pop() {
			let node = ast.node(id);
			assert!(
				node.start <= node.end && node.end as usize <= src.len(),
				"{src:?}: {:?} at {}..{}",
				node.kind,
				node.start,
				node.end
			);
			if let NodeKind::Identifier { name } = node.kind
				&& ast.str(name).is_empty()
			{
				assert_eq!(node.start, node.end, "{src:?}: a placeholder with width");
			}
			children.clear();
			ast.children(id, &mut children);
			for &child in &children {
				let c = ast.node(child);
				assert!(
					node.start <= c.start && c.end <= node.end,
					"{src:?}: {:?} at {}..{} outside {:?} at {}..{}",
					c.kind,
					c.start,
					c.end,
					node.kind,
					node.start,
					node.end
				);
			}
			stack.extend_from_slice(&children);
		}
		(ast, roots, end)
	}
}

#[test]
fn recovery() {
	let module = Options {
		module: true,
		..Options::default()
	};
	fn codes<X>(ast: &Ast<X>) -> Vec<String> {
		ast.errors
			.iter()
			.map(|e| format!("{}@{}", e.code.name(), e.pos))
			.collect()
	}
	let expr = |src: &str, stop: &str| {
		let (ast, roots, end) = JS.recovered(src, 1, Entry::Expression, module, stop);
		(dump(&ast, roots[0], &plain), end, codes(&ast))
	};

	// a missing operand or name: a placeholder where the token was expected
	assert_eq!(
		expr("{x.}", "}"),
		(r#"MemberExpression { object: Identifier { name: "x" }, property: Identifier { name: "" }, computed: false, optional: false }"#.into(), 3, vec!["unexpected_token@3".into()])
	);
	// a stop word after `.` is a property name: the host's syntax cannot start there
	assert_eq!(
		expr("{obj. as item}", "as"),
		(r#"MemberExpression { object: Identifier { name: "obj" }, property: Identifier { name: "as" }, computed: false, optional: false }"#.into(), 8, vec![])
	);
	assert_eq!(
		expr("{a + }", "}"),
		(
			r#"BinaryExpression { operator: Add, left: Identifier { name: "a" }, right: Identifier { name: "" } }"#
				.into(),
			5,
			vec!["unexpected_token@5".into()],
		)
	);
	// a missing closer: the expression fails, a placeholder stands for it, the host's token kept
	assert_eq!(
		expr("{f(a, }", "}"),
		(
			r#"Identifier { name: "" }"#.into(),
			6,
			vec!["unexpected_token@6".into()]
		)
	);
	assert_eq!(
		expr("{[f(a, ]}", "}"),
		(
			r#"Identifier { name: "" }"#.into(),
			8,
			vec!["unexpected_token@7".into()]
		)
	);
	// nothing wrong: the strict tree, no errors
	assert_eq!(expr("{a + b}", "}"), (super::tests::expr("a + b"), 6, vec![]));

	// the host's declaration tag: `let` is a declaration in strict code, its name a placeholder
	let (ast, roots, end) = JS.recovered("{let }", 1, Entry::Statement, module, "}");
	assert_eq!(
		dump(&ast, roots[0], &plain),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "" }, init: None }], kind: Let }"#
	);
	assert_eq!((end, codes(&ast)), (5, vec!["unexpected_token@5".into()]));
	let (ast, roots, _) = JS.recovered("{const x}", 1, Entry::Statement, module, "}");
	assert_eq!(
		dump(&ast, roots[0], &plain),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: None }], kind: Const }"#
	);
	assert_eq!(codes(&ast), ["unexpected_token@8"]);

	// unterminated tokens run to the end of their line or of the source
	let (ast, _, _) = JS.recovered("x = 'abc\ny = 1", 0, Entry::Program, module, "");
	assert_eq!(codes(&ast), ["unterminated_string@4"]);
	assert_eq!(
		script_body(&ast),
		[
			r#"ExpressionStatement { expression: AssignmentExpression { operator: Assign, left: Identifier { name: "x" }, right: StringLiteral { value: "abc" } }, directive: None }"#,
			r#"ExpressionStatement { expression: AssignmentExpression { operator: Assign, left: Identifier { name: "y" }, right: NumberLiteral { value: 1.0 } }, directive: None }"#
		]
	);
	let (ast, _, _) = JS.recovered("f(`abc${x} ", 0, Entry::Program, module, "");
	assert_eq!(codes(&ast), ["unterminated_template@10", "unexpected_eof@11"]);
	let (ast, _, _) = JS.recovered("/* c", 0, Entry::Program, module, "");
	assert_eq!(codes(&ast), ["unterminated_comment@0"]);
	assert_eq!(ast.comments.len(), 1);

	// a missing semicolon is recorded and the statement kept; a statement that fails is skipped
	// to `;`, the closing brace or a keyword on a new line
	let (ast, _, _) = JS.recovered(
		"let x = ;\nfoo bar baz;\nlet y = (\nz = 1",
		0,
		Entry::Program,
		module,
		"",
	);
	assert_eq!(
		codes(&ast),
		[
			"unexpected_token@8",
			"unexpected_token@14",
			"unexpected_token@18",
			"unexpected_eof@38"
		]
	);
	assert_eq!(script_body(&ast).len(), 4);
	let (ast, _, _) = JS.recovered("f(;\nx = 1;\nlet y = )\nreturn", 0, Entry::Program, module, "");
	assert_eq!(
		codes(&ast),
		[
			"unexpected_token@2",
			"unexpected_token@19",
			"return_outside_function@21"
		]
	);
	let body = script_body(&ast);
	assert_eq!(body.len(), 2);
	assert!(body[1].starts_with("VariableDeclaration"));
	// an unclosed body fails with the statement around it, which is skipped
	let (ast, _, _) = JS.recovered("class A { m() { if (x) { y", 0, Entry::Program, module, "");
	// a token no element starts, at the end: the body must not spin on it
	JS.recovered("class A {\n/", 0, Entry::Program, module, "");
	TS.recovered("namespace N {\n/", 0, Entry::Program, module, "");
	TS.recovered("interface I {\n/", 0, Entry::Program, module, "");
	TS.recovered("enum E {\n/", 0, Entry::Program, module, "");
	assert_eq!(codes(&ast), ["unexpected_eof@26"]);
	// a closer nothing opened is reported and skipped
	let (ast, _, _) = JS.recovered("x; ) y", 0, Entry::Program, module, "");
	assert_eq!(codes(&ast), ["unexpected_token@3"]);
	assert_eq!(script_body(&ast).len(), 2);

	// entries: what follows the root is the host's, as in strict mode; a parameter list that fails is empty
	let (ast, roots, end) = JS.recovered("{a b}", 1, Entry::Expression, module, "}");
	assert_eq!(
		(dump(&ast, roots[0], &plain).as_str(), end, codes(&ast)),
		(r#"Identifier { name: "a" }"#, 2, vec![])
	);
	let (ast, roots, end) = JS.recovered("(a, , b)", 0, Entry::Params, module, "");
	assert_eq!(roots.len(), 3);
	assert_eq!((end, codes(&ast)), (8, vec!["unexpected_token@4".into()]));
	let (ast, roots, end) = JS.recovered("{#each xs as [a, }", 13, Entry::Pattern, module, "}");
	assert_eq!(dump(&ast, roots[0], &plain), r#"Identifier { name: "" }"#);
	assert_eq!((end, codes(&ast)), (17, vec!["unexpected_token@17".into()]));

	// a placeholder is neither a binding nor a reference
	let (mut ast, roots, _) = JS.recovered("let = f(a, b)", 0, Entry::Program, module, "");
	let roots = ast.add_list(&[Some(roots[0])]);
	crate::scopes::analyze(&mut ast, Entry::Program, roots);
	let scopes = ast.scopes.as_ref().unwrap();
	assert_eq!(scopes.bindings.len(), 0);
	assert_eq!(
		scopes
			.references
			.iter()
			.map(|r| ast.node(r.node).start)
			.collect::<Vec<_>>(),
		[6, 8, 11]
	);

	// TypeScript: a speculation fails as in strict mode, so `g<A, B` is a sequence; a missing type
	// is a placeholder
	let (ast, _, end) = TS.recovered("{g<A, B }", 1, Entry::Expression, module, "}");
	assert_eq!((end, codes(&ast)), (7, vec![]));
	let (ast, _, _) = TS.recovered("let x: = 1", 0, Entry::Program, module, "");
	assert_eq!(codes(&ast), ["unexpected_token@7"]);
	assert_eq!(ast.node(ast.last()).end, 10);
}

fn script_body(ast: &Ast) -> Vec<String> {
	let NodeKind::Program { body, .. } = ast.node(ast.last()).kind else {
		panic!()
	};
	ast.list(body).iter().map(|s| dump(ast, s.unwrap(), &plain)).collect()
}

/// Every prefix of every JavaScript and TypeScript file of the Svelte test suite, as a program
/// and as an expression, parses under recovery with the invariants `Api::recovered` checks.
#[test]
#[ignore]
fn recovery_prefixes() {
	let root =
		std::env::var("SVELTE_DIR").unwrap_or_else(|_| format!("{}/Projects/svelte", std::env::var("HOME").unwrap()));
	let mut files = Vec::new();
	let mut dirs = vec![std::path::PathBuf::from(format!("{root}/packages/svelte/tests"))];
	while let Some(dir) = dirs.pop() {
		for entry in std::fs::read_dir(dir).unwrap().flatten() {
			let path = entry.path();
			let name = path.file_name().unwrap().to_string_lossy();
			if name == "node_modules" || name.starts_with('.') {
				continue;
			}
			if path.is_dir() {
				dirs.push(path);
			} else if name.ends_with(".js") || name.ends_with(".ts") {
				files.push(path);
			}
		}
	}
	files.sort();
	let module = Options {
		module: true,
		..Options::default()
	};
	let mut prefixes = 0;
	let log = std::env::var("PREFIX_LOG").ok();
	for path in &files {
		let Ok(src) = std::fs::read_to_string(path) else {
			continue;
		};
		if let Some(log) = &log {
			std::fs::write(log, format!("{}\n", path.display())).unwrap();
		}
		let ts = path.to_string_lossy().ends_with(".ts");
		let step = (src.len() / 1000).max(1);
		let mut cut = 0;
		while cut <= src.len() {
			if src.is_char_boundary(cut) {
				let prefix = &src[..cut];
				let outcome = std::panic::catch_unwind(|| {
					for entry in [Entry::Program, Entry::Expression] {
						if ts {
							TS.recovered(prefix, 0, entry, module, "");
						} else {
							JS.recovered(prefix, 0, entry, module, "");
						}
					}
				});
				if outcome.is_err() {
					panic!("{}: prefix of {cut}", path.display());
				}
				prefixes += 1;
			}
			cut += step;
		}
	}
	eprintln!("{} files, {prefixes} prefixes", files.len());
}

#[test]
fn undeclared_exports_can_be_allowed() {
	let options = Options {
		module: true,
		allow_undeclared_exports: true,
		..Options::default()
	};
	assert!(program("export { nope };", options).is_ok());
}

// TEASEL_BENCH=file cargo test --release phases -- --ignored --nocapture
#[test]
#[ignore]
fn phases() {
	use crate::estree::{Binary, Json, Output, Positions, answer};
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
		let _ = whole(&source, options);
	}
	let best = |name: &str, f: &mut dyn FnMut()| {
		let mut m = f64::MAX;
		let mut allocations = 0;
		for _ in 0..300 {
			let before = ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed);
			let t = std::time::Instant::now();
			f();
			m = m.min(t.elapsed().as_secs_f64() * 1e6);
			allocations = ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed) - before;
		}
		eprintln!("{m:9.2} µs  {allocations:4} allocs  {name}");
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
		let _ = whole(&source, options);
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
		let _ = whole(&source, options);
	});
	best("parse + attach comments", &mut || {
		let (mut ast, roots) = whole(&source, options);
		crate::comments::attach(&mut ast, &source, roots, 0);
	});
	best("parse + scopes", &mut || {
		let (mut ast, roots) = whole(&source, options);
		crate::scopes::analyze(&mut ast, Entry::Program, roots);
	});
	{
		let (mut ast, roots) = whole(&source, options);
		crate::comments::attach(&mut ast, &source, roots, 0);
		crate::scopes::analyze(&mut ast, Entry::Program, roots);
		let scoped = Output {
			comments: true,
			scopes: true,
			erase: false,
			errors: false,
		};
		let flat = Positions::new(&source, false);
		let end = source.len() as u32;
		best("Binary encode with scopes", &mut || {
			let _ = answer(&ast, Entry::Program, roots, end, &source, &flat, scoped, Binary::new()).finish();
		});
		let s = ast.scopes.as_ref().unwrap();
		eprintln!(
			"scopes {} bindings {} references {} of_node {} of_identifier {}",
			s.scopes.len(),
			s.bindings.len(),
			s.references.len(),
			s.of_node.iter().count(),
			s.of_identifier.iter().count()
		);
	}
	best("Positions::new, no lines", &mut || {
		let _ = Positions::new(&source, false);
	});
	best("Positions::new, lines", &mut || {
		let _ = Positions::new(&source, true);
	});
	let (mut ast, roots) = whole(&source, options);
	crate::comments::attach(&mut ast, &source, roots, 0);
	let output = Output {
		comments: true,
		scopes: false,
		erase: false,
		errors: false,
	};
	let end = source.len() as u32;
	let flat = Positions::new(&source, false);
	let lines = Positions::new(&source, true);
	{
		let mut request = crate::json::Request::new(Entry::Program, 0);
		request.set("comments");
		let prepared = crate::json::Prepared::borrowed(&source, request);
		best("whole request: positions, parse, comments, encode, finish", &mut || {
			let _ = prepared.binary(Entry::Program, 0.0, None, "").unwrap();
		});
		for flag in ["scopes", "locations"] {
			request.set(flag);
		}
		let prepared = crate::json::Prepared::borrowed(&source, request);
		best("whole request with scopes and loc", &mut || {
			let _ = prepared.binary(Entry::Program, 0.0, None, "").unwrap();
		});
	}
	best("Binary encode, no loc", &mut || {
		let _ = answer(&ast, Entry::Program, roots, end, &source, &flat, output, Binary::new()).finish();
	});
	best("Binary encode, loc", &mut || {
		let _ = answer(&ast, Entry::Program, roots, end, &source, &lines, output, Binary::new()).finish();
	});
	best("Json write, loc", &mut || {
		let _ = answer(
			&ast,
			Entry::Program,
			roots,
			end,
			&source,
			&lines,
			output,
			Json::default(),
		)
		.finish();
	});
	eprintln!(
		"nodes {} lists {} strings {} comments {}",
		ast.nodes.len(),
		ast.lists.len(),
		ast.strings.len(),
		ast.comments.len()
	);
	eprintln!(
		"node {} bytes, token {} bytes",
		std::mem::size_of::<crate::ast::Node>(),
		std::mem::size_of::<crate::lexer::token::Token>()
	);
}

// TEASEL_BENCH=file cargo test --release profile -- --ignored --nocapture; writes target/parse.svg
#[cfg(target_os = "linux")]
#[test]
#[ignore]
fn profile() {
	let Ok(path) = std::env::var("TEASEL_BENCH") else {
		return;
	};
	let source = std::fs::read_to_string(path).unwrap();
	let options = crate::Options {
		module: true,
		..Default::default()
	};
	let guard = pprof::ProfilerGuardBuilder::default()
		.frequency(4000)
		.blocklist(&["libc", "libgcc", "pthread", "vdso"])
		.build()
		.unwrap();
	let mut sink = 0usize;
	if std::env::var("TEASEL_PROFILE").is_ok_and(|what| what == "encode") {
		use crate::estree::{Binary, Output, Positions, answer};
		let (mut ast, roots) = whole(&source, options);
		crate::comments::attach(&mut ast, &source, roots, 0);
		let output = Output {
			comments: true,
			scopes: false,
			erase: false,
			errors: false,
		};
		let positions = Positions::new(&source, false);
		let end = source.len() as u32;
		for _ in 0..30000 {
			let words = answer(
				&ast,
				Entry::Program,
				roots,
				end,
				&source,
				&positions,
				output,
				Binary::new(),
			)
			.finish();
			sink = sink.wrapping_add(words.len());
		}
	} else {
		for _ in 0..3000 {
			let (ast, _) = whole(&source, options);
			sink = sink.wrapping_add(ast.nodes.len());
		}
	}
	let report = guard.report().build().unwrap();
	let file = std::fs::File::create("target/parse.svg").unwrap();
	report.flamegraph(file).unwrap();
	let mut by_frame: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
	let mut total = 0usize;
	for (frames, count) in &report.data {
		total += *count as usize;
		let names: Vec<String> = frames
			.frames
			.iter()
			.flat_map(|f| {
				f.iter().map(|s| {
					s.name
						.as_deref()
						.map(|n| rustc_demangle::demangle(&String::from_utf8_lossy(n)).to_string())
						.unwrap_or_default()
				})
			})
			.collect();
		if let Some(top) = names.first() {
			by_frame.entry(top.clone()).or_default().0 += *count as usize;
		}
		let mut seen = std::collections::HashSet::new();
		for name in &names {
			if seen.insert(name.clone()) {
				by_frame.entry(name.clone()).or_default().1 += *count as usize;
			}
		}
	}
	let mut rows: Vec<_> = by_frame.into_iter().collect();
	rows.sort_by(|a, b| b.1.0.cmp(&a.1.0));
	eprintln!(
		"samples {total}, sink {sink}, token {} bytes, result {} bytes",
		std::mem::size_of::<crate::lexer::token::Token>(),
		std::mem::size_of::<std::result::Result<crate::lexer::token::Token, Box<crate::error::SyntaxError>>>()
	);
	eprintln!("{:>6} {:>6}  frame", "self%", "incl%");
	for (name, (own, incl)) in rows.iter().take(8) {
		eprintln!(
			"{:6.1} {:6.1}  {}",
			*own as f64 * 100.0 / total as f64,
			*incl as f64 * 100.0 / total as f64,
			name.replace("teasel::", "")
				.replace("core::", "")
				.replace("alloc::", "")
				.chars()
				.take(400)
				.collect::<String>()
		);
	}
}

#[test]
fn parenthesized_fact() {
	let options = Options {
		parenthesized: true,
		..Options::default()
	};
	let marked = |src: &str| {
		let (ast, id, _) = at(Entry::Expression, src, 0, options, "").unwrap();
		ast.parenthesized.contains(&id)
	};
	assert!(marked("(a, b)"));
	assert!(marked("((a))"));
	assert!(!marked("a, b"));
	assert!(!marked("(a) => a"));
	let (ast, _, _) = at(Entry::Expression, "(a, b)", 0, Options::default(), "").unwrap();
	assert!(ast.parenthesized.is_empty());
}
