use super::ast::Data;
use crate::SyntaxError;
use crate::ast::{Ast, NodeId, NodeKind};
use crate::parser::tests::{dump as dump_with, expand, one};
use crate::parser::{Entry, Options};

fn at(
	entry: Entry,
	src: &str,
	offset: u32,
	options: Options,
	stop: &str,
) -> Result<(Ast<Data>, NodeId, u32), SyntaxError> {
	one(super::parse_at(src, offset, None, entry, options, stop))
}

fn extension(ast: &Ast<Data>, id: NodeId, index: u32) -> String {
	let mut out = expand(ast, &format!("{:?}", ast.extension.kind(index)), &extension);
	out.push_str(&extras(ast, id));
	out
}

/// The keys TypeScript added to a node, after its own rendering.
fn extras(ast: &Ast<Data>, id: NodeId) -> String {
	let Some(e) = ast.extension.extras(id) else {
		return String::new();
	};
	let mut parts = Vec::new();
	let node = |name: &str, id: Option<NodeId>, parts: &mut Vec<String>| {
		if let Some(id) = id {
			parts.push(format!("{name}: {}", dump(ast, id)));
		}
	};
	node("typeAnnotation", e.type_annotation, &mut parts);
	node("returnType", e.return_type, &mut parts);
	node("typeParameters", e.type_parameters, &mut parts);
	node("typeArguments", e.type_arguments, &mut parts);
	node("superTypeParameters", e.super_type_arguments, &mut parts);
	for (name, list) in [("implements", e.implements), ("decorators", e.decorators)] {
		if let Some(list) = list {
			parts.push(format!("{name}: {}", expand(ast, &format!("{list:?}"), &extension)));
		}
	}
	if let Some(a) = e.accessibility {
		parts.push(format!("accessibility: {}", a.as_str()));
	}
	for (name, kind) in [("importKind", e.import_kind), ("exportKind", e.export_kind)] {
		if let Some(kind) = kind {
			parts.push(format!("{name}: {}", kind.as_str()));
		}
	}
	let flags = [
		("optional", e.optional),
		("definite", e.definite),
		("declare", e.declare),
		("abstract", e.is_abstract),
		("readonly", e.readonly),
		("override", e.is_override),
		("accessor", e.accessor),
		("static", e.is_static),
	];
	parts.extend(flags.iter().filter(|(_, set)| *set).map(|(name, _)| name.to_string()));
	if parts.is_empty() {
		return String::new();
	}
	format!(" +{{{}}}", parts.join(", "))
}

fn dump(ast: &Ast<Data>, id: NodeId) -> String {
	let mut out = dump_with(ast, id, &extension);
	if !matches!(ast.node(id).kind, NodeKind::Extension(_)) {
		out.push_str(&extras(ast, id));
	}
	out
}

fn expr(src: &str) -> String {
	let options = Options {
		module: true,
		..Options::default()
	};
	match at(Entry::Expression, src, 0, options, "") {
		Ok((ast, id, _)) => dump(&ast, id),
		Err(e) => format!("error {}: {}", e.pos, e.message),
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
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: false } +{typeParameters: TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: None, default: None, is_in: false, is_out: false, is_const: false }] }}"#
	);
	assert_eq!(
		expr(r#"async <T>(x: T) => x"#),
		r#"ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: true } +{typeParameters: TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: None, default: None, is_in: false, is_out: false, is_const: false }] }}"#
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
		r#"CallExpression { callee: Identifier { name: "f" }, arguments: [Identifier { name: "x" }], optional: false } +{typeArguments: TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }] }}"#
	);
	assert_eq!(
		expr(r#"a < b"#),
		r#"BinaryExpression { operator: Lt, left: Identifier { name: "a" }, right: Identifier { name: "b" } }"#
	);
	assert_eq!(
		expr(r#"a<b>(c)"#),
		r#"CallExpression { callee: Identifier { name: "a" }, arguments: [Identifier { name: "c" }], optional: false } +{typeArguments: TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "b" }, type_arguments: None }] }}"#
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
	let options = Options {
		module: true,
		..Options::default()
	};
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
	let options = Options {
		module: true,
		..Options::default()
	};
	let error = |src: &str| {
		let error = at(Entry::Expression, src, 0, options, "").unwrap_err();
		(error.code, error.pos)
	};
	assert_eq!(error("(a, b?)"), (crate::error::Code::UnexpectedToken, 5));
	assert_eq!(error("(a?)"), (crate::error::Code::UnexpectedToken, 2));
	assert_eq!(error("f(a?)"), (crate::error::Code::UnexpectedToken, 3));
	assert!(at(Entry::Expression, "(a, b?) => a", 0, options, "").is_ok());
	assert!(at(Entry::Expression, "f((a?) => a)", 0, options, "").is_ok());
}

#[test]
fn program_in_a_range() {
	let src = "<script>let a: number = 1;</script>{a}";
	let (ast, root, _) = one(super::parse_at(
		src,
		8,
		Some(26),
		Entry::Program,
		Options {
			module: true,
			..Options::default()
		},
		"",
	))
	.unwrap();
	assert_eq!((ast.node(root).start, ast.node(root).end), (8, 26));
	assert_eq!(ast.comments.len(), 0);
	let mut request = crate::json::Request::new(Entry::Program, 8);
	request.typescript = true;
	request.end = Some(1000);
	assert!(crate::json::parse(src, &request, "").contains("is not a character boundary"));
	request.end = Some(2);
	assert!(crate::json::parse(src, &request, "").contains("is before"));
	let prepared = crate::json::Prepared::new(src[..26].to_string(), request);
	assert!(prepared.parse(Entry::Program, 8.0, None, "").contains("\"end\":26"));
	assert!(
		prepared
			.parse(Entry::Program, 22.0, Some(8.0), "")
			.contains("is before")
	);
}

/// A host's own syntax may carry a type parameter list, as a generic snippet does.
#[test]
fn type_parameters_entry() {
	let options = Options {
		module: true,
		..Options::default()
	};
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
			.unwrap_err()
			.code,
		crate::error::Code::NotTypeScript
	);
}

#[test]
fn failed_attempts_leave_nothing() {
	let options = Options {
		module: true,
		parenthesized: true,
		..Options::default()
	};
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
	assert_eq!(ast.parenthesized, [expression]);
}

#[test]
fn failed_attempt_leaves_no_scope() {
	let options = Options {
		module: true,
		error_recovery: true,
		..Options::default()
	};
	let src = "let b; <T>(x) => { let a; let a; }; let b;";
	let (ast, _, _) = one(super::parse_at(src, 0, None, Entry::Program, options, "")).unwrap();
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
