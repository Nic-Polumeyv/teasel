import type { Expression, Identifier, Node, Pattern, Program, Statement } from 'estree';

export interface Options {
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
	/** Attach `leadingComments`, `trailingComments` and `innerComments` to nodes. */
	comments?: boolean;
	/**
	 * Scope analysis. Every node that opens a scope gets `scope`, every identifier that declares
	 * something or refers to something gets `binding` (null for a global), with `write` when it
	 * is assigned to and `mutate` when a member of its value is; the answer lists `scopes` and
	 * `bindings`. TypeScript type positions bind nothing.
	 */
	scopes?: boolean;
	/** Add `loc` with line and column to every node, as in acorn; off by default. */
	locations?: boolean;
	preserveParens?: boolean;
	allowReturnOutsideFunction?: boolean;
	allowAwaitOutsideFunction?: boolean;
	allowSuperOutsideMethod?: boolean;
	allowUndeclaredExports?: boolean;
	/** Accepted for acorn's sake and ignored: the latest ECMAScript is always parsed. */
	ecmaVersion?: number | 'latest';
	/**
	 * For `parseExpressionAt` only: the host's own `as` follows the expression, as a template
	 * loop's item follows its list. The expression ends at the last top-level `as`, so
	 * TypeScript assertions before it stay assertions: `list as Type as item` ends before the
	 * second `as`.
	 */
	until?: 'as';
}

/**
 * Thrown for a syntax error. `code` names what went wrong, for a host to branch on, and
 * `message` says it in words, without a position. An error at the token being read spans it
 * with `pos` and `end`; one reported elsewhere, at a declaration seen earlier say, has `end`
 * equal to `pos`. `unexpected_eof` is the end of what was parsed: the `end` of a range parse,
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
		| 'fragment';
	/** The node that opens it; null for the scope around a parameter list parsed on its own. */
	node: Node | null;
	parent: Scope | null;
	/** How many function scopes enclose it, itself included when it is one. */
	functionDepth: number;
	bindings: Binding[];
	declarations: Map<string, Binding>;
	/** The bindings of outer scopes that identifiers inside it resolve to, in first-use order. */
	through: Binding[];
}

/** A binding, as one of `bindings` on the answer. */
export interface Binding {
	name: string;
	kind:
		| 'var'
		| 'let'
		| 'const'
		| 'function'
		| 'class'
		| 'param'
		| 'catch'
		| 'import'
		| 'function-name'
		| 'class-name'
		| 'arguments'
		| 'enum'
		| 'namespace';
	scope: Scope;
	/** The identifier that declares it; null for `arguments`. */
	node: Identifier | null;
	/** The identifiers that refer to it, the declaring one excluded, in source order. */
	references: Identifier[];
}

declare module 'estree' {
	interface BaseNode {
		/** With `scopes`: the scope this node opens. */
		scope?: Scope;
	}
	interface Identifier {
		/** With `scopes`: what the identifier declares or refers to; null for a global. */
		binding?: Binding | null;
		/** With `scopes`: the identifier is assigned to, updated or bound by a destructuring assignment. */
		write?: true;
		/** With `scopes`: a member of the identifier's value is assigned to, updated or deleted. */
		mutate?: true;
	}
}

/** A comment, with `loc` when `locations` is on. */
export interface Comment {
	type: 'Line' | 'Block';
	value: string;
	start: number;
	end: number;
	loc?: { start: { line: number; column: number }; end: { line: number; column: number } };
}

/** A node erasure left in place, by type and range. */
export interface Kept {
	type: string;
	start: number;
	end: number;
	loc?: { start: { line: number; column: number }; end: { line: number; column: number } };
}

/** What a parse at an offset returns. */
export interface Parsed<T> {
	node: T;
	/** With `scopes`. */
	scopes?: Scope[];
	bindings?: Binding[];
	/** The offset after everything the parse consumed: the node, its closing parens and the comments after it. */
	end: number;
	/** Every comment read, in source order; only with `comments`. */
	comments?: Comment[];
	/** What erasure left in place; only with `typescript: 'erase'`. */
	typescript?: Kept[];
}

/** A program, with the comment list and the erasure leftovers when those options are on. */
export type ParsedProgram = Program & { comments?: Comment[]; typescript?: Kept[] };

/** What `parseParamsAt` returns: the list rather than one node, otherwise as `Parsed`. */
export interface Params {
	params: Pattern[];
	/** The offset after the closing paren and the comments after it. */
	end: number;
	comments?: Comment[];
	typescript?: Kept[];
}

/** Parses a whole program; with `comments` it lists every comment as `comments`. */
export function parse(source: string, options?: Options): ParsedProgram;
/** Parses one expression starting at `offset`, a UTF-16 offset into `source`. */
export function parseExpressionAt(source: string, offset: number, options?: Options): Parsed<Expression>;
/** Parses an assignment target starting at `offset`. */
export function parsePatternAt(source: string, offset: number, options?: Options): Parsed<Pattern>;
/** Parses a parenthesized parameter list starting at `offset`. */
export function parseParamsAt(source: string, offset: number, options?: Options): Params;
/** Parses one statement starting at `offset`. */
export function parseStatementAt(source: string, offset: number, options?: Options): Parsed<Statement>;

/**
 * A source kept with its options: the parses out of it share the source copy and the position
 * tables, which is what a host parsing every expression of a template wants.
 */
export class Source {
	constructor(source: string, options?: Options);
	/** The whole source, or the program that spans `start..end` of it; positions stay those of the whole source. */
	parse(start?: number, end?: number): ParsedProgram;
	parseExpressionAt(offset: number, until?: 'as'): Parsed<Expression>;
	parsePatternAt(offset: number): Parsed<Pattern>;
	parseParamsAt(offset: number): Params;
	parseStatementAt(offset: number): Parsed<Statement>;
}

/** Whether a code point can start an identifier, as acorn decides it. */
export function isIdentifierStart(code: number): boolean;
/** Whether a code point can continue an identifier. */
export function isIdentifierChar(code: number): boolean;
