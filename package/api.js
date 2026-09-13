import { decode, facts } from './decode.js';
import { isIdentifierStart, isIdentifierChar } from './identifier.js';

// acorn's option names, which `Request::set` of json.rs takes as they are
const OPTIONS = new Set(['sourceType', 'typescript', 'decorators', 'comments', 'scopes', 'locations', 'parenthesized', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports', 'errorRecovery']);

// the engine takes the options that are on as their names
export function names(options) {
	if (options === undefined) return '';
	const on = [];
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
			on.push(`${value}Decorators`);
		} else if (key === 'sourceType') {
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
 * @property {(source: string, names: string, host: string) => any} create
 * @property {(held: any, entry: number, offset: number, end: number | undefined, stop: string) => Answer} parse
 * @property {(held: any) => void} [free]
 * @property {() => string[]} constants
 * @property {() => ArrayLike<number>} shapes
 */

// words the engine has to judge: keywords, the strict-mode reserved words, the contextual ones,
// and the two names strict mode refuses to bind
const KEYWORD = new Set('arguments await break case catch class const continue debugger default delete do else enum eval export extends false finally for function if implements import in instanceof interface let new null package private protected public return static super switch this throw true try typeof var void while with yield'.split(' '));

/**
 * The offset after an identifier the host's syntax follows directly, so the answer needs no
 * engine: a name, then optional space, then an unmatched closer, the cut, or one of `stopAt`.
 * Anything the grammar could continue with, a dot or an operator or a TypeScript `as`, and any
 * word the engine has to judge, is left to it.
 * @param {string} source @param {number} at @param {number} end @param {string[] | undefined} stopAt
 * @returns {[number, number] | null} the identifier's end and where the parse ends
 */
function bare(source, at, end, stopAt, typescript) {
	if (at >= end) return null;
	let i = at;
	const first = source.codePointAt(i);
	if (first === undefined || !isIdentifierStart(first) || first === 0x5c) return null;
	i += first > 0xffff ? 2 : 1;
	if (i > end) return null;
	while (i < end) {
		const code = /** @type {number} */ (source.codePointAt(i));
		if (code === 0x5c) return null;
		if (!isIdentifierChar(code)) break;
		i += code > 0xffff ? 2 : 1;
	}
	const name_end = i;
	if (KEYWORD.has(source.slice(at, name_end))) return null;
	while (i < end && /\s/.test(source[i])) i++;
	if (i === end) return [name_end, name_end];
	const c = source[i];
	if (c === '}' || c === ')' || c === ']') return [name_end, name_end];
	if (stopAt !== undefined) {
		for (const stop of stopAt) {
			if (!source.startsWith(stop, i)) continue;
			const after = source.codePointAt(i + stop.length);
			if (isIdentifierStart(/** @type {number} */ (stop.codePointAt(0))) && after !== undefined && isIdentifierChar(after)) continue;
			// a stop TypeScript reads as its own is the engine's to judge
			if (typescript && (stop === 'as' || stop === 'satisfies' || stop === ':')) return null;
			return [name_end, name_end];
		}
	}
	return null;
}

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
		/** @type {number[] | undefined} */
		#lines;

		constructor(source, options) {
			this.#held = engine.create(source, names(options), options?.host ?? '');
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
			const stop = stops(stopAt);
			if (this.#options.host !== undefined && index === ENTRY.program) return result(engine.parse(this.#held, index, 0, undefined, ''), this.#source);
			if ((index === ENTRY.expression || index === ENTRY.pattern) && Number.isInteger(offset) && offset >= 0) {
				const cut = end === undefined ? this.#source.length : end;
				const found = Number.isInteger(cut) && cut <= this.#source.length && offset <= cut ? bare(this.#source, offset, cut, index === ENTRY.pattern ? [',', '(', ':', '='] : stopAt, !!this.#options.typescript) : null;
				if (found !== null) return this.#identifier(offset, found[0], index === ENTRY.pattern);
			}
			return result(engine.parse(this.#held, index, offset, end, stop), this.#source);
		}

		/** The answer the engine would give for a bare identifier, built here. */
		#identifier(start, end, pattern) {
			const o = this.#options;
			/** @type {any} */
			const node = { type: 'Identifier', start, end, name: this.#source.slice(start, end) };
			if (o.locations) node.loc = { start: this.#position(start), end: this.#position(end) };
			/** @type {any} */
			const answer = { node, end };
			if (o.comments) answer.comments = [];
			if (o.errorRecovery) answer.errors = [];
			if (o.typescript === 'erase') answer.typescript = [];
			if (o.scopes) {
				const scope = { kind: 'fragment', parent: null, topLevelAwait: false, node };
				const binding = pattern ? { name: node.name, kind: 'pattern', scope, node, declaration: null } : null;
				const reference = pattern ? null : { scope, binding: null, write: false, read: true, mutate: false, node, writeExpr: null };
				answer.scopes = [scope];
				answer.bindings = binding === null ? [] : [binding];
				answer.references = reference === null ? [] : [reference];
				facts(node, scope, binding, reference);
			} else {
				facts(node, undefined, undefined, undefined);
			}
			return answer;
		}

		/** Line and column of an offset, from a line table built on first use. */
		#position(offset) {
			if (this.#lines === undefined) {
				this.#lines = [0];
				for (const m of this.#source.matchAll(/\r\n?|[\n\u2028\u2029]/g)) this.#lines.push(m.index + m[0].length);
			}
			let lo = 0, hi = this.#lines.length - 1;
			while (lo < hi) {
				const mid = (lo + hi + 1) >> 1;
				if (this.#lines[mid] <= offset) lo = mid;
				else hi = mid - 1;
			}
			return { line: lo + 1, column: offset - this.#lines[lo] };
		}

		[Symbol.dispose]() {
			if (this.#held === undefined) return;
			registry?.unregister(this);
			engine.free?.(this.#held);
			this.#held = undefined;
		}
	};
}
