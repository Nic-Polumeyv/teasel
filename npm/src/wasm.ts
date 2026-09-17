import type { Engine, Tree } from './lib/decode.js';

const encoder = new TextEncoder();
const utf8 = new TextDecoder();

// `crates/teasel-wasm/src/lib.rs`: buffers are taken over by the module, answers read back from its memory
interface Exports {
	readonly memory: WebAssembly.Memory;
	alloc(len: number): number;
	source_new(ptr: number, len: number, capacity: number, flags: number, host: number, host_len: number, host_capacity: number): number;
	source_free(handle: number): void;
	source_parse(handle: number, entry: number, offset: number, end: number, has_end: number, ptr: number, len: number, capacity: number): number;
	words_ptr(): number;
	words_len(): number;
	text_ptr(): number;
	text_len(): number;
	constants(): void;
	shapes(): void;
	tree(): number;
	layout(): void;
}

// `teasel.wasm` next to this file, read where there is a file system and fetched elsewhere
// TODO: `import source` once we require node >= 22.19 and bundlers accept it: no fs, ~10 ms less startup
const url = new URL('./teasel.wasm', import.meta.url);
const { module, instance } =
	url.protocol === 'file:'
		? await WebAssembly.instantiate(await (await import('node:fs/promises')).readFile(url), {})
		: await WebAssembly.instantiateStreaming(fetch(url), {});
let wasm = instance.exports as unknown as Exports;
let constants: string[] = [];
let shapes: number[] = [];
let shapes_known = 0;
// a panic traps the instance for good: a fresh one takes over, and the sources held by the old one are gone
let generation = 0;

function guarded<T>(f: () => T): T {
	try {
		return f();
	} catch (error) {
		if (!(error instanceof WebAssembly.RuntimeError)) throw error;
		wasm = new WebAssembly.Instance(module, {}).exports as unknown as Exports;
		constants = [];
		shapes = [];
		shapes_known = 0;
		generation++;
		throw new Error('the engine panicked and started over; the sources it held are gone', { cause: error });
	}
}

// the module takes the bytes over
function bytes(text: string): [ptr: number, len: number, capacity: number] {
	const capacity = text.length * 3;
	const ptr = wasm.alloc(capacity);
	const { written } = encoder.encodeInto(text, new Uint8Array(wasm.memory.buffer, ptr, capacity));
	return [ptr, written, capacity];
}

const text = () => utf8.decode(new Uint8Array(wasm.memory.buffer, wasm.text_ptr(), wasm.text_len()));
const words = () => new Uint32Array(wasm.memory.buffer, wasm.words_ptr(), wasm.words_len());

let tree_at = 0;

// the constants, the shapes and the tree's addresses come first: writing them can grow the memory and detach a view taken before
function answer(status: number) {
	if (status !== 0) return text();
	layout();
	tree_at = wasm.tree();
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

let elements: { js: [string, string][]; ts: [string, string][] } | undefined;
const trees: ({ buffer: ArrayBuffer; at: Uint32Array; views: (Uint32Array | Float64Array | Uint8Array | number | undefined)[] } | undefined)[] = [undefined, undefined];
let layout_text: string | undefined;
// read once, before any view of the tree: writing it can grow the memory and detach them
function layout() {
	if (layout_text === undefined) {
		wasm.layout();
		layout_text = text();
	}
	return layout_text;
}

export const engine: Engine = {
	create(source, flags, host) {
		const handle = guarded(() => wasm.source_new(...bytes(source), flags, ...bytes(host)));
		if (handle === 0) throw new Error(JSON.parse(text()).error.message);
		const held = generation;
		return {
			// the words outlive the source: they sit in the answer buffer until the next parse
			parse(entry, offset, end, stop) {
				if (held !== generation) throw new Error('the source was held by an engine that panicked and started over');
				return answer(guarded(() => wasm.source_parse(handle, entry, offset, end ?? 0, end === undefined ? 0 : 1, ...bytes(stop))));
			},
			free() {
				if (held === generation) wasm.source_free(handle);
			},
		};
	},
	constants: () => constants,
	shapes: () => shapes,
	layout,
	// the tree sits in the module's memory until the next parse: a view is made anew when its buffer moved or the memory grew
	tree(typescript) {
		elements ??= JSON.parse(layout()).views;
		const { buffer } = wasm.memory;
		const at = new Uint32Array(buffer, tree_at, 65);
		const kinds = typescript ? elements!.ts : elements!.js;
		const held = (trees[+typescript] ??= { buffer, at: new Uint32Array(65), views: [+typescript] });
		const same = held.buffer === buffer;
		held.buffer = buffer;
		for (let i = 0; i < at[0] >> 1; i++) {
			const ptr = at[1 + 2 * i], bytes = at[2 + 2 * i];
			if (same && held.at[1 + 2 * i] === ptr && held.at[2 + 2 * i] === bytes) continue;
			held.at[1 + 2 * i] = ptr;
			held.at[2 + 2 * i] = bytes;
			held.views[1 + i] = ptr === 0 ? undefined : kinds[i][1] === 'f64' ? new Float64Array(buffer, ptr, bytes >> 3) : kinds[i][1] === 'u8' ? new Uint8Array(buffer, ptr, bytes) : new Uint32Array(buffer, ptr, bytes >> 2);
		}
		return held.views;
	},
};
