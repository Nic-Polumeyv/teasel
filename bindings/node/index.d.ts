import type { Expression, Pattern, Program, Statement } from 'estree';

export interface Options {
	/** `script` by default, as in acorn. */
	sourceType?: 'script' | 'module';
	/** Parse TypeScript. */
	typescript?: boolean;
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
}

/** Thrown for a syntax error, with acorn's `pos` and `loc`. */
export interface ParseError extends SyntaxError {
	pos: number;
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

/** What a parse at an offset returns. */
export interface Parsed<T> {
	node: T;
	/** The offset after everything the parse consumed: the node, its closing parens and the comments after it. */
	end: number;
	/** Every comment read, in source order; only with `comments`. */
	comments?: Comment[];
}

export interface Params {
	params: Pattern[];
	end: number;
	comments?: Comment[];
}

/** Parses a whole program; with `comments` it lists every comment as `comments`. */
export function parse(source: string, options?: Options): Program & { comments?: Comment[] };
/** Parses one expression starting at `offset`, a UTF-16 offset into `source`. */
export function parseExpressionAt(source: string, offset: number, options?: Options): Parsed<Expression>;
/** Parses an assignment target starting at `offset`. */
export function parsePatternAt(source: string, offset: number, options?: Options): Parsed<Pattern>;
/** Parses a parenthesized parameter list starting at `offset`. */
export function parseParamsAt(source: string, offset: number, options?: Options): Params;
/** Parses one statement starting at `offset`. */
export function parseStatementAt(source: string, offset: number, options?: Options): Parsed<Statement>;

/** Whether a code point can start an identifier, as acorn decides it. */
export function isIdentifierStart(code: number): boolean;
/** Whether a code point can continue an identifier. */
export function isIdentifierChar(code: number): boolean;
