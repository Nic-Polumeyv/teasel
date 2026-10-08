import type { Expression, Identifier, Node, Position, SourceLocation } from 'estree';

/** A scope, as one of `scopes` on the answer. */
export interface Scope {
	kind:
		| 'module'
		| 'script'
		| 'function'
		| 'function-name'
		| 'class'
		| 'block'
		| 'catch'
		| 'for'
		| 'switch'
		| 'static-block'
		| 'with'
		| 'namespace'
		| 'enum'
		| 'fragment';
	/** The node that opens it; null for a function-name scope and for the scope around a parameter list parsed on its own. */
	node: Node | HostNode | null;
	parent: Scope | null;
	/** An `await` or `for await` runs directly in it, no function around; only a program or fragment scope can say so. */
	topLevelAwait: boolean;
}

/** A binding, as one of `bindings` on the answer: one an identifier declares, or the `arguments` a function reads. */
export type Binding = Declared | Arguments;

/** A binding an identifier declares. It is the reference that identifier makes, the first of its own: `referenceOf` answers with it, and its `binding` is itself. */
export interface Declared extends Reference {
	name: string;
	/**
	 * What declared it. `function-name` and `class-name` are the name a function expression or a class
	 * expression has inside itself, `const f = function g() {}` declaring `g`. `pattern` is a name that
	 * a `js.pattern` piece declares, parsed on its own; a `js.params` piece declares `param`s.
	 */
	kind:
		| 'var'
		| 'let'
		| 'const'
		| 'using'
		| 'await using'
		| 'function'
		| 'class'
		| 'param'
		| 'catch'
		| 'import'
		| 'function-name'
		| 'class-name'
		| 'enum'
		| 'enum-member'
		| 'namespace'
		| 'pattern';
	/** The identifier that declares it. */
	node: Identifier;
	/** The scope it is declared in. */
	scope: Scope;
	/** What declares it: the declarator, function, class, import specifier, catch clause or enum, as eslint-scope's definition node; null for a pattern or parameter list parsed on its own. */
	declaration: Node | null;
	binding: Declared;
	declares: true;
	/** The declaration binds a value: an initializer, a parameter, a function, a class, an import; not a bare `let x;`. */
	write: boolean;
	read: false;
	mutate: false;
	/** The initializer of a declarator, `1` in `let x = 1`; null otherwise, the iterated expression of a `for-of` and a parameter's default being on the tree. */
	writeExpr: Expression | null;
}

/** The `arguments` of a function that reads it: bound by the call, declared by no identifier. */
export interface Arguments {
	name: 'arguments';
	kind: 'arguments';
	scope: Scope;
	node: null;
	declaration: null;
	binding: Arguments;
	declares: true;
	write: true;
	read: false;
	mutate: false;
	writeExpr: null;
}

/** A piece of JavaScript a host read on its own, as one of `roots` on a document's answer, with what the tables hold for it. */
export interface Root {
	node: Node;
	/** The scope the piece sits in. */
	scope: Scope;
	/** The scopes opened inside it, the bindings declared and the references made there. */
	scopes: Scope[];
	bindings: Binding[];
	references: Reference[];
}

/** A reference, as one of `references` on the answer: an identifier using a name, or declaring it again. A binding is one too, the reference its declaring identifier makes. */
export interface Reference {
	node: Identifier;
	/** The scope the reference is made from. */
	scope: Scope;
	/** Null for a global. */
	binding: Binding | null;
	/** The identifier is assigned to, updated or bound by a destructuring assignment. */
	write: boolean;
	/** A member of the identifier's value is assigned to, updated or deleted. */
	mutate: boolean;
	/** The identifier's value is read: every reference but a declaration, a plain assignment's target or a destructuring one's; a compound assignment or an update reads and writes. */
	read: boolean;
	/** What a write assigns: the right side of the assignment, the iterated expression of a `for-in` or `for-of`, or what a declaration is initialized with, as eslint-scope's `writeExpr`; null for an update. */
	writeExpr: Expression | null;
	/** The identifier declares its binding: the binding itself for the first declaration, and a reference of its own for a name declared again, `var x` twice, which writes when a value is bound there. */
	declares: boolean;
}

/** A range of the source, with `loc` when `locations` is on. */
export interface Span {
	start: number;
	end: number;
	loc?: SourceLocation;
}

export interface Comment extends Span {
	type: 'Line' | 'Block';
	value: string;
}

/** A node erasure left in place, by type. */
export interface Kept extends Span {
	type: string;
}

