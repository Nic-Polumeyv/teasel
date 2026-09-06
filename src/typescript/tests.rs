use super::ast::Data;
use super::{parse, parse_expression_at};
use crate::ast::{Ast, NodeId, NodeKind};
use crate::parser::Options;
use crate::parser::tests::{dump as dump_with, expand};

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

fn module(src: &str) -> String {
	let options = Options {
		module: true,
		..Options::default()
	};
	match parse(src, options) {
		Ok(ast) => {
			let root = ast.last();
			let NodeKind::Program { body, .. } = ast.node(root).kind else {
				unreachable!()
			};
			ast.list(body)
				.iter()
				.map(|s| dump(&ast, s.unwrap()))
				.collect::<Vec<_>>()
				.join("; ")
		}
		Err(e) => format!("error {}: {}", e.pos, e.message),
	}
}

fn expr(src: &str) -> String {
	let options = Options {
		module: true,
		preserve_parens: true,
		..Options::default()
	};
	match parse_expression_at(src, 0, options) {
		Ok((ast, id, _)) => dump(&ast, id),
		Err(e) => format!("error {}: {}", e.pos, e.message),
	}
}

#[test]
fn annotations() {
	assert_eq!(
		module(r#"let x: number = 1;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NumberLiteral { value: 1.0 }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x!: string;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: None }], kind: Let }"#
	);
	assert_eq!(
		module(r#"function f(a?: string, ...rest: number[]): void {}"#),
		r#"FunctionDeclaration { function: Function { id: Some(Identifier { name: "f" }), params: [Identifier { name: "a" }, RestElement { argument: Identifier { name: "rest" } }], body: BlockStatement { body: [] }, is_async: false, generator: false } } +{returnType: TypeAnnotation { type_annotation: Keyword(Void) }}"#
	);
	assert_eq!(
		module(r#"function f<T extends object = {}>(this: Window, x: T): x is T { return true }"#),
		r#"FunctionDeclaration { function: Function { id: Some(Identifier { name: "f" }), params: [Identifier { name: "this" }, Identifier { name: "x" }], body: BlockStatement { body: [ReturnStatement { argument: Some(BooleanLiteral { value: true }) }] }, is_async: false, generator: false } } +{returnType: TypeAnnotation { type_annotation: TypePredicate { parameter_name: Identifier { name: "x" }, type_annotation: Some(TypeAnnotation { type_annotation: TypeReference { type_name: Identifier { name: "T" }, type_arguments: None } }), asserts: false } }, typeParameters: TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: Some(Keyword(Object)), default: Some(TypeLiteral { members: [] }), is_in: false, is_out: false, is_const: false }] }}"#
	);
	assert_eq!(
		module(r#"const g = (a: A, b = 1): B => a;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "g" }, init: Some(ArrowFunctionExpression { params: [Identifier { name: "a" }, AssignmentPattern { left: Identifier { name: "b" }, right: NumberLiteral { value: 1.0 } }], body: Identifier { name: "a" }, expression: true, is_async: false }) }], kind: Const }"#
	);
	assert_eq!(
		module(r#"for (const x of y as any[]) {}"#),
		r#"ForOfStatement { left: VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: None }], kind: Const }, right: AsExpression { expression: Identifier { name: "y" }, type_annotation: ArrayType { element_type: Keyword(Any) } }, body: BlockStatement { body: [] }, is_await: false }"#
	);
	assert_eq!(
		module(r#"try {} catch (e: unknown) {}"#),
		r#"TryStatement { block: BlockStatement { body: [] }, handler: Some(CatchClause { param: Some(Identifier { name: "e" }), body: BlockStatement { body: [] } }), finalizer: None }"#
	);
	assert_eq!(
		module(r#"let [a, b]: [number, string] = c;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: ArrayPattern { elements: [Identifier { name: "a" }, Identifier { name: "b" }] }, init: Some(Identifier { name: "c" }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x: A<B<C<D>>> = 1;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NumberLiteral { value: 1.0 }) }], kind: Let }"#
	);
}

#[test]
fn arrows() {
	assert_eq!(
		module(r#"const g = <T,>(x: T) => x;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "g" }, init: Some(ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: false }) }], kind: Const }"#
	);
	assert_eq!(
		module(r#"const g = async <T>(x: T): Promise<T> => x;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "g" }, init: Some(ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: true }) }], kind: Const }"#
	);
	assert_eq!(
		module(r#"const h = a ? (b) : c => d;"#),
		r#"error 26: Unexpected token"#
	);
	assert_eq!(
		module(r#"const f = <const T,>(x: T) => x;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "f" }, init: Some(ArrowFunctionExpression { params: [Identifier { name: "x" }], body: Identifier { name: "x" }, expression: true, is_async: false }) }], kind: Const }"#
	);
}

#[test]
fn functions() {
	assert_eq!(
		module(r#"function f(): void;"#),
		r#"DeclareFunction { id: Some(Identifier { name: "f" }), params: [], is_async: false, generator: false } +{returnType: TypeAnnotation { type_annotation: Keyword(Void) }}"#
	);
	assert_eq!(
		module(r#"function f(): void; function f() {}"#),
		r#"DeclareFunction { id: Some(Identifier { name: "f" }), params: [], is_async: false, generator: false } +{returnType: TypeAnnotation { type_annotation: Keyword(Void) }}; FunctionDeclaration { function: Function { id: Some(Identifier { name: "f" }), params: [], body: BlockStatement { body: [] }, is_async: false, generator: false } }"#
	);
	assert_eq!(
		module("function f(a: string) {}\nfunction f(a) {}"),
		r#"error 34: Identifier 'f' has already been declared"#
	);
	assert_eq!(
		module(r#"let f = function<T>() {};"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "f" }, init: Some(FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [] }, is_async: false, generator: false } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let o = { m<T>(x: T) {}, get g(): number { return 1 } };"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "o" }, init: Some(ObjectExpression { properties: [Property { key: Identifier { name: "m" }, value: FunctionExpression { function: Function { id: None, params: [Identifier { name: "x" }], body: BlockStatement { body: [] }, is_async: false, generator: false } }, kind: Init, computed: false, method: true, shorthand: false }, Property { key: Identifier { name: "g" }, value: FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [ReturnStatement { argument: Some(NumberLiteral { value: 1.0 }) }] }, is_async: false, generator: false } }, kind: Get, computed: false, method: false, shorthand: false }] }) }], kind: Let }"#
	);
}

#[test]
fn classes() {
	assert_eq!(
		module(r#"class C<T> extends D<T> implements I, J {}"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "C" }), super_class: Some(Identifier { name: "D" }), body: ClassBody { body: [] } } } +{typeParameters: TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: None, default: None, is_in: false, is_out: false, is_const: false }] }, superTypeParameters: TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }] }, implements: [ExpressionWithTypeArguments { expression: Identifier { name: "I" }, type_arguments: None }, ExpressionWithTypeArguments { expression: Identifier { name: "J" }, type_arguments: None }]}"#
	);
	assert_eq!(
		module(r#"class C { private readonly x?: number; static y!: string; declare z: T }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "C" }), super_class: None, body: ClassBody { body: [PropertyDefinition { key: Identifier { name: "x" }, value: None, computed: false, is_static: false }, PropertyDefinition { key: Identifier { name: "y" }, value: None, computed: false, is_static: true }, PropertyDefinition { key: Identifier { name: "z" }, value: None, computed: false, is_static: false }] } } }"#
	);
	assert_eq!(
		module(r#"class C { constructor(public a: string, protected b?: number) { super() } }"#),
		r#"error 64: super() call outside constructor of a subclass"#
	);
	assert_eq!(
		module(r#"class C { m<U>(x: U): U { return x } [key: string]: unknown; get v(): number { return 1 } }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "C" }), super_class: None, body: ClassBody { body: [MethodDefinition { key: Identifier { name: "m" }, value: FunctionExpression { function: Function { id: None, params: [Identifier { name: "x" }], body: BlockStatement { body: [ReturnStatement { argument: Some(Identifier { name: "x" }) }] }, is_async: false, generator: false } }, kind: Method, computed: false, is_static: false }, IndexSignature { parameters: [Identifier { name: "key" }], type_annotation: Some(TypeAnnotation { type_annotation: Keyword(Unknown) }) }, MethodDefinition { key: Identifier { name: "v" }, value: FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [ReturnStatement { argument: Some(NumberLiteral { value: 1.0 }) }] }, is_async: false, generator: false } }, kind: Get, computed: false, is_static: false }] } } }"#
	);
	assert_eq!(
		module(r#"abstract class A { abstract m(): void; abstract x: number; override y = 1 }"#),
		r#"error 59: This member cannot have an 'override' modifier because its containing class does not extend another class."#
	);
	assert_eq!(
		module(r#"class A { constructor(); constructor(x?: number) {} }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [MethodDefinition { key: Identifier { name: "constructor" }, value: DeclareMethod { params: [], is_async: false, generator: false }, kind: Constructor, computed: false, is_static: false }, MethodDefinition { key: Identifier { name: "constructor" }, value: FunctionExpression { function: Function { id: None, params: [Identifier { name: "x" }], body: BlockStatement { body: [] }, is_async: false, generator: false } }, kind: Constructor, computed: false, is_static: false }] } } }"#
	);
	assert_eq!(
		module(r#"class A { accessor x = 1 }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [PropertyDefinition { key: Identifier { name: "x" }, value: Some(NumberLiteral { value: 1.0 }), computed: false, is_static: false }] } } }"#
	);
	assert_eq!(
		module(r#"class A { m(): void; }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [MethodDefinition { key: Identifier { name: "m" }, value: DeclareMethod { params: [], is_async: false, generator: false } +{returnType: TypeAnnotation { type_annotation: Keyword(Void) }}, kind: Method, computed: false, is_static: false }] } } }"#
	);
	assert_eq!(
		module(r#"class A { static { } }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [StaticBlock { body: [] }] } } }"#
	);
	assert_eq!(
		module(r#"class A { static x: number; static m(): void {} }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [PropertyDefinition { key: Identifier { name: "x" }, value: None, computed: false, is_static: true }, MethodDefinition { key: Identifier { name: "m" }, value: FunctionExpression { function: Function { id: None, params: [], body: BlockStatement { body: [] }, is_async: false, generator: false } }, kind: Method, computed: false, is_static: true }] } } }"#
	);
	assert_eq!(
		module(r#"class A { private #x = 1 }"#),
		r#"error 10: Private elements cannot have an accessibility modifier ('private')."#
	);
	assert_eq!(
		module(r#"abstract class A { abstract m() {} }"#),
		r#"error 19: Method 'm' cannot have an implementation because it is marked abstract."#
	);
	assert_eq!(
		module(r#"class A { abstract m(): void }"#),
		r#"error 10: Abstract methods can only appear within an abstract class."#
	);
}

#[test]
fn interfaces_and_types() {
	assert_eq!(
		module(r#"interface A<T> extends B<T>, C { a?: string; readonly b: number }"#),
		r#"InterfaceDeclaration { id: Identifier { name: "A" }, type_parameters: Some(TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: None, default: None, is_in: false, is_out: false, is_const: false }] }), extends: Some([ExpressionWithTypeArguments { expression: Identifier { name: "B" }, type_arguments: Some(TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }] }) }, ExpressionWithTypeArguments { expression: Identifier { name: "C" }, type_arguments: None }]), body: InterfaceBody { body: [PropertySignature { key: Identifier { name: "a" }, computed: Some(false), optional: true, readonly: false, kind: None, type_annotation: Some(TypeAnnotation { type_annotation: Keyword(String) }) }, PropertySignature { key: Identifier { name: "b" }, computed: Some(false), optional: false, readonly: true, kind: None, type_annotation: Some(TypeAnnotation { type_annotation: Keyword(Number) }) }] } }"#
	);
	assert_eq!(
		module(r#"interface A { m(x: T): void; new (x: T): A<T>; (x: T): void }"#),
		r#"InterfaceDeclaration { id: Identifier { name: "A" }, type_parameters: None, extends: None, body: InterfaceBody { body: [MethodSignature { key: Identifier { name: "m" }, computed: false, optional: false, kind: Method, type_parameters: None, parameters: [Identifier { name: "x" }], type_annotation: Some(TypeAnnotation { type_annotation: Keyword(Void) }) }, ConstructSignatureDeclaration { type_parameters: None, parameters: [Identifier { name: "x" }], type_annotation: Some(TypeAnnotation { type_annotation: TypeReference { type_name: Identifier { name: "A" }, type_arguments: Some(TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }] }) } }) }, CallSignatureDeclaration { type_parameters: None, parameters: [Identifier { name: "x" }], type_annotation: Some(TypeAnnotation { type_annotation: Keyword(Void) }) }] } }"#
	);
	assert_eq!(
		module(r#"interface A { [k: string]: any; get g(): number; set s(v) }"#),
		r#"InterfaceDeclaration { id: Identifier { name: "A" }, type_parameters: None, extends: None, body: InterfaceBody { body: [IndexSignature { parameters: [Identifier { name: "k" }], type_annotation: Some(TypeAnnotation { type_annotation: Keyword(Any) }) }, MethodSignature { key: Identifier { name: "g" }, computed: false, optional: false, kind: Get, type_parameters: None, parameters: [], type_annotation: Some(TypeAnnotation { type_annotation: Keyword(Number) }) }, MethodSignature { key: Identifier { name: "s" }, computed: false, optional: false, kind: Set, type_parameters: None, parameters: [Identifier { name: "v" }], type_annotation: None }] } }"#
	);
	assert_eq!(
		module(r#"type A<in out T> = { [K in keyof T]?: T[K] };"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "A" }, type_parameters: Some(TypeParameterDeclaration { params: [TypeParameter { name: "T", constraint: None, default: None, is_in: true, is_out: true, is_const: false }] }), type_annotation: MappedType { readonly: None, type_parameter: TypeParameter { name: "K", constraint: Some(TypeOperator { operator: "keyof", type_annotation: TypeReference { type_name: Identifier { name: "T" }, type_arguments: None } }), default: None, is_in: false, is_out: false, is_const: false }, name_type: None, optional: Some(True), type_annotation: Some(IndexedAccessType { object_type: TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }, index_type: TypeReference { type_name: Identifier { name: "K" }, type_arguments: None } }) } }"#
	);
	assert_eq!(
		module(r#"type A<const T> = T;"#),
		r#"error 7: Unexpected keyword 'const'"#
	);
	assert_eq!(
		module(r#"type B = T extends U ? X : Y;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "B" }, type_parameters: None, type_annotation: ConditionalType { check_type: TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }, extends_type: TypeReference { type_name: Identifier { name: "U" }, type_arguments: None }, true_type: TypeReference { type_name: Identifier { name: "X" }, type_arguments: None }, false_type: TypeReference { type_name: Identifier { name: "Y" }, type_arguments: None } } }"#
	);
	assert_eq!(
		module(r#"type C = T extends (infer U extends string)[] ? U : never;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "C" }, type_parameters: None, type_annotation: ConditionalType { check_type: TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }, extends_type: ArrayType { element_type: ParenthesizedType { type_annotation: InferType { type_parameter: TypeParameter { name: "U", constraint: Some(Keyword(String)), default: None, is_in: false, is_out: false, is_const: false } } } }, true_type: TypeReference { type_name: Identifier { name: "U" }, type_arguments: None }, false_type: Keyword(Never) } }"#
	);
	assert_eq!(
		module(r#"type D = [a: string, b?: number, ...rest: boolean[]];"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "D" }, type_parameters: None, type_annotation: TupleType { element_types: [NamedTupleMember { label: Identifier { name: "a" }, optional: false, element_type: Keyword(String) }, NamedTupleMember { label: Identifier { name: "b" }, optional: true, element_type: Keyword(Number) }, RestType { type_annotation: NamedTupleMember { label: Identifier { name: "rest" }, optional: false, element_type: ArrayType { element_type: Keyword(Boolean) } } }] } }"#
	);
	assert_eq!(
		module(r#"type E = `a${string}b`;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "E" }, type_parameters: None, type_annotation: LiteralType { literal: TemplateLiteral { quasis: [TemplateElement { cooked: Some("a"), raw: "a", tail: false }, TemplateElement { cooked: Some("b"), raw: "b", tail: true }], expressions: [Keyword(String)] } } }"#
	);
	assert_eq!(
		module(r#"type F = typeof import("x").y<Z>;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "F" }, type_parameters: None, type_annotation: TypeQuery { expr_name: ImportType { argument: StringLiteral { value: "x" }, qualifier: Some(Identifier { name: "y" }), type_arguments: Some(TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "Z" }, type_arguments: None }] }) }, type_arguments: None } }"#
	);
	assert_eq!(
		module(r#"type G = readonly string[] | unique symbol | keyof typeof x;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "G" }, type_parameters: None, type_annotation: UnionType { types: [TypeOperator { operator: "readonly", type_annotation: ArrayType { element_type: Keyword(String) } }, TypeOperator { operator: "unique", type_annotation: Keyword(Symbol) }, TypeOperator { operator: "keyof", type_annotation: TypeQuery { expr_name: Identifier { name: "x" }, type_arguments: None } }] } }"#
	);
	assert_eq!(
		module(r#"type H = new (x: number) => Foo;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "H" }, type_parameters: None, type_annotation: ConstructorType { type_parameters: None, parameters: [Identifier { name: "x" }], type_annotation: TypeAnnotation { type_annotation: TypeReference { type_name: Identifier { name: "Foo" }, type_arguments: None } }, is_abstract: false } }"#
	);
	assert_eq!(
		module(r#"type I = abstract new () => Foo;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "I" }, type_parameters: None, type_annotation: ConstructorType { type_parameters: None, parameters: [], type_annotation: TypeAnnotation { type_annotation: TypeReference { type_name: Identifier { name: "Foo" }, type_arguments: None } }, is_abstract: true } }"#
	);
	assert_eq!(
		module(r#"type J = (a: string) => asserts a is string;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "J" }, type_parameters: None, type_annotation: FunctionType { type_parameters: None, parameters: [Identifier { name: "a" }], type_annotation: TypeAnnotation { type_annotation: TypePredicate { parameter_name: Identifier { name: "a" }, type_annotation: Some(TypeAnnotation { type_annotation: Keyword(String) }), asserts: true } } } }"#
	);
	assert_eq!(
		module(r#"type K = this;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "K" }, type_parameters: None, type_annotation: ThisType }"#
	);
	assert_eq!(
		module(r#"type L = intrinsic;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "L" }, type_parameters: None, type_annotation: Keyword(Intrinsic) }"#
	);
	assert_eq!(
		module(r#"type M = -1 | "a" | true | null | undefined | void;"#),
		r#"TypeAliasDeclaration { id: Identifier { name: "M" }, type_parameters: None, type_annotation: UnionType { types: [LiteralType { literal: UnaryExpression { operator: Minus, argument: NumberLiteral { value: 1.0 } } }, LiteralType { literal: StringLiteral { value: "a" } }, LiteralType { literal: BooleanLiteral { value: true } }, Keyword(Null), Keyword(Undefined), Keyword(Void)] } }"#
	);
}

#[test]
fn enums_and_namespaces() {
	assert_eq!(
		module(r#"enum E { A = 1, B, "C" }"#),
		r#"EnumDeclaration { id: Identifier { name: "E" }, members: [EnumMember { id: Identifier { name: "A" }, initializer: Some(NumberLiteral { value: 1.0 }) }, EnumMember { id: Identifier { name: "B" }, initializer: None }, EnumMember { id: StringLiteral { value: "C" }, initializer: None }], is_const: false }"#
	);
	assert_eq!(
		module(r#"const enum E { A }"#),
		r#"EnumDeclaration { id: Identifier { name: "E" }, members: [EnumMember { id: Identifier { name: "A" }, initializer: None }], is_const: true }"#
	);
	assert_eq!(
		module(r#"declare enum E { A }"#),
		r#"EnumDeclaration { id: Identifier { name: "E" }, members: [EnumMember { id: Identifier { name: "A" }, initializer: None }], is_const: false } +{declare}"#
	);
	assert_eq!(
		module(r#"namespace N.M { export const x = 1 }"#),
		r#"ModuleDeclaration { id: Identifier { name: "N" }, body: Some(ModuleDeclaration { id: Identifier { name: "M" }, body: Some(ModuleBlock { body: [ExportNamedDeclaration { declaration: Some(VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NumberLiteral { value: 1.0 }) }], kind: Const }), specifiers: [], source: None, attributes: [] }] }), global: false }), global: false }"#
	);
	assert_eq!(
		module(r#"declare module "m" { export function f(): void; }"#),
		r#"ModuleDeclaration { id: StringLiteral { value: "m" }, body: Some(ModuleBlock { body: [ExportNamedDeclaration { declaration: Some(DeclareFunction { id: Some(Identifier { name: "f" }), params: [], is_async: false, generator: false } +{returnType: TypeAnnotation { type_annotation: Keyword(Void) }}), specifiers: [], source: None, attributes: [] }] }), global: false } +{declare}"#
	);
	assert_eq!(
		module(r#"declare global { interface Window { x: number } }"#),
		r#"ModuleDeclaration { id: Identifier { name: "global" }, body: Some(ModuleBlock { body: [InterfaceDeclaration { id: Identifier { name: "Window" }, type_parameters: None, extends: None, body: InterfaceBody { body: [PropertySignature { key: Identifier { name: "x" }, computed: Some(false), optional: false, readonly: false, kind: None, type_annotation: Some(TypeAnnotation { type_annotation: Keyword(Number) }) }] } }] }), global: true } +{declare}"#
	);
	assert_eq!(
		module(r#"declare namespace N { const x = 1; }"#),
		r#"ModuleDeclaration { id: Identifier { name: "N" }, body: Some(ModuleBlock { body: [VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NumberLiteral { value: 1.0 }) }], kind: Const }] }), global: false } +{declare}"#
	);
	assert_eq!(
		module(r#"declare namespace N { export const x = a.b; }"#),
		r#"ModuleDeclaration { id: Identifier { name: "N" }, body: Some(ModuleBlock { body: [ExportNamedDeclaration { declaration: Some(VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(MemberExpression { object: Identifier { name: "a" }, property: Identifier { name: "b" }, computed: false, optional: false }) }], kind: Const }), specifiers: [], source: None, attributes: [] }] }), global: false } +{declare}"#
	);
}

#[test]
fn ambient() {
	assert_eq!(
		module(r#"declare const x: number;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: None }], kind: Const } +{declare}"#
	);
	assert_eq!(
		module(r#"declare const x = 1;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NumberLiteral { value: 1.0 }) }], kind: Const } +{declare}"#
	);
	assert_eq!(
		module(r#"declare function f(): void;"#),
		r#"DeclareFunction { id: Some(Identifier { name: "f" }), params: [], is_async: false, generator: false } +{returnType: TypeAnnotation { type_annotation: Keyword(Void) }, declare}"#
	);
	assert_eq!(
		module(r#"declare function f() {}"#),
		r#"error 0: An implementation cannot be declared in ambient contexts."#
	);
	assert_eq!(
		module(r#"declare class A { m(): void }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [MethodDefinition { key: Identifier { name: "m" }, value: DeclareMethod { params: [], is_async: false, generator: false } +{returnType: TypeAnnotation { type_annotation: Keyword(Void) }}, kind: Method, computed: false, is_static: false }] } } } +{declare}"#
	);
	assert_eq!(
		module(r#"declare abstract class A {}"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [] } } } +{declare, abstract}"#
	);
	assert_eq!(
		module(r#"export declare const x: number;"#),
		r#"ExportNamedDeclaration { declaration: Some(VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: None }], kind: Const }), specifiers: [], source: None, attributes: [] }"#
	);
}

#[test]
fn modules() {
	assert_eq!(
		module(r#"export type { A, B as C };"#),
		r#"error 14: Export 'A' is not defined"#
	);
	assert_eq!(
		module(r#"export type A = 1;"#),
		r#"ExportNamedDeclaration { declaration: Some(TypeAliasDeclaration { id: Identifier { name: "A" }, type_parameters: None, type_annotation: LiteralType { literal: NumberLiteral { value: 1.0 } } }), specifiers: [], source: None, attributes: [] } +{exportKind: type}"#
	);
	assert_eq!(
		module(r#"export interface A {}"#),
		r#"ExportNamedDeclaration { declaration: Some(InterfaceDeclaration { id: Identifier { name: "A" }, type_parameters: None, extends: None, body: InterfaceBody { body: [] } }), specifiers: [], source: None, attributes: [] } +{exportKind: type}"#
	);
	assert_eq!(
		module(r#"export default interface A {}"#),
		r#"ExportDefaultDeclaration { declaration: InterfaceDeclaration { id: Identifier { name: "A" }, type_parameters: None, extends: None, body: InterfaceBody { body: [] } } }"#
	);
	assert_eq!(
		module(r#"export default abstract class A {}"#),
		r#"ExportDefaultDeclaration { declaration: ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [] } } } }"#
	);
	assert_eq!(
		module(r#"export = x;"#),
		r#"ExportAssignment { expression: Identifier { name: "x" } }"#
	);
	assert_eq!(
		module(r#"export as namespace N;"#),
		r#"NamespaceExportDeclaration { id: Identifier { name: "N" } }"#
	);
	assert_eq!(
		module(r#"export import x = N.y;"#),
		r#"ImportEqualsDeclaration { id: Identifier { name: "x" }, module_reference: QualifiedName { left: Identifier { name: "N" }, right: Identifier { name: "y" } }, is_export: true, import_kind: Value }"#
	);
	assert_eq!(
		module(r#"import type A from "a";"#),
		r#"ImportDeclaration { specifiers: [ImportDefaultSpecifier { local: Identifier { name: "A" } }], source: StringLiteral { value: "a" }, attributes: [] } +{importKind: type}"#
	);
	assert_eq!(
		module(r#"import { type A, B, type C as D } from "a";"#),
		r#"ImportDeclaration { specifiers: [ImportSpecifier { imported: Identifier { name: "A" }, local: Identifier { name: "A" } }, ImportSpecifier { imported: Identifier { name: "B" }, local: Identifier { name: "B" } }, ImportSpecifier { imported: Identifier { name: "C" }, local: Identifier { name: "D" } }], source: StringLiteral { value: "a" }, attributes: [] } +{importKind: value}"#
	);
	assert_eq!(
		module(r#"import x = require("x");"#),
		r#"ImportEqualsDeclaration { id: Identifier { name: "x" }, module_reference: ExternalModuleReference { expression: StringLiteral { value: "x" } }, is_export: false, import_kind: Value }"#
	);
	assert_eq!(
		module(r#"import type x = require("x");"#),
		r#"ImportEqualsDeclaration { id: Identifier { name: "x" }, module_reference: ExternalModuleReference { expression: StringLiteral { value: "x" } }, is_export: false, import_kind: Type }"#
	);
	assert_eq!(
		module(r#"import type A, { B } from "a";"#),
		r#"error 0: A type-only import can specify a default import or named bindings, but not both."#
	);
}

#[test]
fn contextual_keywords() {
	assert_eq!(
		module("type\nX = 1;"),
		r#"ExpressionStatement { expression: Identifier { name: "type" }, directive: None }; ExpressionStatement { expression: AssignmentExpression { operator: Assign, left: Identifier { name: "X" }, right: NumberLiteral { value: 1.0 } }, directive: None }"#
	);
	assert_eq!(
		module(r#"type = 1;"#),
		r#"ExpressionStatement { expression: AssignmentExpression { operator: Assign, left: Identifier { name: "type" }, right: NumberLiteral { value: 1.0 } }, directive: None }"#
	);
	assert_eq!(
		module("interface\nX {}"),
		r#"error 0: The keyword 'interface' is reserved"#
	);
	assert_eq!(
		module("abstract\nclass A {}"),
		r#"ExpressionStatement { expression: Identifier { name: "abstract" }, directive: None }; ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [] } } }"#
	);
}

#[test]
fn expressions() {
	assert_eq!(
		module(r#"let x = a as B<C>;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(AsExpression { expression: Identifier { name: "a" }, type_annotation: TypeReference { type_name: Identifier { name: "B" }, type_arguments: Some(TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "C" }, type_arguments: None }] }) } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = a satisfies B;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(SatisfiesExpression { expression: Identifier { name: "a" }, type_annotation: TypeReference { type_name: Identifier { name: "B" }, type_arguments: None } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = a as const;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(AsExpression { expression: Identifier { name: "a" }, type_annotation: TypeReference { type_name: Identifier { name: "const" }, type_arguments: None } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = <T>y;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(TypeAssertion { type_annotation: TypeReference { type_name: Identifier { name: "T" }, type_arguments: None }, expression: Identifier { name: "y" } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = a!.b!;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NonNullExpression { expression: MemberExpression { object: NonNullExpression { expression: Identifier { name: "a" } }, property: Identifier { name: "b" }, computed: false, optional: false } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = f<T>(y);"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(CallExpression { callee: Identifier { name: "f" }, arguments: [Identifier { name: "y" }], optional: false }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = a < b > c;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(BinaryExpression { operator: Gt, left: BinaryExpression { operator: Lt, left: Identifier { name: "a" }, right: Identifier { name: "b" } }, right: Identifier { name: "c" } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = a<b>>c;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(BinaryExpression { operator: Lt, left: Identifier { name: "a" }, right: BinaryExpression { operator: Shr, left: Identifier { name: "b" }, right: Identifier { name: "c" } } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = new Foo<T>();"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(NewExpression { callee: Identifier { name: "Foo" }, arguments: [] }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = f<T>`tpl`;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(TaggedTemplateExpression { tag: Identifier { name: "f" }, quasi: TemplateLiteral { quasis: [TemplateElement { cooked: Some("tpl"), raw: "tpl", tail: true }], expressions: [] } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = a?.<T>();"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(ChainExpression { expression: CallExpression { callee: Identifier { name: "a" }, arguments: [], optional: true } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = A<B>;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(InstantiationExpression { expression: Identifier { name: "A" }, type_arguments: TypeParameterInstantiation { params: [TypeReference { type_name: Identifier { name: "B" }, type_arguments: None }] } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"let x = A<B>.c;"#),
		r#"error 12: Invalid property access after an instantiation expression. You can either wrap the instantiation expression in parentheses, or delete the type arguments."#
	);
	assert_eq!(
		module(r#"let x = (a as any) = 1;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(AssignmentExpression { operator: Assign, left: Identifier { name: "a" }, right: NumberLiteral { value: 1.0 } }) }], kind: Let }"#
	);
	assert_eq!(
		module(r#"f(a?: number);"#),
		r#"error 4: Did not expect a type annotation here."#
	);
	assert_eq!(
		module(r#"let x = y as T >> 2;"#),
		r#"VariableDeclaration { declarations: [VariableDeclarator { id: Identifier { name: "x" }, init: Some(BinaryExpression { operator: Shr, left: AsExpression { expression: Identifier { name: "y" }, type_annotation: TypeReference { type_name: Identifier { name: "T" }, type_arguments: None } }, right: NumberLiteral { value: 2.0 } }) }], kind: Let }"#
	);
}

#[test]
fn decorators() {
	assert_eq!(
		module(r#"@dec() @other class A { @prop x = 1; @m() f(@p x: number) {} }"#),
		r#"ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [PropertyDefinition { key: Identifier { name: "x" }, value: Some(NumberLiteral { value: 1.0 }), computed: false, is_static: false }, MethodDefinition { key: Identifier { name: "f" }, value: FunctionExpression { function: Function { id: None, params: [Identifier { name: "x" }], body: BlockStatement { body: [] }, is_async: false, generator: false } }, kind: Method, computed: false, is_static: false }] } } } +{decorators: [Decorator { expression: CallExpression { callee: Identifier { name: "dec" }, arguments: [], optional: false } }, Decorator { expression: Identifier { name: "other" } }]}"#
	);
	assert_eq!(
		module(r#"@dec export class A {}"#),
		r#"ExportNamedDeclaration { declaration: Some(ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [] } } }), specifiers: [], source: None, attributes: [] }"#
	);
	assert_eq!(
		module(r#"export @dec class A {}"#),
		r#"ExportNamedDeclaration { declaration: Some(ClassDeclaration { class: Class { id: Some(Identifier { name: "A" }), super_class: None, body: ClassBody { body: [] } } }), specifiers: [], source: None, attributes: [] }"#
	);
	assert_eq!(
		module(r#"@dec let x = 1;"#),
		r#"error 5: Leading decorators must be attached to a class declaration."#
	);
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
fn until_as() {
	let options = Options {
		module: true,
		until_as: true,
		..Options::default()
	};
	let end = |src: &str| parse_expression_at(src, 1, options).unwrap().2;
	assert_eq!(end("{xs as item}"), 3);
	assert_eq!(end("{xs as item, i (item.id)}"), 3);
	assert_eq!(end("{xs as [a, b = 1]}"), 3);
	assert_eq!(end("{xs as T[] as item}"), 10);
	assert_eq!(end("{xs as T === y as item}"), 14);
	assert_eq!(end("{xs as T, ys as item}"), 12);
	assert_eq!(end("{xs as unknown as T[] as item: T, i}"), 21);
	assert_eq!(end("{xs as const as item}"), 12);
	assert_eq!(end("{f(x as T) as item}"), 10);
	assert_eq!(end("{(xs as T) as item}"), 10);
}

/// The erased program's statements and what stayed TypeScript, from the JSON answer.
fn erase(src: &str) -> (Vec<String>, Vec<String>) {
	let mut request = crate::json::Request::new(crate::json::Entry::Program, 0);
	request.typescript = true;
	request.erase = true;
	let json = crate::json::parse(src, &request);
	assert!(!json.contains("\"error\""), "{json}");
	let types = |key: &str| {
		let start = json.find(&format!("\"{key}\":[")).unwrap() + key.len() + 4;
		let mut depth = 0;
		let mut out = Vec::new();
		let mut at = start;
		for (i, c) in json[start..].char_indices() {
			match c {
				'{' if depth == 0 => at = start + i,
				'{' => {}
				'}' if depth == 1 => {
					let item = &json[at..=start + i];
					let ty = item.split("\"type\":\"").nth(1).unwrap().split('"').next().unwrap();
					out.push(ty.to_string());
				}
				']' if depth == 0 => break,
				_ => {}
			}
			match c {
				'{' => depth += 1,
				'}' => depth -= 1,
				_ => {}
			}
		}
		out
	};
	(types("body"), types("typescript"))
}

#[test]
fn erasure() {
	let (body, kept) =
		erase("import type T from 't'; import { a, type B } from 'b'; export type { T }; type U = 1; interface I {}");
	assert_eq!(body, ["ImportDeclaration"]);
	assert!(kept.is_empty());
	let (body, kept) =
		erase("declare const d: number; export declare function f(): void; export const x: U = <any>(1 as any)!;");
	assert_eq!(body, ["ExportNamedDeclaration"]);
	assert!(kept.is_empty());
	let (body, kept) = erase(
		"enum E {} namespace N { export type X = 1 } namespace M { export const y = 1 } @dec class Z { constructor(public q: number) {} }",
	);
	assert_eq!(body, ["TSEnumDeclaration", "TSModuleDeclaration", "ClassDeclaration"]);
	assert_eq!(
		kept,
		[
			"TSEnumDeclaration",
			"TSModuleDeclaration",
			"Decorator",
			"TSParameterProperty"
		]
	);
	let (body, _) =
		erase("abstract class K implements I { declare d: number; abstract m(): void; p?: number; f(this: K) {} }");
	assert_eq!(body, ["ClassDeclaration"]);
	let mut request = crate::json::Request::new(crate::json::Entry::Program, 0);
	request.typescript = true;
	request.erase = true;
	let json = crate::json::parse("function f(this: Window, a?: number): void {}", &request);
	assert!(
		!json.contains("this") && !json.contains("optional") && !json.contains("returnType"),
		"{json}"
	);
}

#[test]
fn program_in_a_range() {
	let src = "<script>let a: number = 1;</script>{a}";
	let (ast, root) = {
		let ast = crate::parser::parse_range::<super::TypeScript>(
			src,
			8,
			26,
			Options {
				module: true,
				..Options::default()
			},
		)
		.unwrap();
		let root = ast.last();
		(ast, root)
	};
	assert_eq!((ast.node(root).start, ast.node(root).end), (8, 26));
	assert_eq!(ast.comments.len(), 0);
}
