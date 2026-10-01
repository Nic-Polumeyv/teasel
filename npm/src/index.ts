import type { Expression, Node, Pattern, Program, Statement } from 'estree';
import { decode, PARENT, REFERENCE, SCOPE } from './decode.ts';
import type { Code, Held, HostNode, Parsed, Prepared, Reference, Scope } from './types.ts';
import { ENTRY, flags, type Options } from './options.ts';
import { engine } from '#engine';
import type { Answers, Grammar } from './grammar.ts';

export type { Options } from './options.ts';
export type { Arguments, Binding, Code, Comment, Declared, HostNode, Kept, Parsed, Recovered, Reference, Root, Scope, Span } from './types.ts';

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
	declare loc?: { line: number; column: number };

	constructor({ message, ...fields }: Pick<ParseError, 'message' | 'code' | 'pos' | 'end' | 'loc'>) {
		super(message);
		Object.assign(this, fields);
	}
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

const registry = typeof FinalizationRegistry === 'undefined' ? null : new FinalizationRegistry<Held>((held) => held.free());

let read: (plan: Plan<unknown>) => { entry: number; stop: string; held: Held | undefined };

/**
 * What a parse reads. The built-in plans read a piece of JavaScript at a position of the source,
 * `program` the whole source; `until` ends one where the host's own tokens follow. `new Plan(grammar)`
 * reads the whole source as a document of the host language a grammar from `@teasel/parser/grammar`
 * describes: the host's own nodes around the JavaScript ones, in one tree, in TypeScript when the
 * grammar says so of a script tag. A plan is built once and applied to any source. `T` is what its
 * parse answers with.
 */
export class Plan<T = HostNode> {
	#entry: number;
	#stop: string;
	#held: Held | undefined;

	constructor(grammar: Grammar & Answers<T>);
	constructor(grammar: Grammar | number, stop = '') {
		if (typeof grammar === 'number') this.#entry = grammar;
		else {
			if (!(grammar?.wire instanceof Uint8Array)) throw new TypeError('a plan reads a grammar made by @teasel/parser/grammar');
			this.#entry = ENTRY.program;
			this.#held = engine.plan(grammar.wire);
			registry?.register(this, this.#held, this);
		}
		this.#stop = stop;
	}

	// the built-in plans come through the constructor's implementation, which the overload hides
	static #builtin<T>(entry: number, stop = ''): Plan<T> {
		return new (Plan as unknown as new (entry: number, stop: string) => Plan<T>)(entry, stop);
	}

	/** The whole source, or the program inside `[start, end]` of it. */
	static readonly program: Plan<Program> = Plan.#builtin(ENTRY.program);
	static readonly expression: Plan<Expression> = Plan.#builtin(ENTRY.expression);
	/** An assignment target: an identifier or a destructuring pattern. */
	static readonly pattern: Plan<Pattern> = Plan.#builtin(ENTRY.pattern);
	/** A parenthesized parameter list, as an arrow function's is read. */
	static readonly params: Plan<Pattern[]> = Plan.#builtin(ENTRY.params);
	static readonly statement: Plan<Statement> = Plan.#builtin(ENTRY.statement);
	/** A `TSTypeParameterDeclaration`; TypeScript only, `not_typescript` otherwise. */
	static readonly typeParameters: Plan<Node> = Plan.#builtin(ENTRY.typeParameters);

	/**
	 * The same reading, ended where one of the host's own tokens, words or punctuators, follows.
	 * One read outside every bracket the parse opened, where the expression could end, ends it:
	 * `,` ends an expression before a sequence would, and `/>` is never a division. A `then`
	 * after `.` is a property name. A TypeScript `as` is the host's unless another `as` follows
	 * the assertion, so `xs as T[] as item` ends after the type.
	 */
	until(...tokens: string[]): Plan<T> {
		if (this.#held !== undefined) throw new TypeError('a document plan reads the whole source');
		if (tokens.length === 0 || !tokens.every((token) => typeof token === 'string' && token !== '' && !/\s/.test(token))) {
			throw new TypeError('until takes words and punctuators');
		}
		return Plan.#builtin<T>(this.#entry, this.#stop === '' ? tokens.join(' ') : `${this.#stop} ${tokens.join(' ')}`);
	}

	static {
		read = (plan) => ({ entry: plan.#entry, stop: plan.#stop, held: plan.#held });
	}
}

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
		registry?.register(this, this.#held, this);
	}

	/**
	 * What `plan` reads at `at`: the whole source by default; a UTF-16 offset for a piece of
	 * JavaScript, or `[start, end]` for one read as if the source ended at `end`.
	 */
	parse(): Parsed<Program>;
	parse<T>(plan: Plan<T>, at?: number | [start: number, end: number]): Parsed<T>;
	parse(plan: Plan<unknown> = Plan.program, at: number | [number, number] = 0): Parsed<any> {
		if (this.#held === undefined) throw new TypeError('the source is freed');
		if (!(plan instanceof Plan)) throw new TypeError('a parse takes a plan');
		const { entry, stop, held } = read(plan);
		let offset: number, end: number | undefined;
		if (typeof at === 'number') offset = at;
		else if (Array.isArray(at) && at.length === 2 && typeof at[0] === 'number' && typeof at[1] === 'number') [offset, end] = at;
		else throw new TypeError('at is an offset or [start, end]');
		if (held !== undefined && (offset !== 0 || end !== undefined)) throw new TypeError('a document plan reads the whole source');
		const answer = this.#held.parse(entry, offset, end, stop, held);
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

	[Symbol.dispose]() {
		if (this.#held === undefined) return;
		registry?.unregister(this);
		this.#held.free();
		this.#held = undefined;
	}
}
