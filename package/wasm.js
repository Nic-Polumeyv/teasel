// `@teasel/parser` anywhere: the API over the WebAssembly module, loaded by `init` first. The
// module has no glue: the source goes into its memory once, and every answer is read straight
// out of it.
import { bind } from './api.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';

const encoder = new TextEncoder();
const utf8 = new TextDecoder();

/** @type {WebAssembly.Exports & Record<string, Function> & { memory: WebAssembly.Memory }} */
let wasm;
/** @type {string[]} */
let constants = [];

/**
 * Instantiates the module: the bytes, a compiled `WebAssembly.Module`, or a `Response` (or a
 * promise of one) to stream it from; by default `teasel.wasm` next to this file is fetched.
 * @param {BufferSource | WebAssembly.Module | Response | Promise<Response>} [module]
 */
export async function init(module = fetch(new URL('./teasel.wasm', import.meta.url))) {
	if (module instanceof Promise) module = await module;
	const { instance } =
		typeof Response !== 'undefined' && module instanceof Response
			? await WebAssembly.instantiateStreaming(module, {})
			: await WebAssembly.instantiate(module, {});
	wasm = /** @type {any} */ (instance.exports);
}

/** Writes a string into the module's memory as UTF-8 and hands the room to a source. */
function create(source, bits) {
	const capacity = source.length * 3;
	const ptr = wasm.alloc(capacity);
	const { written } = encoder.encodeInto(source, new Uint8Array(wasm.memory.buffer, ptr, capacity));
	return wasm.source_new(ptr, written, capacity, bits);
}

const text = () => utf8.decode(new Uint8Array(wasm.memory.buffer, wasm.text_ptr(), wasm.text_len()));
const words = () => new Uint32Array(wasm.memory.buffer, wasm.words_ptr(), wasm.words_len());

/**
 * The last answer: its words in place, or the error text. The constants are fetched first, as
 * writing them can grow the memory, which would detach a view taken before.
 */
function answer(status) {
	if (status !== 0) return text();
	if (words()[4] > constants.length) {
		wasm.constants();
		constants = JSON.parse(text());
	}
	return words();
}

export const { parse, parseExpressionAt, parsePatternAt, parseParamsAt, parseStatementAt, Source } = bind({
	// the words outlive the source: they sit in the module's answer buffer until the next parse
	once(source, bits, entry, offset, until) {
		const held = create(source, bits);
		try {
			return answer(wasm.source_parse(held, entry, offset, until));
		} finally {
			wasm.source_free(held);
		}
	},
	create,
	parse: (held, entry, offset, until) => answer(wasm.source_parse(held, entry, offset, until)),
	parseRange: (held, start, end) => answer(wasm.source_parse_range(held, start, end ?? -1)),
	free: (held) => wasm.source_free(held),
	constants: () => constants,
});
