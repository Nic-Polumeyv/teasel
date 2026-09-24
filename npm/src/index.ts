import type { Expression, Identifier, Node, Pattern, Program, SourceLocation, Statement } from 'estree';
import { decode, type Held, PARENT, type Prepared, REFERENCE, SCOPE } from './lib/decode.js';
import { ENTRY, flags, type Options } from './lib/options.js';
import { engine } from '#engine';

import type { Code } from './lib/codes.js';

export type { Options } from './lib/options.js';
export type { Code } from './lib/codes.js';

/**
 * Thrown for a syntax error. `code` names what went wrong, for a host to branch on, and
 * `message` says it in words, without a position. An error at the token being read spans it
 * with `pos` and `end`; one reported elsewhere, at a declaration seen earlier say, has `end`
 * equal to `pos`. `unexpected_eof` is the end of what was parsed: the `end` the parse was given,
 * else the end of the source. A bad offset from the host is an `invalid_request` without a `loc`.
 */
export interface ParseError extends SyntaxError {
	code: Code;
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
	/**
	 * What declared it. `function-name` and `class-name` are the name a function expression or a class
	 * expression has inside itself, `const f = function g() {}` declaring `g`. `pattern` is a name that
	 * a `Plan.pattern` piece declares, parsed on its own; a `Plan.params` piece declares `param`s.
	 */
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

const registry = typeof FinalizationRegistry === 'undefined' ? null : new FinalizationRegistry<Held>((held) => held.free());

let read: (plan: Plan<unknown>) => { entry: number; stop: string; held: Held | undefined; whole: boolean };

/**
 * What a parse reads. The built-in plans read a piece of JavaScript at a position of the source,
 * `program` the whole source; `until` ends one where the host's own tokens follow. `new Plan(grammar)`
 * reads the whole source as a document of the host language the grammar describes: the host's
 * own nodes around the JavaScript ones, in one tree, in TypeScript when the grammar says so of a
 * script tag; the grammar's format is at https://teasel.dev/host-grammar. A plan is built once and
 * applied to any source. `T` is what its parse answers with.
 */
export class Plan<T = HostNode> {
	#entry: number;
	#stop: string;
	#held: Held | undefined;

	constructor(grammar: string);
	constructor(grammar: string | number, stop = '') {
		if (typeof grammar === 'number') this.#entry = grammar;
		else {
			if (typeof grammar !== 'string') throw new TypeError('a plan is the grammar as a string');
			this.#entry = ENTRY.program;
			this.#held = engine.plan(grammar);
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
	/** The whole source as a CSS stylesheet: a `StyleSheet` of rules and at-rules, its comments listed. */
	static readonly stylesheet: Plan<HostNode> = Plan.#builtin(ENTRY.stylesheet);

	/** A document and a stylesheet read the whole source, no offset and no `until`. */
	get #whole(): boolean {
		return this.#held !== undefined || this.#entry === ENTRY.stylesheet;
	}

	/**
	 * The same reading, ended where one of the host's own tokens, words or punctuators, follows.
	 * One read outside every bracket the parse opened, where the expression could end, ends it:
	 * `,` ends an expression before a sequence would, and `/>` is never a division. A `then`
	 * after `.` is a property name. A TypeScript `as` is the host's unless another `as` follows
	 * the assertion, so `xs as T[] as item` ends after the type.
	 */
	until(...tokens: string[]): Plan<T> {
		if (this.#whole) throw new TypeError('the plan reads the whole source');
		if (tokens.length === 0 || !tokens.every((token) => typeof token === 'string' && token !== '' && !/\s/.test(token))) {
			throw new TypeError('until takes words and punctuators');
		}
		return Plan.#builtin<T>(this.#entry, this.#stop === '' ? tokens.join(' ') : `${this.#stop} ${tokens.join(' ')}`);
	}

	static {
		read = (plan) => ({ entry: plan.#entry, stop: plan.#stop, held: plan.#held, whole: plan.#whole });
	}
}

/**
 * A source kept with its options: the parses out of it share the source copy and the position
 * tables. Offsets are UTF-16, as in acorn; positions stay those of the whole source.
 */
export class Source {
	#held: Prepared | undefined;
	#source: string;

	constructor(source: string, options: Options = {}) {
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
		const { entry, stop, held, whole } = read(plan);
		let offset: number, end: number | undefined;
		if (typeof at === 'number') offset = at;
		else if (Array.isArray(at) && at.length === 2 && typeof at[0] === 'number' && typeof at[1] === 'number') [offset, end] = at;
		else throw new TypeError('at is an offset or [start, end]');
		if (whole && (offset !== 0 || end !== undefined)) throw new TypeError('the plan reads the whole source');
		const answer = this.#held.parse(entry, offset, end, stop, held);
		if (typeof answer !== 'string') {
			try {
				return decode(answer, this.#source, engine) as Parsed<any>;
			} catch (error) {
				// a tree deeper than the caller's stack has room for overflowed the decoder
				if (!(error instanceof RangeError)) throw error;
				throw Object.assign(new SyntaxError('Maximum nesting depth exceeded'), { code: 'nesting_depth', pos: offset, end: offset });
			}
		}
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
