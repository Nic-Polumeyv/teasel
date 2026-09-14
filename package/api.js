import { decode } from './decode.js';

// acorn's option names, which `Request::set` of json.rs takes as they are
const OPTIONS = new Set(['sourceType', 'typescript', 'decorators', 'comments', 'scopes', 'locations', 'parenthesized', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports', 'errorRecovery']);

// `flag` of json.rs by bit
const FLAG = { module: 1, typescript: 2, erase: 4, comments: 8, scopes: 16, locations: 32, parenthesized: 64, legacyDecorators: 128, proposalDecorators: 256, allowReturnOutsideFunction: 512, allowAwaitOutsideFunction: 1024, allowSuperOutsideMethod: 2048, allowUndeclaredExports: 4096, errorRecovery: 8192 };

// the engine takes the options that are on as one word of bits
export function flags(options) {
	if (options === undefined) return 0;
	let on = 0;
	for (const key in options) {
		const value = options[key];
		if (!OPTIONS.has(key)) throw new TypeError(`${key} is not an option`);
		if (value === undefined || value === false) continue;
		if (key === 'decorators') {
			if (value !== 'legacy' && value !== 'proposal') throw new TypeError(`decorators must be "legacy" or "proposal", not ${JSON.stringify(value)}`);
			on |= FLAG[`${value}Decorators`];
		} else if (key === 'sourceType') {
			if (value !== 'script' && value !== 'module') throw new TypeError(`sourceType must be "script" or "module", not ${JSON.stringify(value)}`);
			if (value === 'module') on |= FLAG.module;
		} else if (value === true) on |= FLAG[key];
		else if (key === 'typescript' && value === 'erase') on |= FLAG.typescript | FLAG.erase;
		else throw new TypeError(`${key} must be a boolean, not ${JSON.stringify(value)}`);
	}
	return on;
}

// `Entry` of parser/mod.rs by index
export const ENTRY = { program: 0, expression: 1, pattern: 2, params: 3, statement: 4, typeParameters: 5 };

/**
 * What a parse reads: an entry of the grammar, ended by the host's tokens with `until`, on the
 * source cut with `within`. Immutable: each refinement is a new description.
 */
export class Description {
	#entry;
	#stop;
	#end;
	constructor(entry, stop = '', end = undefined) {
		this.#entry = entry;
		this.#stop = stop;
		this.#end = end;
	}
	/** The same reading, ended where one of the host's own tokens, words or punctuators, follows. */
	until(...tokens) {
		if (tokens.length === 0 || !tokens.every((stop) => typeof stop === 'string' && stop !== '' && !/\s/.test(stop))) {
			throw new TypeError('until takes words and punctuators');
		}
		return new Description(this.#entry, this.#stop === '' ? tokens.join(' ') : `${this.#stop} ${tokens.join(' ')}`, this.#end);
	}
	/** The same reading of the source cut at `end`, a UTF-16 offset; positions stay those of the whole source. */
	within(end) {
		return new Description(this.#entry, this.#stop, end);
	}
	static read(description) {
		return { entry: description.#entry, stop: description.#stop, end: description.#end };
	}
}

export const program = new Description(ENTRY.program);
export const expression = new Description(ENTRY.expression);
/** An assignment target: an identifier or a destructuring pattern. */
export const pattern = new Description(ENTRY.pattern);
/** A parenthesized parameter list, as an arrow function's is read. */
export const params = new Description(ENTRY.params);
export const statement = new Description(ENTRY.statement);
/** A `TSTypeParameterDeclaration`. */
export const typeParameters = new Description(ENTRY.typeParameters);

/**
 * @typedef {ArrayBuffer | Uint32Array | string} Answer
 * @typedef {object} Engine
 * @property {(text: string) => any} plan
 * @property {(source: string, flags: number) => any} create
 * @property {(held: any, entry: number, offset: number, end: number | undefined, stop: string, plan: any) => Answer} parse
 * @property {(held: any) => void} [free]
 * @property {() => string[]} constants
 * @property {() => ArrayLike<number>} shapes
 */

/** @param {Engine} engine */
export function bind(engine) {
	const registry = engine.free && typeof FinalizationRegistry !== 'undefined' ? new FinalizationRegistry(engine.free) : null;

	function result(answer, source) {
		if (typeof answer !== 'string') return decode(answer, source, engine);
		const { message, ...error } = JSON.parse(answer).error;
		throw Object.assign(new SyntaxError(message), error);
	}

	/** @type {WeakMap<Plan, any>} what the engine holds for each plan */
	const plans = new WeakMap();

	class Plan {
		/** @param {string} text the plan as JSON */
		constructor(text) {
			if (typeof text !== 'string') throw new TypeError('a plan is its JSON text');
			plans.set(this, engine.plan(text));
		}
	}

	class Source {
		#held;
		#source;

		constructor(source, options) {
			this.#held = engine.create(source, flags(options));
			this.#source = source;
			registry?.register(this, this.#held, this);
		}

		/**
		 * @param {Description | Plan} [description] what to read: a program by default, or the whole source as a document of a plan
		 * @param {number} [at] where it starts, a UTF-16 offset
		 */
		parse(description = program, at = 0) {
			if (this.#held === undefined) throw new TypeError('the source is freed');
			if (description instanceof Plan) {
				if (at !== 0) throw new TypeError('a plan reads the whole source');
				return result(engine.parse(this.#held, ENTRY.program, 0, undefined, '', plans.get(description)), this.#source);
			}
			if (!(description instanceof Description)) throw new TypeError('parse takes a description: program, expression, pattern, params, statement, typeParameters, or a plan');
			const { entry, stop, end } = Description.read(description);
			return result(engine.parse(this.#held, entry, at, end, stop, undefined), this.#source);
		}

		[Symbol.dispose]() {
			if (this.#held === undefined) return;
			registry?.unregister(this);
			engine.free?.(this.#held);
			this.#held = undefined;
		}
	}
	return { Source, Plan };
}
