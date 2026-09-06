import { decode } from './decode.js';

// bit i is `FLAGS[i]` of json.rs
const FLAGS = ['typescript', 'comments', 'scopes', 'locations', 'script', 'preserveParens', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports', 'untilAs', 'erase'];
const BIT = Object.fromEntries(FLAGS.map((flag, i) => [flag, 1 << i]));
const DERIVED = new Set(['script', 'untilAs']);

export function bits(options) {
	if (options === undefined) return BIT.script;
	for (const key of ['ranges', 'onComment', 'onToken', 'onInsertedSemicolon', 'onTrailingComma']) {
		if (key in options) throw new TypeError(`the ${key} option is not supported`);
	}
	if ('sourceType' in options && options.sourceType !== 'script' && options.sourceType !== 'module') {
		throw new TypeError(`sourceType must be "script" or "module", not ${JSON.stringify(options.sourceType)}`);
	}
	if ('until' in options && options.until !== 'as') {
		throw new TypeError(`until must be "as", not ${JSON.stringify(options.until)}`);
	}
	let on = options.sourceType === 'module' ? 0 : BIT.script;
	if (options.until === 'as') on |= BIT.untilAs;
	for (const flag of FLAGS) {
		const value = options[flag];
		if (value === undefined || value === false || DERIVED.has(flag)) continue;
		if (value === true) on |= BIT[flag];
		else if (flag === 'typescript' && value === 'erase') on |= BIT.typescript | BIT.erase;
		else throw new TypeError(`${flag} must be a boolean, not ${JSON.stringify(value)}`);
	}
	return on;
}

// `json::Entry` by index
const ENTRY = { program: 0, expression: 1, pattern: 2, params: 3, statement: 4 };

/**
 * @typedef {ArrayBuffer | Uint32Array | string} Answer
 * @typedef {object} Engine
 * @property {(source: string, bits: number, entry: number, offset: number, until: boolean) => Answer} once
 * @property {(source: string, bits: number) => any} create
 * @property {(held: any, entry: number, offset: number, until: boolean) => Answer} parse
 * @property {(held: any, start: number, end: number | undefined) => Answer} parseRange
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

	const once = (source, options, entry, offset, until = false) => result(engine.once(source, bits(options), entry, offset, until), source);

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

		parse(start, end) {
			if (start === undefined && end === undefined) return this.#at(ENTRY.program, 0);
			if (this.#held === undefined) throw new TypeError('the source is freed');
			return result(engine.parseRange(this.#held, start ?? 0, end), this.#source);
		}

		/** @param {number} offset @param {'as'} [until] */
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
