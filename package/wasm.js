import { bind } from './api.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';
export { scopeOf, bindingOf, referenceOf, parentOf } from './api.js';

const encoder = new TextEncoder();
const utf8 = new TextDecoder();

// `teasel.wasm` next to this file, read where there is a file system and fetched elsewhere
const url = new URL('./teasel.wasm', import.meta.url);
const { module, instance } =
	url.protocol === 'file:'
		? await WebAssembly.instantiate(await (await import('node:fs/promises')).readFile(url), {})
		: await WebAssembly.instantiateStreaming(fetch(url), {});
/** @type {WebAssembly.Exports & Record<string, Function> & { memory: WebAssembly.Memory }} */
let wasm = /** @type {any} */ (instance.exports);
/** @type {string[]} */
let constants = [];
/** @type {number[]} */
let shapes = [];
let shapes_known = 0;
// a panic traps the instance for good: a fresh one takes over, and the sources held by the old one are gone
let generation = 0;

/** @template T @param {() => T} f @returns {T} */
function guarded(f) {
	try {
		return f();
	} catch (error) {
		if (!(error instanceof WebAssembly.RuntimeError)) throw error;
		wasm = /** @type {any} */ (new WebAssembly.Instance(module, {}).exports);
		constants = [];
		shapes = [];
		shapes_known = 0;
		generation++;
		throw new Error('the engine panicked and started over; the sources it held are gone', { cause: error });
	}
}

// the module takes the bytes over
function bytes(text) {
	const capacity = text.length * 3;
	const ptr = wasm.alloc(capacity);
	const { written } = encoder.encodeInto(text, new Uint8Array(wasm.memory.buffer, ptr, capacity));
	return [ptr, written, capacity];
}

function create(source, names, host) {
	const handle = guarded(() => wasm.source_new(...bytes(source), ...bytes(names), ...bytes(host)));
	if (handle === 0) throw new Error(JSON.parse(text()).error.message);
	return { handle, generation };
}

/** @param {{ handle: number, generation: number }} held */
function handle(held) {
	if (held.generation !== generation) throw new Error('the source was held by an engine that panicked and started over');
	return held.handle;
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

export const engine = {
	create,
	// the words outlive the source: they sit in the answer buffer until the next parse
	parse: (held, entry, offset, end, stop) => answer(guarded(() => wasm.source_parse(handle(held), entry, offset, end ?? 0, end === undefined ? 0 : 1, ...bytes(stop)))),
	free: (held) => {
		if (held.generation === generation) wasm.source_free(held.handle);
	},
	constants: () => constants,
	shapes: () => shapes,
};

export const Source = bind(engine);
