import { decode } from './decode.js';

// acorn's option names, which `Request::set` of json.rs takes as they are
const OPTIONS = new Set(['sourceType', 'typescript', 'comments', 'scopes', 'locations', 'parenthesized', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports', 'errorRecovery']);

// the engine takes the options that are on as their names
export function names(options) {
	if (options === undefined) return '';
	const on = [];
	for (const key in options) {
		const value = options[key];
		if (!OPTIONS.has(key)) throw new TypeError(`${key} is not an option`);
		if (value === undefined || value === false) continue;
		if (key === 'sourceType') {
			if (value !== 'script' && value !== 'module') throw new TypeError(`sourceType must be "script" or "module", not ${JSON.stringify(value)}`);
			if (value === 'module') on.push('module');
		} else if (value === true) on.push(key);
		else if (key === 'typescript' && value === 'erase') on.push('typescript', 'erase');
		else throw new TypeError(`${key} must be a boolean, not ${JSON.stringify(value)}`);
	}
	return on.join(' ');
}

// `Entry` of parser/mod.rs by index
export const ENTRY = { program: 0, expression: 1, pattern: 2, params: 3, statement: 4, typeParameters: 5 };

// the engine takes the stop tokens as one string
function stops(list) {
	if (list === undefined) return '';
	if (!Array.isArray(list) || !list.every((stop) => typeof stop === 'string' && stop !== '' && !/\s/.test(stop))) {
		throw new TypeError('stopAt must be a list of words and punctuators');
	}
	return list.join(' ');
}

/**
 * @typedef {ArrayBuffer | Uint32Array | string} Answer
 * @typedef {object} Engine
 * @property {(source: string, names: string) => any} create
 * @property {(held: any, entry: number, offset: number, end: number | undefined, stop: string) => Answer} parse
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

		constructor(source, options) {
			this.#held = engine.create(source, names(options));
			this.#source = source;
			registry?.register(this, this.#held, this);
		}

		/**
		 * @param {keyof typeof ENTRY} [entry] what to read
		 * @param {number} [offset] where it starts
		 * @param {{ end?: number, stopAt?: string[] }} [at] where the source is cut, and the host's tokens that end the parse
		 */
		parse(entry = 'program', offset = 0, { end, stopAt } = {}) {
			if (this.#held === undefined) throw new TypeError('the source is freed');
			const index = ENTRY[entry];
			if (index === undefined) throw new TypeError(`${JSON.stringify(entry)} is not an entry`);
			return result(engine.parse(this.#held, index, offset, end, stops(stopAt)), this.#source);
		}

		free() {
			if (this.#held === undefined) return;
			registry?.unregister(this);
			engine.free?.(this.#held);
			this.#held = undefined;
		}
	};
}
