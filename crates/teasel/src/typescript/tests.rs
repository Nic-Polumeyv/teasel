use super::ast::Extras;
use crate::ast::{Ast, List, NodeId, NodeKind};
use crate::layout::Ty;
use crate::parser::tests::{Failed, dump as dump_with, expand, one, typescript};
use crate::parser::{Entry, Options};

fn at(entry: Entry, src: &str, offset: u32, options: Options, stop: &str) -> Result<(Ast, NodeId, u32), Failed> {
	one(crate::parse_at(src, offset, None, entry, typescript(options), stop))
}

fn extension(ast: &Ast, id: NodeId, index: u32) -> String {
	let mut out = expand(ast, &format!("{:?}", ast.extension.kind(index)), &extension);
	out.push_str(&extras(ast, id));
	out
}

/// The keys TypeScript added to a node, after its own rendering.
fn extras(ast: &Ast, id: NodeId) -> String {
	let Some(e) = ast.extension.extras(id) else {
		return String::new();
	};
	let base = e as *const Extras as *const u8;
	let parts: Vec<String> = Extras::FIELDS
		.iter()
		.filter_map(|field| unsafe {
			let (at, name) = (base.add(field.at), field.name);
			match field.ty {
				Ty::Bool => at.cast::<bool>().read().then(|| name.to_owned()),
				Ty::OptNode => at
					.cast::<Option<NodeId>>()
					.read()
					.map(|id| format!("{name}: {}", dump(ast, id))),
				Ty::OptList => at
					.cast::<Option<List>>()
					.read()
					.map(|list| format!("{name}: {}", expand(ast, &format!("{list:?}"), &extension))),
				Ty::OptEnum(names) => names
					.get(usize::from(at.read()))
					.map(|value| format!("{name}: {value}")),
				_ => unreachable!(),
			}
		})
		.collect();
	if parts.is_empty() {
		return String::new();
	}
	format!(" +{{{}}}", parts.join(", "))
}

fn dump(ast: &Ast, id: NodeId) -> String {
	let mut out = dump_with(ast, id, &extension);
	if !matches!(ast.node(id).kind, NodeKind::Extension(_)) {
		out.push_str(&extras(ast, id));
	}
	out
}

fn expr(src: &str) -> String {
	let options = Options(Options::MODULE);
	match at(Entry::Expression, src, 0, options, "") {
		Ok((ast, id, _)) => dump(&ast, id),
		Err(e) => format!("error {}: {}", e.pos, e.text),
	}
}

