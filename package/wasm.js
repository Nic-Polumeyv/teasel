import { bind } from './api.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';
export { scopeOf, bindingOf, referenceOf, parentOf } from './decode.js';

const encoder = new TextEncoder();
const utf8 = new TextDecoder();

/** @type {WebAssembly.Exports & Record<string, Function> & { memory: WebAssembly.Memory }} */
let wasm;
/** @type {string[]} */
let constants = [];
/** @type {number[]} */
let shapes = [];
let shapes_known = 0;

/** @param {BufferSource | WebAssembly.Module | Response | Promise<Response>} [module] `teasel.wasm` next to this file by default */
export async function init(module) {
	if (module === undefined) {
		const url = new URL('./teasel.wasm', import.meta.url);
		module = url.protocol === 'file:' ? (await import('node:fs/promises')).readFile(url) : fetch(url);
	}
	if (module instanceof Promise) module = await module;
	const { instance } =
		typeof Response !== 'undefined' && module instanceof Response
			? await WebAssembly.instantiateStreaming(module, {})
			: await WebAssembly.instantiate(module, {});
	wasm = /** @type {any} */ (instance.exports);
}

// the module takes the bytes over
function bytes(text) {
	const capacity = text.length * 3;
	const ptr = wasm.alloc(capacity);
	const { written } = encoder.encodeInto(text, new Uint8Array(wasm.memory.buffer, ptr, capacity));
	return [ptr, written, capacity];
}

function create(source, bits) {
	if (wasm === undefined) throw new Error('init() first');
	return wasm.source_new(...bytes(source), bits);
}

const text = () => utf8.decode(new Uint8Array(wasm.memory.buffer, wasm.text_ptr(), wasm.text_len()));
const words = () => new Uint32Array(wasm.memory.buffer, wasm.words_ptr(), wasm.words_len());

// the constants and shapes come first: writing them can grow the memory and detach a view taken before
function answer(status) {
	if (status !== 0) return text();
	if (words()[4] > constants.length) {
		wasm.constants();
		constants = JSON.parse(text());
	}
	if (words()[5] > shapes_known) {
		shapes_known = words()[5];
		wasm.shapes();
		shapes = JSON.parse(text());
	}
	return words();
}

export const Source = bind({
	create,
	// the words outlive the source: they sit in the answer buffer until the next parse
	parse: (held, entry, offset, end, stop) => answer(wasm.source_parse(held, entry, offset, end ?? 0, end === undefined ? 0 : 1, ...bytes(stop))),
	free: (held) => wasm.source_free(held),
	constants: () => constants,
	shapes: () => shapes,
});