/** A recovered error: what the thrown `SyntaxError` carries, as a plain object. */
export interface Recovered {
	code: Code;
	message: string;
	pos: number;
	end: number;
	loc: Position;
}

declare const answers: unique symbol;
/** What a parse reads as a whole and up to the end it is given: JavaScript, or a host language by its grammar. `T` is what its parse answers with. */
export interface Language<T> {
	readonly [answers]?: T;
	/** For every type of node a parse of the language answers with, the fields that hold a node or a list of nodes: what a walk follows. A grammar's names the JavaScript types beside the host's. */
	readonly children: Readonly<Record<string, readonly string[]>>;
}

/** What a parse returns: the node, or the patterns of a parameter list, and what the options add; a key is there exactly when its option is on. */
export interface Parsed<T> {
	node: T;
	/** The offset after everything the parse consumed: the node, its closing parens and the comments after it; a program's is the end it was given. */
	end: number;
	/** Every comment read, in source order; with `comments`. */
	comments?: Comment[];
	/** What erasure left in place; with `typescript: 'erase'`. */
	typescript?: Kept[];
	/** The errors recovered from, in source order; with `errorRecovery`. */
	errors?: Recovered[];
	/** With `scopes`. */
	scopes?: Scope[];
	bindings?: Binding[];
	references?: Reference[];
	/** With `scopes`, for a document read by a host grammar: its pieces of JavaScript in source order. */
	roots?: Root[];
}

/**
 * A node of a host language, as its grammar names the type and the fields; the JavaScript under
 * it is ESTree. The node a grammar wraps children in has no span.
 */
export interface HostNode extends Partial<Span> {
	type: string;
	[field: string]: unknown;
}

