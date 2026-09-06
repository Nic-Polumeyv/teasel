// The same API as `@teasel/parser` over the WebAssembly module, loaded by `init` first. The
// module has no glue: the source goes into its memory once, and every answer is decoded straight
// out of it, the packed token stream `decode.js` reads for the Node addon too.
import { decode } from '../node/decode.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';

const FLAGS = ['typescript', 'erase', 'comments', 'scopes', 'locations', 'preserveParens', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports'];
const ENTRY = { program: 0, expression: 1, pattern: 2, params: 3, statement: 4 };

const encoder = new TextEncoder();
const utf8 = new TextDecoder();

/** @type {WebAssembly.Exports & Record<string, Function> & { memory: WebAssembly.Memory }} */
let wasm;

/**
 * Instantiates the module: the bytes, a compiled `WebAssembly.Module`, or a `Response` (or a
 * promise of one) to stream it from.
 * @param {BufferSource | WebAssembly.Module | Response | Promise<Response>} module
 */
export async function init(module) {
	if (module instanceof Promise) module = await module;
	const { instance } =
		typeof Response !== 'undefined' && module instanceof Response
			? await WebAssembly.instantiateStreaming(module, {})
			: await WebAssembly.instantiate(module, {});
	wasm = /** @type {any} */ (instance.exports);
}

function flags(options = {}) {
	for (const key of ['ranges', 'onComment', 'onToken', 'onInsertedSemicolon', 'onTrailingComma']) {
		if (key in options) throw new TypeError(`the ${key} option is not supported`);
	}
	if ('sourceType' in options && options.sourceType !== 'script' && options.sourceType !== 'module') {
		throw new TypeError(`sourceType must be "script" or "module", not ${JSON.stringify(options.sourceType)}`);
	}
	if ('until' in options && options.until !== 'as') {
		throw new TypeError(`until must be "as", not ${JSON.stringify(options.until)}`);
	}
	const on = FLAGS.filter((flag) => options[flag] === true);
	if (options.typescript === 'erase') on.push('typescript', 'erase');
	if (options.sourceType !== 'module') on.push('script');
	return on.join(',');
}

/** Writes a string into the module's memory as UTF-8, taking ownership of the room. */
function write(text) {
	const capacity = text.length * 3;
	const ptr = wasm.alloc(capacity);
	const { written } = encoder.encodeInto(text, new Uint8Array(wasm.memory.buffer, ptr, capacity));
	return [ptr, written, capacity];
}

function text() {
	return utf8.decode(new Uint8Array(wasm.memory.buffer, wasm.text_ptr(), wasm.text_len()));
}

/** The constant strings the module has numbered so far. @type {string[]} */
let constants = [];
const table = () => constants;

/**
 * The last answer as a tree, or thrown as the `SyntaxError` it describes. The words are read in
 * place, so the constants are fetched first: writing them can grow the memory, which would
 * detach a view taken before.
 */
function result(status, source) {
	if (status !== 0) {
		const { message, ...error } = JSON.parse(text()).error;
		throw Object.assign(new SyntaxError(message), error);
	}
	const words = () => new Uint32Array(wasm.memory.buffer, wasm.words_ptr(), wasm.words_len());
	if (words()[4] > constants.length) {
		wasm.constants();
		constants = JSON.parse(text());
	}
	return decode(words(), source, table);
}

const registry = typeof FinalizationRegistry === 'undefined' ? null : new FinalizationRegistry((handle) => wasm.source_free(handle));

/** A source kept with its options, so the parses out of it share its copy and its tables. */
export class Source {
	#handle;
	#source;

	constructor(source, options) {
		const on = flags(options);
		const [ptr, len, capacity] = write(source);
		const [flags_ptr, flags_len, flags_capacity] = write(on);
		this.#handle = wasm.source_new(ptr, len, capacity, flags_ptr, flags_len, flags_capacity);
		this.#source = source;
		registry?.register(this, this.#handle, this);
	}

	/** The whole source, or the program spanning `start..end` of it. */
	parse(start, end) {
		if (start === undefined && end === undefined) return result(wasm.source_parse(this.#handle, ENTRY.program, 0, 0), this.#source);
		return result(wasm.source_parse_range(this.#handle, start ?? 0, end ?? -1), this.#source);
	}

	/** @param {number} offset @param {'as'} [until] when the host's `as` follows the expression */
	parseExpressionAt(offset, until) {
		return result(wasm.source_parse(this.#handle, ENTRY.expression, offset, until === 'as' ? 1 : 0), this.#source);
	}

	parsePatternAt(offset) {
		return result(wasm.source_parse(this.#handle, ENTRY.pattern, offset, 0), this.#source);
	}

	parseParamsAt(offset) {
		return result(wasm.source_parse(this.#handle, ENTRY.params, offset, 0), this.#source);
	}

	parseStatementAt(offset) {
		return result(wasm.source_parse(this.#handle, ENTRY.statement, offset, 0), this.#source);
	}

	/** Frees the module memory the source holds; the registry does it otherwise. */
	free() {
		if (this.#handle === undefined) return;
		registry?.unregister(this);
		wasm.source_free(this.#handle);
		this.#handle = undefined;
	}
}

function once(source, options, run) {
	const held = new Source(source, options);
	try {
		return run(held);
	} finally {
		held.free();
	}
}

export const parse = (source, options) => once(source, options, (held) => held.parse());
export const parseExpressionAt = (source, offset, options) => once(source, options, (held) => held.parseExpressionAt(offset, options?.until));
export const parsePatternAt = (source, offset, options) => once(source, options, (held) => held.parsePatternAt(offset));
export const parseParamsAt = (source, offset, options) => once(source, options, (held) => held.parseParamsAt(offset));
export const parseStatementAt = (source, offset, options) => once(source, options, (held) => held.parseStatementAt(offset));