#[test]
fn expression_entry_point() {
	assert_eq!(
		expr(r#"(a: number, b?: string) => a"#),
		r#"ArrowFunctionExpression { params: [Identifier { name: "a" }, Identifier { name: "b" }], body: Identifier { name: "a" }, expression: true, is_async: false }"#
	);
	assert_eq!(
		expr(r#"a as B<C>"#),
		r#"AsExpression { expression: Identifier { name: "a" }, type_annotation: TypeReference { type_name: Identifier { name: "B" }, type_arguments: Some(TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "C" }, type_arguments: None }] }) } }"#
	);
	assert_eq!(
		expr(r#"x!"#),
		r#"NonNullExpression { expression: Identifier { name: "x" } }"#
	);
	assert_eq!(
		expr(r#"<T>(x: T) => x"#),
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: false } +{type_parameters: TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: None, default: None, is_in: false, is_out: false, is_const: false }] }}"#
	);
	assert_eq!(
		expr(r#"async <T>(x: T) => x"#),
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: true } +{type_parameters: TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: None, default: None, is_in: false, is_out: false, is_const: false }] }}"#
	);
	assert_eq!(
		expr(r#"(x?: number) => x"#),
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: false }"#
	);
	assert_eq!(
		expr(r#"({ a, b }: T) => a"#),
		r#"ArrowFunctionExpression { params: [ObjectPattern { properties: [Property { key: Identifier { name: "a" }, value: Identifier { name: "a" }, kind: Init, computed: false, method: false, shorthand: true }, Property { key: Identifier { name: "b" }, value: Identifier { name: "b" }, kind: Init, computed: false, method: false, shorthand: true }] }], body: Identifier { name: "a" }, expression: true, is_async: false }"#
	);
	assert_eq!(
		expr(r#"(...args: number[]) => args"#),
		r#"ArrowFunctionExpression { params: [RestElement { argument: Identifier { name: "args" } }], body: Identifier { name: "args" }, expression: true, is_async: false }"#
	);
	assert_eq!(
		expr(r#"f<T>(x)"#),
		r#"CallExpression { callee: Identifier { name: "f" }, arguments: [Identifier { name: "x" }], optional: false } +{type_arguments: TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }] }}"#
	);
	assert_eq!(
		expr(r#"a < b"#),
		r#"BinaryExpression { operator: Lt, left: Identifier { name: "a" }, right: Identifier { name: "b" } }"#
	);
	assert_eq!(
		expr(r#"a<b>(c)"#),
		r#"CallExpression { callee: Identifier { name: "a" }, arguments: [Identifier { name: "c" }], optional: false } +{type_arguments: TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "b" }, type_arguments: None }] }}"#
	);
	assert_eq!(
		expr(r#"class { }"#),
		r#"ClassExpression { class: Class { id: None, super_class: None, body: ClassBody { body: [] } } }"#
	);
	assert_eq!(
		expr(r#"(a?) => a"#),
		r#"ArrowFunctionExpression { params: [Identifier { name: "a" }], body: Identifier { name: "a" }, expression: true, is_async: false }"#
	);
}

#[test]
fn stop_at() {
	let options = Options(Options::MODULE);
	let end = |src: &str, stop: &str| at(Entry::Expression, src, 1, options, stop).unwrap().2;
	assert_eq!(end("{xs as item}", "as"), 3);
	assert_eq!(end("{xs as item, i (item.id)}", "as"), 3);
	assert_eq!(end("{xs as [a, b = 1]}", "as"), 3);
	// an assertion before the host's `as` is read as such: the host's is the last of the run
	assert_eq!(end("{xs as T[] as item}", "as"), 10);
	assert_eq!(end("{xs.ys as const as item}", "as"), 15);
	assert_eq!(end("{xs as T[] as item, i}", "as ,"), 10);
	assert_eq!(end("{xs as [''] as item: string, i (item)}", "as ,"), 11);
	assert_eq!(end("{p.then(f) then r}", "then catch"), 10);
	assert_eq!(end("{x.as as y}", "as"), 5);
	assert_eq!(end("{f(x as T) as item}", "as"), 10);
	assert_eq!(end("{(xs as T) as item}", "as"), 10);
	assert_eq!(end("{f<A, B>(), i}", "as ,"), 10);
	assert_eq!(end("{<T>x as y}", "as"), 5);
	assert_eq!(end("{new Map<A, B>() as y}", "as ,"), 16);
	assert_eq!(end("{x satisfies A<B, C>, i}", ","), 20);
	assert_eq!(end("{xs!, i}", ","), 4);
	assert_eq!(end("{xs! as T as item}", "as"), 9);
	assert_eq!(end("{obj. as item}", "as"), 8);
}

/// `?` marks an optional parameter, so it needs the arrow that makes the list parameters.
#[test]
fn optional_marker_needs_an_arrow() {
	let options = Options(Options::MODULE);
	let error = |src: &str| {
		let error = at(Entry::Expression, src, 0, options, "").unwrap_err();
		(error.code, error.pos)
	};
	assert_eq!(error("(a, b?)"), (crate::error::Code::UnexpectedToken, 5));
	assert_eq!(error("(a?)"), (crate::error::Code::UnexpectedToken, 2));
	assert_eq!(error("f(a?)"), (crate::error::Code::UnexpectedToken, 4));
	assert!(at(Entry::Expression, "(a, b?) => a", 0, options, "").is_ok());
	assert!(at(Entry::Expression, "f((a?) => a)", 0, options, "").is_ok());
}

#[test]
fn accessor_belongs_to_a_property() {
	let options = Options(Options::MODULE);
	let error = |src: &str| {
		let error = at(Entry::Program, src, 0, options, "").unwrap_err();
		(error.code, error.pos)
	};
	use crate::error::Code;
	assert_eq!(error("class A { accessor get x() {} }"), (Code::AccessorPlacement, 10));
	assert_eq!(error("class A { accessor set x(v) {} }"), (Code::AccessorPlacement, 10));
	assert_eq!(
		error("class A { accessor constructor() {} }"),
		(Code::AccessorPlacement, 10)
	);
	assert_eq!(
		error("class A { static accessor m() {} }"),
		(Code::AccessorPlacement, 10)
	);
	assert_eq!(
		error("class A { accessor readonly x = 1 }"),
		(Code::ConflictingModifiers, 19)
	);
	assert_eq!(
		error("class A { accessor declare x: T }"),
		(Code::ConflictingModifiers, 19)
	);
}

#[test]
fn program_in_a_range() {
	let src = "<script>let a: number = 1;</script>{a}";
	let (ast, root, _) = one(crate::parse_at(
		src,
		8,
		Some(26),
		Entry::Program,
		Options(Options::MODULE | Options::TYPESCRIPT),
		"",
	))
	.unwrap();
	assert_eq!((ast.node(root).start, ast.node(root).end), (8, 26));
	assert_eq!(ast.comments.len(), 0);
	use crate::Options;
	use crate::json::Request;
	let mut request = Request {
		offset: 8,
		..Request::from_flags(Options::MODULE | Options::TYPESCRIPT)
	};
	request.end = Some(1000);
	assert!(crate::json::parse(src, &request).contains("is not a character boundary"));
	request.end = Some(2);
	assert!(crate::json::parse(src, &request).contains("is before"));
	let prepared = crate::json::Prepared::new(src[..26].to_string(), request.options);
	assert!(
		prepared
			.parse(Entry::Program, 8.0, None, "", None)
			.contains("\"end\":26")
	);
	assert!(
		prepared
			.parse(Entry::Program, 22.0, Some(8.0), "", None)
			.contains("is before")
	);
}

/// A host's own syntax may carry a type parameter list, as a generic snippet does.
#[test]
fn type_parameters_entry() {
	let options = Options(Options::MODULE);
	let end = |src: &str| at(Entry::TypeParameters, src, 3, options, "").unwrap().2;
	assert_eq!(end("foo<T extends () => void>(x: T)"), 25);
	assert_eq!(end("foo<T = '>'>()"), 12);
	assert_eq!(end("foo<const T, U extends T[]>"), 27);
	assert_eq!(
		at(Entry::TypeParameters, "foo<>", 3, options, "").unwrap_err().code,
		crate::error::Code::EmptyTypeParameters
	);
	assert_eq!(
		crate::parse_at("foo<T>", 3, None, Entry::TypeParameters, options, "")
			.1
			.unwrap_err()
			.code,
		crate::error::Code::NotTypescript
	);
}

#[test]
fn failed_attempts_leave_nothing() {
	let options = Options(Options::MODULE | Options::PARENTHESIZED);
	// every `<` tries type arguments over the rest of the list
	let list = (0..300).map(|i| format!("a{i} < b{i}")).collect::<Vec<_>>().join(", ");
	let (ast, _, _) = at(Entry::Expression, &format!("[{list}]"), 0, options, "").unwrap();
	assert!(ast.nodes.len() < 4 * 300 + 8, "{} nodes", ast.nodes.len());
	assert!(ast.extension.nodes.is_empty());
	assert!(ast.extension.extras.is_empty());
	// a generic arrow is tried first: the parens it saw go with it
	let (ast, root, _) = at(Entry::Expression, "<T>(a)", 0, options, "").unwrap();
	let NodeKind::Extension(index) = ast.node(root).kind else {
		panic!()
	};
	let super::ast::TsKind::TypeAssertion { expression, .. } = ast.extension.kind(index) else {
		panic!()
	};
	assert!(ast.is_parenthesized(expression));
	assert_eq!(
		(0..ast.nodes.len() as u32)
			.filter(|&i| ast.is_parenthesized(NodeId::at(i)))
			.count(),
		1
	);
}

#[test]
fn failed_attempt_leaves_no_scope() {
	let options = Options(Options::MODULE | Options::ERROR_RECOVERY);
	let src = "let b; <T>(x) => { let a; let a; }; let b;";
	let (ast, _, _) = one(crate::parse_at(src, 0, None, Entry::Program, typescript(options), "")).unwrap();
	let redeclared: Vec<u32> = ast
		.errors
		.iter()
		.filter(|e| e.code == crate::error::Code::Redeclaration)
		.map(|e| e.pos)
		.collect();
	assert_eq!(
		redeclared,
		[src.rfind("a;").unwrap() as u32, src.rfind("b;").unwrap() as u32]
	);
}

#[test]
fn nested_ambiguity_is_linear() {
	let options = Options(Options::MODULE);
	let n = 24;
	let nest = |open: &str, middle: &str, close: &str| format!("x = {}{middle}{};", open.repeat(n), close.repeat(n));
	// each of these took twice as long per level while a failed reading was read again
	for src in [
		nest("<A>(", "x", ")"),
		nest("<A>(a = ", "x", ")"),
		nest("async <T>(a = ", "x", ")"),
		nest("a ? (b) : c => ", "d", ""),
		nest("a ? (b): T => ", "c", " : d"),
		nest("a ? <T>(b) : c => ", "d", ""),
		nest("a ? async (b) : c => ", "d", ""),
	] {
		let started = std::time::Instant::now();
		one(crate::parse_at(&src, 0, None, Entry::Program, typescript(options), "")).unwrap();
		assert!(started.elapsed().as_millis() < 100, "{src}");
	}
}

#[test]
fn a_list_item_fails_where_it_stands() {
	let options = Options(Options::MODULE);
	let error = |src: &str| {
		let error = at(Entry::Program, src, 0, options, "").unwrap_err();
		(error.code, error.pos)
	};
	use crate::error::Code;
	assert_eq!(error("[eval: T = (b: U)];"), (Code::UnexpectedTypeAnnotation, 5));
	assert_eq!(error("f(a as T: U = 1);"), (Code::UnexpectedTypeAnnotation, 8));
	assert_eq!(error("({a = 1} += 2);"), (Code::InvalidAssignmentTarget, 1));
	assert_eq!(error("const g = ({ a }?) => a;"), (Code::OptionalPatternParameter, 11));
}

#[test]
fn an_asserted_cast_is_no_parameter() {
	let options = Options(Options::MODULE);
	let error = |src: &str| at(Entry::Program, src, 0, options, "").unwrap_err().pos;
	assert_eq!(error("(a as T: U = 1) => 1;"), 1);
	assert_eq!(error("(a!: T = 1) => 1;"), 1);
	assert_eq!(error("(...a: T += 1) => 1;"), 9);
}

#[test]
fn an_async_rest_ends_after_its_annotation() {
	let src = "const f = async (first, ...rest: number[]) => rest;";
	let (ast, _, _) = at(Entry::Program, src, 0, Options(Options::MODULE), "").unwrap();
	let rest = (0..ast.nodes.len() as u32)
		.map(NodeId::at)
		.find(|&id| matches!(ast.node(id).kind, NodeKind::RestElement { .. }))
		.unwrap();
	assert_eq!((ast.node(rest).start, ast.node(rest).end), (24, 41));
}

#[test]
fn a_generic_head_checks_its_defaults() {
	let options = Options(Options::MODULE);
	let error = |src: &str| {
		let error = at(Entry::Program, src, 0, options, "").unwrap_err();
		(error.code, error.pos)
	};
	use crate::error::Code;
	assert_eq!(
		error("function* g() { <T>(a = yield) => 1 }"),
		(Code::YieldInDefaultValue, 24)
	);
	assert_eq!(
		error("async function f() { <T>(a = await 1) => 1 }"),
		(Code::AwaitInDefaultValue, 29)
	);
}
