import type { Comment, Expression, Node, Pattern, Position, Program, Statement } from 'estree';
import { decode, PARENT, REFERENCE, SCOPE } from './decode.ts';
import type { Code, HostNode, Language, Parsed, Prepared, Reference, Scope } from './types.ts';
import { flags, type Options } from './options.ts';
import { engine } from '#engine';
import { children, extras, frozen } from './children.ts';
import { compiled } from './held.ts';
import type { Grammar } from './grammar.ts';

export type { Options } from './options.ts';
export type { Arguments, Binding, Code, Comment, Declared, HostNode, Kept, Language, Parsed, Recovered, Reference, Root, Scope, Span } from './types.ts';

/**
 * Thrown for a syntax error. `code` names what went wrong, for a host to branch on, and
 * `message` says it in words, without a position. An error at the token being read spans it
 * with `pos` and `end`; one reported elsewhere, at a declaration seen earlier say, has `end`
 * equal to `pos`. `unexpected_eof` is the end of what was parsed: the `end` the parse was given,
 * else the end of the source. A bad offset from the host is an `invalid_request` without a `loc`.
 */
export class ParseError extends SyntaxError {
	declare code: Code;
	declare pos: number;
	declare end: number;
	declare loc?: Position;

	constructor({ message, ...fields }: Pick<ParseError, 'message' | 'code' | 'pos' | 'end' | 'loc'>) {
		super(message);
		Object.assign(this, fields);
	}
}

// what the decoder hangs on a node, under keys JSON and enumeration skip
const slot = (node: object | null | undefined, key: symbol) => (node as Record<symbol, any> | null | undefined)?.[key];

/** The node `node` is a child of; undefined for the root of an answer and for a comment listed in `comments`. A literal's `regex` and a template element's `value` are not nodes and have none. */
export function parentOf(node: Node | HostNode | Comment | null | undefined): Node | HostNode | undefined {
	return slot(node, PARENT);
}
/** With `scopes`: the scope `node` opens, when it opens one. */
export function scopeOf(node: Node | HostNode | null | undefined): Scope | undefined {
	return slot(node, SCOPE);
}
/** With `scopes`: the reference an identifier makes, the binding itself for the identifier that declares it; a global's too, which no binding lists. Undefined when the identifier names no value, a property key say. */
export function referenceOf(node: Node | null | undefined): Reference | undefined {
	return slot(node, REFERENCE);
}

let read: (piece: Piece<unknown>) => { entry: number; stop: string };
let piece: <T>(entry: number) => Piece<T>;

/**
 * One piece of JavaScript read at a position of the source, as far as it goes; `until` ends it
 * where the host's own tokens follow. A piece is built once and read at any position of any
 * source. `T` is what its parse answers with.
 */
export class Piece<T> {
	#entry: number;
	#stop: string;

	private constructor(entry: number, stop = '') {
		this.#entry = entry;
		this.#stop = stop;
	}

