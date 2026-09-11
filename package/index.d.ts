import type { Expression, Identifier, Node, Pattern, Program, SourceLocation, Statement } from 'estree';

export interface Options {
	/**
	 * The grammar of a host language the whole source is a document of: a template language
	 * with JavaScript inside it. The program entry then answers with the document's root, the
	 * host's own nodes around the JavaScript ones, in one tree; the other entries read
	 * JavaScript at an offset as before. TypeScript turns on by what the grammar says of a
	 * script tag.
	 */
	host?: string;
	/** `script` by default, as in acorn. */
	sourceType?: 'script' | 'module';
	/**
	 * Parse TypeScript. `'erase'` parses it and emits JavaScript: annotations, type-only
	 * declarations and imports go, assertions give way to their expression, and what erasure
	 * cannot express (enums, namespaces with values, parameter properties, `export =`, `import =`)
	 * stays in the tree and is listed as `typescript` on the answer, as are the proposals
	 * JavaScript itself has: decorators and accessor fields (`AccessorProperty`).
	 */
	typescript?: boolean | 'erase';
	/** Attach `leadingComments`, `trailingComments` and `innerComments` to nodes, and list every comment read as `comments` on the answer. */
	comments?: boolean;
	/**
	 * Scope analysis: the answer lists `scopes`, `bindings` and `references`, and `scopeOf`,
	 * `bindingOf` and `referenceOf` answer for a node. The tree itself carries nothing, and a copy
	 * of a node carries no facts. TypeScript type positions bind nothing.
	 */
	scopes?: boolean;
	/** Add `loc` with line and column to every node, as in acorn; off by default. */
	locations?: boolean;
	/** Mark a node the source wraps in parens with `parenthesized: true`, absent otherwise. */
	parenthesized?: boolean;
	allowReturnOutsideFunction?: boolean;
	allowAwaitOutsideFunction?: boolean;
	allowSuperOutsideMethod?: boolean;
	allowUndeclaredExports?: boolean;
	/**
	 * List syntax errors on the answer as `errors` instead of throwing the first: a missing
	 * operand, name or pattern is an `Identifier` named `''` of no width where it was expected,
	 * and a statement or entry that cannot be read is skipped to the next stop token or
	 * unmatched closing bracket, an empty identifier standing for it. Placeholders are neither
	 * bindings nor references.
	 */
	errorRecovery?: boolean;
}

/**
 * Thrown for a syntax error. `code` names what went wrong, for a host to branch on, and
 * `message` says it in words, without a position. An error at the token being read spans it
 * with `pos` and `end`; one reported elsewhere, at a declaration seen earlier say, has `end`
 * equal to `pos`. `unexpected_eof` is the end of what was parsed: the `end` the parse was given,
 * else the end of the source. A bad offset from the host is an `invalid_request` without a `loc`.
 */
export interface ParseError extends SyntaxError {
	code: string;
	pos: number;
	end: number;
	loc?: { line: number; column: number };
}

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
	node: Node | null;
	parent: Scope | null;
	/** An `await` or `for await` runs directly in it, no function around; only a program or fragment scope can say so. */
	topLevelAwait: boolean;
}

/** A binding, as one of `bindings` on the answer. */
export interface Binding {
	name: string;
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
		| 'arguments'
		| 'enum'
		| 'enum-member'
		| 'namespace'
		| 'pattern';
	scope: Scope;
	/** The identifier that declares it; null for `arguments`. */
	node: Identifier | null;
	/** What declares it: the declarator, function, class, import specifier, catch clause or enum, as eslint-scope's definition node; null for `arguments` and for a pattern or parameter list parsed on its own. */
	declaration: Node | null;
}

/** A reference, as one of `references` on the answer. */
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
	/** The identifier's value is read: every reference but a plain assignment's target or a destructuring one's; a compound assignment or an update reads and writes. */
	read: boolean;
	/** What a write assigns: the right side of the assignment or the iterated expression of a `for-in` or `for-of`, as eslint-scope's `writeExpr`; null for an update. */
	writeExpr: Expression | null;
}

/** The node `node` is a child of; undefined for the root of an answer. A literal's `regex` and a template element's `value` are not nodes and have none. */
export function parentOf(node: Node): Node | undefined;
/** With `scopes`: the scope `node` opens, when it opens one. */
export function scopeOf(node: Node): Scope | undefined;
/** With `scopes`: what an identifier declares or refers to; null for a global, undefined when it names no value, a property key say. */
export function bindingOf(node: Node): Binding | null | undefined;
/** With `scopes`: the reference an identifier makes, with its `write` and `mutate`; a global's too, which no binding lists. */
export function referenceOf(node: Node): Reference | undefined;

/** A range of the source, with `loc` when `locations` is on. */
export interface Span {
	start: number;
	end: number;
	loc?: { start: { line: number; column: number }; end: { line: number; column: number } };
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
export type Recovered = Pick<ParseError, 'code' | 'message' | 'pos' | 'end'> & { loc: { line: number; column: number } };

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
}

/**
 * What a parse reads: a program, or what a host embedding JavaScript in a larger syntax reads at
 * a point of it. A type parameter list `<...>` is TypeScript only, `not_typescript` otherwise.
 */
export type Entry = 'program' | 'expression' | 'pattern' | 'params' | 'statement' | 'typeParameters';

export interface At {
	/** Where the source is cut, a UTF-16 offset; the end of the source by default. A program reads to it. */
	end?: number;
	/**
	 * The host's own tokens, words or punctuators, that follow what is parsed. One read outside
	 * every bracket the parse opened, where the expression could end, ends it: `,` ends an
	 * expression before a sequence would, and `/>` is never a division. A `then` after `.` is a
	 * property name. A TypeScript `as` is the host's unless another `as` follows the assertion,
	 * so `xs as T[] as item` ends after the type.
	 */
	stopAt?: string[];
}

/**
 * A node of a host language, as its grammar names the type and the fields; the JavaScript under
 * it is ESTree.
 */
export interface HostNode {
	type: string;
	start: number;
	end: number;
	loc?: SourceLocation;
	[field: string]: unknown;
}

/**
 * A source kept with its options: the parses out of it share the source copy and the position
 * tables. Offsets are UTF-16, as in acorn; positions stay those of the whole source. `Root` is
 * what the program entry answers with: the program, or the document's root with a `host`.
 */
export class Source<Root = Program> {
	constructor(source: string, options?: Options);
	/** The program starting at `offset`, the whole source by default; the document with a `host`. */
	parse(entry?: 'program', offset?: number, at?: At): Parsed<Root>;
	parse(entry: 'expression', offset: number, at?: At): Parsed<Expression>;
	/** An assignment target: an identifier or a destructuring pattern. */
	parse(entry: 'pattern', offset: number, at?: At): Parsed<Pattern>;
	/** A parenthesized parameter list, as an arrow function's is read. */
	parse(entry: 'params', offset: number, at?: At): Parsed<Pattern[]>;
	parse(entry: 'statement', offset: number, at?: At): Parsed<Statement>;
	/** A `TSTypeParameterDeclaration`. */
	parse(entry: 'typeParameters', offset: number, at?: At): Parsed<Node>;
	/** Releases what the engine holds for the source; the collector does it otherwise. */
	free(): void;
}

/** Whether a code point can start an identifier, as acorn decides it. */
export function isIdentifierStart(code: number): boolean;
/** Whether a code point can continue an identifier. */
export function isIdentifierChar(code: number): boolean;
