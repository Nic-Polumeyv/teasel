import type { Expression, Pattern, Program, Statement } from 'estree';

export interface Options {
	/** `script` by default, as in acorn. */
	sourceType?: 'script' | 'module';
	/**
	 * Parse TypeScript. `'erase'` parses it and emits JavaScript: annotations, type-only
	 * declarations and imports go, assertions give way to their expression, and what erasure
	 * cannot express (enums, namespaces with values, parameter properties, decorators, `export =`,
	 * `import =`) stays in the tree and is listed as `typescript` on the answer.
	 */
	typescript?: boolean | 'erase';
	/** Attach `leadingComments`, `trailingComments` and `innerComments` to nodes. */
	comments?: boolean;
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
 * `message` says it in words, without a position; `pos` and `end` span the offending token,
 * or meet at the error when there is none. A bad offset from the host is an `invalid_request`
 * without a `loc`.
 */
export interface ParseError extends SyntaxError {
	code: string;
	pos: number;
	end: number;
	loc?: { line: number; column: number };
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