	/**
	 * The same piece, ended where one of the host's own tokens, words or punctuators, follows.
	 * One read outside every bracket the parse opened, where the expression could end, ends it:
	 * `,` ends an expression before a sequence would, and `/>` is never a division. A `then`
	 * after `.` is a property name. A TypeScript `as` is the host's unless another `as` follows
	 * the assertion, so `xs as T[] as item` ends after the type.
	 */
	until(...tokens: string[]): Piece<T> {
		if (tokens.length === 0 || !tokens.every((token) => typeof token === 'string' && token !== '' && !/\s/.test(token))) {
			throw new TypeError('until takes words and punctuators');
		}
		return new Piece(this.#entry, this.#stop === '' ? tokens.join(' ') : `${this.#stop} ${tokens.join(' ')}`);
	}

	static {
		read = (piece) => ({ entry: piece.#entry, stop: piece.#stop });
		piece = (entry) => new Piece(entry);
	}
}

// `Entry` of parser/mod.rs by index; the program is 0
/** JavaScript: a parse of it reads the whole source, or the program inside `[start, end]`. Its members read one piece. */
export const js: Language<Program> & {
	readonly expression: Piece<Expression>;
	/** An assignment target: an identifier or a destructuring pattern. */
	readonly pattern: Piece<Pattern>;
	/** A parenthesized parameter list, as an arrow function's is read. */
	readonly params: Piece<Pattern[]>;
	readonly statement: Piece<Statement>;
	/** A `TSTypeParameterDeclaration`; TypeScript only, `not_typescript` otherwise. */
	readonly typeParameters: Piece<Node>;
	/** The fields TypeScript may add to a node of any type, holding nodes: annotations, type parameters and arguments, what a class implements, decorators. */
	readonly extras: readonly string[];
} = Object.freeze({
	children,
	extras,
	expression: piece<Expression>(1),
	pattern: piece<Pattern>(2),
	params: piece<Pattern[]>(3),
	statement: piece<Statement>(4),
	typeParameters: piece<Node>(5),
});

/** CSS: a parse of it reads the whole source as a stylesheet, a `StyleSheet` of rules and at-rules with its comments listed. */
export const css: Language<HostNode> = Object.freeze({
	get children() {
		return (sheet ??= frozen(JSON.parse(engine.children(undefined))));
	},
});
let sheet: Language<unknown>['children'] | undefined;

/**
 * A source kept with its options: the parses out of it share the source copy and the position
 * tables. Offsets are UTF-16; positions stay those of the whole source.
 */
export class Source {
	#held: Prepared | undefined;
	#source: string;

	constructor(source: string, options: Options = {}) {
		// Rust cannot read a V8 string, so it parses its own copy
		this.#held = engine.create(source, flags(options));
		this.#source = source;
	}

	/**
	 * What `what` answers with: `js` by default, the whole source as a program. A piece of
	 * JavaScript is read at `at`, a UTF-16 offset, or at `[start, end]` as if the source ended at
	 * `end`; `js` takes the same for the program inside a range. `css` reads the whole source as a
	 * stylesheet. A grammar from
	 * `@teasel/parser/grammar` reads the whole source as a document of its host language: the
	 * host's own nodes around the JavaScript ones, in one tree, in TypeScript when the grammar
	 * says so of a script tag. The engine reads a grammar on its first parse and keeps it while
	 * the grammar lives, so a grammar is made once.
	 */
	parse(): Parsed<Program>;
	parse<T>(piece: Piece<T>, at?: number | [start: number, end: number]): Parsed<T>;
	parse(language: typeof js, at?: number | [start: number, end: number]): Parsed<Program>;
	parse<T>(what: Piece<T> | typeof js, at?: number | [start: number, end: number]): Parsed<T | Program>;
	parse<T>(language: Language<T>): Parsed<T>;
	parse(what: Piece<unknown> | Language<unknown> = js, at?: number | [number, number]): Parsed<any> {
		if (this.#held === undefined) throw new TypeError('the source is freed');
		let entry = 0, stop = '', grammar: object | undefined, offset = 0, end: number | undefined;
		if (what === js || what instanceof Piece) {
			if (what !== js) ({ entry, stop } = read(what as Piece<unknown>));
			if (typeof at === 'number') offset = at;
			else if (Array.isArray(at) && at.length === 2 && typeof at[0] === 'number' && typeof at[1] === 'number') [offset, end] = at;
			else if (at !== undefined) throw new TypeError('at is an offset or [start, end]');
		} else {
			if (what === css) entry = 6;
			else grammar = compiled(what as Grammar);
			if (at !== undefined) throw new TypeError('only `js` and its pieces take a position');
		}
		const answer = this.#held.parse(entry, offset, end, stop, grammar);
		if (typeof answer !== 'string') {
			try {
				return decode(answer, this.#source, engine) as Parsed<any>;
			} catch (error) {
				// a tree deeper than the caller's stack has room for overflowed the decoder
				if (!(error instanceof RangeError)) throw error;
				throw new ParseError({ message: 'Maximum nesting depth exceeded', code: 'nesting_depth', pos: offset, end: offset });
			}
		}
		throw new ParseError(JSON.parse(answer).error);
	}

	// the engine frees an undisposed source when it is collected
	[Symbol.dispose]() {
		this.#held?.free();
		this.#held = undefined;
	}
}
