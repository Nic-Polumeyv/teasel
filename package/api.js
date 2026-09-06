// The package's API over an engine, the Node addon or the WebAssembly module. An engine parses
// one entry of a source at an offset and answers the packed token stream `decode.js` reads, or
// the error as JSON text.
import { decode } from './decode.js';
import { bits } from './options.js';

/** `json::Entry` by index. */
const ENTRY = { program: 0, expression: 1, pattern: 2, params: 3, statement: 4 };

/**
 * @typedef {ArrayBuffer | Uint32Array | string} Answer the words, or the error as JSON
 * @typedef {object} Engine
 * @property {(source: string, bits: number, entry: number, offset: number, until: boolean) => Answer} once one parse of a source
 * @property {(source: string, bits: number) => any} create a source held for many parses
 * @property {(held: any, entry: number, offset: number, until: boolean) => Answer} parse
 * @property {(held: any, start: number, end: number | undefined) => Answer} parseRange
 * @property {(held: any) => void} [free] when the engine holds memory the collector cannot see
 * @property {() => string[]} constants the strings the engine has numbered so far
 */

/** @param {Engine} engine */
export function bind(engine) {
	const registry = engine.free && typeof FinalizationRegistry !== 'undefined' ? new FinalizationRegistry(engine.free) : null;

	/** The answer as a tree, or thrown as the `SyntaxError` it describes. */
	function result(answer, source) {
		if (typeof answer !== 'string') return decode(answer, source, engine.constants);
		const { message, ...error } = JSON.parse(answer).error;
		throw Object.assign(new SyntaxError(message), error);
	}

	const once = (source, options, entry, offset, until = false) => result(engine.once(source, bits(options), entry, offset, until), source);

	/** A source kept with its options, so the parses out of it share its copy and its tables. */
	class Source {
		#held;
		#source;

		constructor(source, options) {
			this.#held = engine.create(source, bits(options));
			this.#source = source;
			registry?.register(this, this.#held, this);
		}

		#at(entry, offset, until = false) {
			if (this.#held === undefined) throw new TypeError('the source is freed');
			return result(engine.parse(this.#held, entry, offset, until), this.#source);
		}

		/** The whole source, or the program spanning `start..end` of it. */
		parse(start, end) {
			if (start === undefined && end === undefined) return this.#at(ENTRY.program, 0);
			if (this.#held === undefined) throw new TypeError('the source is freed');
			return result(engine.parseRange(this.#held, start ?? 0, end), this.#source);
		}

		/** @param {number} offset @param {'as'} [until] when the host's `as` follows the expression */
		parseExpressionAt(offset, until) {
			return this.#at(ENTRY.expression, offset, until === 'as');
		}

		parsePatternAt(offset) {
			return this.#at(ENTRY.pattern, offset);
		}

		parseParamsAt(offset) {
			return this.#at(ENTRY.params, offset);
		}

		parseStatementAt(offset) {
			return this.#at(ENTRY.statement, offset);
		}

		/** Frees what the engine holds for the source; the collector does it otherwise. */
		free() {
			if (this.#held === undefined) return;
			registry?.unregister(this);
			engine.free?.(this.#held);
			this.#held = undefined;
		}
	}

	return {
		parse: (source, options) => once(source, options, ENTRY.program, 0),
		parseExpressionAt: (source, offset, options) => once(source, options, ENTRY.expression, offset, options?.until === 'as'),
		parsePatternAt: (source, offset, options) => once(source, options, ENTRY.pattern, offset),
		parseParamsAt: (source, offset, options) => once(source, options, ENTRY.params, offset),
		parseStatementAt: (source, offset, options) => once(source, options, ENTRY.statement, offset),
		Source,
	};
}
