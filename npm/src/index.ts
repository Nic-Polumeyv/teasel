import type { Expression, Identifier, Node, Pattern, Program, SourceLocation, Statement } from 'estree';
import { decode, type Engine, PARENT, type Prepared, REFERENCE, SCOPE } from './lib/decode.js';
import { ENTRY, type Entry, flags, type Options } from './lib/options.js';
import { engine } from '#engine';

export type { Entry, Options } from './lib/options.js';

declare global {
	interface SymbolConstructor {
		readonly dispose: unique symbol;
	}
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

/** A binding, as one of `bindings` on the answer: one an identifier declares, or the `arguments` a function reads. */
export type Binding = Declared | Arguments;

/** A binding an identifier declares. It is the reference that identifier makes, the first of its own: `referenceOf` answers with it, and its `binding` is itself. */
export interface Declared extends Reference {
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

// what the decoder hangs on a node, under keys JSON and enumeration skip
interface Linked {
	[PARENT]?: Node;
	[SCOPE]?: Scope;
	[REFERENCE]?: Reference;
}

/** The node `node` is a child of; undefined for the root of an answer. A literal's `regex` and a template element's `value` are not nodes and have none. */
export function parentOf(node: Node | null | undefined): Node | undefined {
	return node == null ? undefined : (node as Linked)[PARENT];
}
/** With `scopes`: the scope `node` opens, when it opens one. */
export function scopeOf(node: Node | null | undefined): Scope | undefined {
	return node == null ? undefined : (node as Linked)[SCOPE];
}
/** With `scopes`: the reference an identifier makes, the binding itself for the identifier that declares it; a global's too, which no binding lists. Undefined when the identifier names no value, a property key say. */
export function referenceOf(node: Node | null | undefined): Reference | undefined {
	return node == null ? undefined : (node as Linked)[REFERENCE];
}

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
	/** With `scopes`, for a document read by a host grammar: its pieces of JavaScript in source order. */
	roots?: Root[];
}

/**
 * What a parse reads, and what it answers with: a program, or what a host embedding JavaScript in
 * a larger syntax reads at a point of it. `Root` is the program, or the document's root with a
 * `host`.
 */
export interface Answer<Root> {
	program: Root;
	expression: Expression;
	/** An assignment target: an identifier or a destructuring pattern. */
	pattern: Pattern;
	/** A parenthesized parameter list, as an arrow function's is read. */
	params: Pattern[];
	statement: Statement;
	/** A `TSTypeParameterDeclaration`; TypeScript only, `not_typescript` otherwise. */
	typeParameters: Node;
}

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

// the engine takes the stop tokens as one string
function stops(list: string[] = []) {
	if (!Array.isArray(list) || !list.every((stop) => typeof stop === 'string' && stop !== '' && !/\s/.test(stop))) {
		throw new TypeError('stopAt must be a list of words and punctuators');
	}
	return list.join(' ');
}

const registry = typeof FinalizationRegistry === 'undefined' ? null : new FinalizationRegistry<Prepared>((held) => held.free());

/**
 * A source kept with its options: the parses out of it share the source copy and the position
 * tables. Offsets are UTF-16, as in acorn; positions stay those of the whole source. `Root` is
 * what the program entry answers with: the program, or the document's root with a `host`.
 */
export class Source<Root = Program> {
	#held: Prepared | undefined;
	#source: string;
	#options: Options;

	constructor(source: string, options: Options = {}) {
		this.#held = engine.create(source, flags(options), options.host ?? '');
		this.#source = source;
		// what the engine was prepared with, however the caller's object changes after
		this.#options = { ...options };
		registry?.register(this, this.#held, this);
	}

	/** What `entry` reads at `offset`: the program, the whole source, by default; the document with a `host`. Every other entry needs its offset. */
	parse<E extends Entry = 'program'>(entry?: E, ...rest: E extends 'program' ? [offset?: number, at?: At] : [offset: number, at?: At]): Parsed<Answer<Root>[E]>;
	parse(entry: Entry = 'program', offset = 0, { end, stopAt }: At = {}): Parsed<any> {
		if (this.#held === undefined) throw new TypeError('the source is freed');
		if (!Object.hasOwn(ENTRY, entry)) throw new TypeError(`${JSON.stringify(entry)} is not an entry`);
		const index = ENTRY[entry];
		const stop = stops(stopAt);
		const answer = this.#options.host !== undefined && index === ENTRY.program ? this.#held.parse(index, 0, undefined, '') : this.#held.parse(index, offset, end, stop);
		if (typeof answer !== 'string') return decode(answer, this.#source, engine) as Parsed<any>;
		const { message, ...error } = JSON.parse(answer).error;
		throw Object.assign(new SyntaxError(message), error);
	}

	/** Releases what the engine holds for the source, as `using` does at the end of its block; the collector does it otherwise. */
	[Symbol.dispose]() {
		if (this.#held === undefined) return;
		registry?.unregister(this);
		this.#held.free();
		this.#held = undefined;
	}
}
