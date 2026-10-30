import type { Comment, Expression, Node, Pattern, Program, Statement } from 'estree';
import { decode, PARENT, REFERENCE, SCOPE } from './decode.ts';
import { LANGUAGE, PIECE, type answers, type HostNode, type Language, type Parsed, type Recovered, type Reference, type Scope } from './types.ts';
import { flags, type Options } from './options.ts';
import { engine } from '#engine';
import { children, extras, stylesheet } from './children.ts';

export type { Options } from './options.ts';
export type { Arguments, Binding, Code, Comment, Declared, HostNode, Kept, Language, Parsed, Recovered, Reference, Root, Scope, Span } from './types.ts';

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

/** One piece of JavaScript, what `parseAt` reads. A piece is built once and read in any text. `T` is what its parse answers with. */
export interface Piece<T> {
	// `T` is read nowhere else, so without this any piece would pass for any other
	readonly [answers]?: T;
	readonly [PIECE]: { readonly entry: number; readonly stop: string };
	/**
	 * The same piece, ended where one of the host's own tokens, words or punctuators, follows.
	 * One read outside every bracket the parse opened, where the expression could end, ends it:
	 * `,` ends an expression before a sequence would, and `/>` is never a division. A `then`
	 * after `.` is a property name. A TypeScript `as` is the host's unless another `as` follows
	 * the assertion, so `xs as T[] as item` ends after the type. A program reads to the end of
	 * the text and takes no tokens.
	 */
	until(...tokens: string[]): Piece<T>;
}

// `Entry` of parser/mod.rs by index; the program is 0
const piece = <T>(entry: number, stop = ''): Piece<T> =>
	Object.freeze({
		[PIECE]: { entry, stop },
		until(...tokens: string[]) {
			if (entry === 0) throw new TypeError('a program reads to the end of the text');
			if (tokens.length === 0 || !tokens.every((token) => typeof token === 'string' && token !== '' && !/\s/.test(token))) {
				throw new TypeError('until takes words and punctuators');
			}
			return piece<T>(entry, stop === '' ? tokens.join(' ') : `${stop} ${tokens.join(' ')}`);
		},
	});

/** JavaScript: a parse of it reads the whole text as a program. Its members are pieces for `parseAt`. */
export const js: Language<Program> & {
	/** A program from the offset `parseAt` is given to the end of the text. */
	readonly program: Piece<Program>;
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
	[LANGUAGE]: { entry: 0 },
	children,
	extras,
	program: piece<Program>(0),
	expression: piece<Expression>(1),
	pattern: piece<Pattern>(2),
	params: piece<Pattern[]>(3),
	statement: piece<Statement>(4),
	typeParameters: piece<Node>(5),
});

/** CSS: a parse of it reads the whole text as a stylesheet, a `StyleSheet` of rules and at-rules with its comments listed. */
export const css: Language<HostNode> = Object.freeze({ [LANGUAGE]: { entry: 6 }, children: stylesheet });

/**
 * The whole of `text`, as `language` reads it: `js` by default, a program. `css` reads it as a
 * stylesheet, and a grammar from `@teasel/parser/grammar` as a document of its host language: the
 * host's own nodes around the JavaScript ones, in one tree, in TypeScript when the grammar says so
 * of a script tag. Positions are UTF-16 offsets. A lone surrogate in the text reaches the parser as
 * U+FFFD, the way `TextEncoder` encodes it, so string values, template cooked and raw text and
 * regex patterns carry U+FFFD there while spans and `loc` are right.
 */
export function parse<T = Program>(text: string, options?: Options, language: Language<T> = js as Language<never>): Parsed<T> {
	const read = language?.[LANGUAGE];
	if (read === undefined) throw new TypeError('parse takes `js`, `css`, or a grammar made by this copy of @teasel/parser/grammar; a piece goes to parseAt');
	if (typeof text !== 'string') throw new TypeError('parse takes a string');
	const on = flags(options);
	return answer(engine.parse(text, on, read.entry, 0, '', read.plan), text, on, 0, false);
}

/**
 * `piece` read from `text` at `at`, a UTF-16 offset, with positions of the whole text; a piece
 * inside a range is read from the text cut at the range's end, as
 * `parseAt(text.slice(0, end), js.program, start)`. Every call copies the text into the parser,
 * however small the piece, so reading one file piece by piece costs a copy of the file per piece:
 * the parser answers a text, it does not hold a conversation about one. For a document of a host
 * language, describe it as a grammar and `parse` it once.
 */
export function parseAt<P extends Piece<unknown>>(text: string, piece: P, at: number, options?: Options): Parsed<P extends Piece<infer T> ? T : never> {
	const read = piece?.[PIECE];
	if (read === undefined) throw new TypeError('parseAt takes a piece of `js`, such as js.program or js.expression');
	if (typeof text !== 'string') throw new TypeError('parseAt takes a string');
	if (!Number.isInteger(at) || at < 0 || at > text.length || (at > 0 && text.codePointAt(at - 1)! > 0xffff)) throw new RangeError(`${at} is no offset between characters of the text`);
	const on = flags(options);
	return answer(engine.parse(text, on, read.entry, at, read.stop, undefined), text, on, at, read.entry === 3);
}

function answer<T>(words: Uint32Array, text: string, on: number, offset: number, listed: boolean): Parsed<T> {
	let error: Omit<Recovered, 'line' | 'column'>;
	try {
		const parsed = decode(words, text, engine, on, listed) as Parsed<T>;
		if (parsed.node !== null) return parsed;
		error = parsed.errors![0];
	} catch (thrown) {
		// a tree deeper than the caller's stack has room for overflowed the decoder
		if (!(thrown instanceof RangeError)) throw thrown;
		error = { code: 'nesting_depth', args: [], pos: offset, end: offset };
	}
	throw Object.assign(new SyntaxError(error.args.reduce((text: string, arg) => `${text} '${arg}'`, error.code)), error);
}