/** What a parse error names in `code`: every code the engine can report. */
export type Code =
	| 'unexpected_token'
	| 'expected'
	| 'unclosed'
	| 'unexpected_close'
	| 'invalid_name'
	| 'duplicate'
	| 'placement'
	| 'unexpected_eof'
	| 'unexpected_character'
	| 'unexpected_keyword'
	| 'reserved_word'
	| 'escape_in_keyword'
	| 'nesting_depth'
	| 'tree_size'
	| 'not_typescript'
	| 'unterminated_comment'
	| 'unterminated_string'
	| 'unterminated_template'
	| 'unterminated_regexp'
	| 'invalid_regexp'
	| 'invalid_regexp_flag'
	| 'duplicate_regexp_flag'
	| 'invalid_number'
	| 'expected_number_in_radix'
	| 'identifier_after_number'
	| 'numeric_separator_first'
	| 'numeric_separator_last'
	| 'numeric_separator_double'
	| 'numeric_separator_legacy_octal'
	| 'bad_character_escape'
	| 'bad_template_escape'
	| 'invalid_unicode_escape'
	| 'expected_unicode_escape'
	| 'code_point_out_of_bounds'
	| 'strict_with'
	| 'strict_delete'
	| 'strict_directive_non_simple_params'
	| 'strict_binding'
	| 'strict_octal'
	| 'strict_escape'
	| 'let_as_binding'
	| 'redeclaration'
	| 'duplicate_label'
	| 'duplicate_parameter'
	| 'duplicate_proto'
	| 'duplicate_default'
	| 'duplicate_export'
	| 'undefined_export'
	| 'string_export_without_from'
	| 'lone_surrogate_in_module_name'
	| 'duplicate_import_attribute'
	| 'import_export_in_script'
	| 'import_export_not_top_level'
	| 'import_meta_outside_module'
	| 'import_meta_escaped'
	| 'invalid_import_meta'
	| 'invalid_new_target'
	| 'new_target_escaped'
	| 'new_target_outside_function'
	| 'unsyntactic'
	| 'return_outside_function'
	| 'newline_after_throw'
	| 'missing_catch_or_finally'
	| 'for_in_of_initializer'
	| 'for_of_let'
	| 'await_as_identifier'
	| 'await_outside_async'
	| 'await_in_default_value'
	| 'yield_as_identifier'
	| 'yield_in_default_value'
	| 'invalid_in_static_block'
	| 'arguments_in_field_initializer'
	| 'invalid_assignment_target'
	| 'invalid_binding_target'
	| 'binding_member_expression'
	| 'parenthesized_pattern'
	| 'binding_parenthesized'
	| 'optional_chain_assignment'
	| 'optional_chain_in_new'
	| 'optional_chain_in_tagged_template'
	| 'mixed_coalesce'
	| 'private_name_outside_in'
	| 'undeclared_private_name'
	| 'delete_private'
	| 'invalid_super'
	| 'super_outside_method'
	| 'super_call_outside_constructor'
	| 'comma_after_rest'
	| 'rest_with_default'
	| 'accessor_in_pattern'
	| 'shorthand_assignment'
	| 'pattern_without_initializer'
	| 'using_pattern'
	| 'using_without_initializer'
	| 'using_in_for_in'
	| 'using_outside_block'
	| 'invalid_default_operator'
	| 'getter_params'
	| 'setter_params'
	| 'setter_rest_param'
	| 'generator_constructor'
	| 'async_constructor'
	| 'accessor_constructor'
	| 'duplicate_constructor'
	| 'constructor_field'
	| 'private_constructor'
	| 'static_prototype'
	| 'abstract_outside_abstract_class'
	| 'abstract_with_implementation'
	| 'abstract_with_initializer'
	| 'duplicate_accessibility'
	| 'duplicate_modifier'
	| 'conflicting_modifiers'
	| 'modifier_order'
	| 'declare_on_method'
	| 'readonly_on_method'
	| 'readonly_placement'
	| 'readonly_type_operand'
	| 'override_without_extends'
	| 'index_signature_modifier'
	| 'private_modifier'
	| 'static_block_modifier'
	| 'accessor_type_parameters'
	| 'constructor_type_parameters'
	| 'setter_return_type'
	| 'decorator_placement'
	| 'decorator_on_constructor'
	| 'decorator_without_body'
	| 'this_parameter_modifiers'
	| 'optional_with_initializer'
	| 'required_after_optional'
	| 'optional_rest'
	| 'setter_optional_parameter'
	| 'setter_parameter_initializer'
	| 'constructor_return_type'
	| 'constructor_modifier'
	| 'index_signature_type'
	| 'predicate_placement'
	| 'infer_placement'
	| 'const_assertion_target'
	| 'catch_clause_type'
	| 'abstract_placement'
	| 'export_assignment_in_namespace'
	| 'quoted_module_name'
	| 'implementation_in_ambient'
	| 'initializer_in_ambient'
	| 'ambient_const_initializer'
	| 'export_declare_without_declaration'
	| 'interface_without_name'
	| 'type_redeclaration'
	| 'import_type_alias'
	| 'type_import_argument'
	| 'type_import_default_and_named'
	| 'type_modifier_in_type_import'
	| 'unexpected_type_annotation'
	| 'type_annotation_after_default'
	| 'empty_type_arguments'
	| 'empty_type_parameters'
	| 'empty_list'
	| 'type_member_modifier'
	| 'type_parameter_modifier'
	| 'invalid_const'
	| 'tuple_label'
	| 'optional_pattern_parameter'
	| 'parameter_property_pattern'
	| 'signature_parameter_default'
	| 'definite_with_initializer'
	| 'generator_in_ambient'
	| 'generator_signature'
	| 'declare_in_ambient'
	| 'statement_in_ambient'
	| 'using_in_ambient'
	| 'invalid_request'
	| 'property_after_instantiation';

// ── the engine

type View = Uint32Array | Float64Array | Uint8Array;
/** Whether the tree is the TypeScript one, then each view of the layout's `views`, as long as its buffer's room; `undefined` for a table no parse filled yet. */
export type Tree = readonly (View | number | undefined)[];

/** What parses, as the reader sees it. */
export interface Views {
	/** The tree's memory layout, the names of its views and the recipes, as JSON. */
	readonly layout: () => string;
	/** The views of the last parse's tree, JavaScript's or TypeScript's; `moved` when a buffer of it has since this reader last took them. The same array as long as no view in it changed, `moved` aside. */
	readonly tree: (typescript: boolean, moved: boolean) => Tree;
}

/** What the engine holds: a prepared source, or a host language's grammar read once. */
export interface Held {
	readonly free: () => void;
}

/** A source the engine prepared: it parses at an entry and offset, cut at `end`, the stop tokens as one string, the whole source as a document by a grammar `plan` holds; the answer is its words, or an error as JSON. */
export interface Prepared extends Held {
	readonly parse: (entry: number, offset: number, end: number | undefined, stop: string, plan: Held | undefined) => Uint32Array | string;
}

/** What parses: the addon or the WebAssembly module. */
export interface Engine extends Views {
	readonly create: (source: string, flags: number) => Prepared;
	/** The grammar of a host language on its wire, read once. */
	readonly plan: (grammar: Uint8Array) => Held;
	/** Each node type of a grammar's host with the fields that hold nodes, as JSON; a stylesheet's without one. */
	readonly children: (plan: Held | undefined) => string;
}
