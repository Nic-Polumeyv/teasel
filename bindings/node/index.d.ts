import type { Expression, Pattern, Program, Statement } from 'estree';

export interface Options {
	/** `module` by default. */
	sourceType?: 'module' | 'script';
	typescript?: boolean;
	/** Attach `leadingComments`, `trailingComments` and `innerComments` to nodes. */
	comments?: boolean;
	preserveParens?: boolean;
	allowReturnOutsideFunction?: boolean;
	allowAwaitOutsideFunction?: boolean;
	allowSuperOutsideMethod?: boolean;
	allowUndeclaredExports?: boolean;
}

/** Thrown for a syntax error, with acorn's `pos` and `loc`. */
export interface ParseError extends SyntaxError {
	pos: number;
	raisedAt: number;
	loc: { line: number; column: number };
}

/** Parses a whole program. */
export function parse(source: string, options?: Options): Program;
/** Parses one expression starting at `offset`, a UTF-16 offset into `source`. */
export function parseExpressionAt(source: string, offset: number, options?: Options): Expression;
/** Parses an assignment target starting at `offset`. */
export function parsePatternAt(source: string, offset: number, options?: Options): Pattern;
/** Parses a parenthesized parameter list starting at `offset`. */
export function parseParamsAt(source: string, offset: number, options?: Options): Pattern[];
/** Parses one statement starting at `offset`. */
export function parseStatementAt(source: string, offset: number, options?: Options): Statement;
