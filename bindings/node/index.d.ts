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

export interface Params {
	params: Pattern[];
	/** The offset after the closing paren. */
	end: number;
}

/** Parses a whole program. */
export function parse(source: string, options?: Options): Program;
/** Parses one expression starting at `offset`, a UTF-16 offset into `source`. */
export function parseExpressionAt(source: string, offset: number, options?: Options): Expression;
/** Parses an assignment target starting at `offset`. */
export function parsePatternAt(source: string, offset: number, options?: Options): Pattern;
/** Parses a parenthesized parameter list starting at `offset`. */
export function parseParamsAt(source: string, offset: number, options?: Options): Params;
/** Parses one statement starting at `offset`. */
export function parseStatementAt(source: string, offset: number, options?: Options): Statement;
