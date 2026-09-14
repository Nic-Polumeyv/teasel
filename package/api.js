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
		if (key === 'host') {
			if (value !== undefined && typeof value !== 'string') throw new TypeError('host must be the grammar as a string');
			continue;
		}
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

// the engine registers a stop set once, from its tokens as one string
function stops(list) {
	if (!Array.isArray(list) || !list.every((stop) => typeof stop === 'string' && stop !== '' && !/\s/.test(stop))) {
		throw new TypeError('stopAt must be a list of words and punctuators');
	}
	return list.join(' ');
}

/**
 * @typedef {ArrayBuffer | Uint32Array | string} Answer
 * @typedef {object} Engine
 * @property {(source: string, flags: number, host: string) => any} create
 * @property {(held: any, tokens: string) => number} stops
 * @property {(held: any, entry: number, offset: number, end: number | undefined, stop: number) => Answer} parse
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

	return class Source {
		#held;
		#source;
		#options;
		/** stop set ids by the list given, so a list a host keeps crosses once @type {WeakMap<string[], number>} */
		#stops = new WeakMap();

		constructor(source, options) {
			this.#held = engine.create(source, flags(options), options?.host ?? '');
			this.#source = source;
			// what the engine was prepared with, however the caller's object changes after
			this.#options = { ...options };
			registry?.register(this, this.#held, this);
		}

		/**
		 * @param {keyof typeof ENTRY} [entry] what to read
		 * @param {number} [offset] where it starts
		 * @param {{ end?: number, stopAt?: string[] }} [at] where the source is cut, and the host's tokens that end the parse
		 */
		parse(entry = 'program', offset = 0, { end, stopAt } = {}) {
			if (this.#held === undefined) throw new TypeError('the source is freed');
			const index = Object.hasOwn(ENTRY, entry) ? ENTRY[entry] : undefined;
			if (index === undefined) throw new TypeError(`${JSON.stringify(entry)} is not an entry`);
			const stop = this.#stop(stopAt);
			if (this.#options.host !== undefined && index === ENTRY.program) return result(engine.parse(this.#held, index, 0, undefined, 0), this.#source);
			return result(engine.parse(this.#held, index, offset, end, stop), this.#source);
		}

		#stop(list) {
			if (list === undefined) return 0;
			let id = this.#stops.get(list);
			if (id === undefined) {
				id = engine.stops(this.#held, stops(list));
				this.#stops.set(list, id);
			}
			return id;
		}

		[Symbol.dispose]() {
			if (this.#held === undefined) return;
			registry?.unregister(this);
			engine.free?.(this.#held);
			this.#held = undefined;
		}
	};
}
