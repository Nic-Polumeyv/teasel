import type { Engine, Tree } from '../types.ts';

const encoder = new TextEncoder();
const utf8 = new TextDecoder();

// `crates/teasel-wasm/src/lib.rs`: buffers are taken over by the module, answers read back from its memory
interface Exports {
	readonly memory: WebAssembly.Memory;
	alloc(len: number): number;
	parse(ptr: number, len: number, capacity: number, flags: number, entry: number, plan: number): number;
	parse_at(ptr: number, len: number, capacity: number, flags: number, entry: number, offset: number, end: number, has_end: number, stop_ptr: number, stop_len: number, stop_capacity: number): number;
	plan_new(ptr: number, len: number, capacity: number): number;
	plan_free(handle: number): void;
	plan_children(handle: number): void;
	words_ptr(): number;
	words_len(): number;
	text_ptr(): number;
	text_len(): number;
	tree(): number;
	layout(): void;
}

// `teasel.wasm` beside the package entry: a bundler includes the file without configuration
const url = new URL('../teasel.wasm', import.meta.url);
let module: WebAssembly.Module;
let wasm: Exports;

function start(compiled: WebAssembly.Module, instance: WebAssembly.Instance): Engine {
	module = compiled;
	wasm = instance.exports as unknown as Exports;
	return engine;
}

// synchronous: 7 ms against 13 async for the 1.1 MB module, and require() refuses a graph with top-level await
// `import source` measured slower (6.3 against 4.7 ms), needs Node 24 where it is Stability 1.2, TypeScript 7.0.2 rejects it (7.1 adds it), and no bundler handles it
export function compile(): Engine {
	const compiled = new WebAssembly.Module(globalThis.process.getBuiltinModule('node:fs').readFileSync(url));
	return start(compiled, new WebAssembly.Instance(compiled, {}));
}

// read where there is a file system and fetched elsewhere
export async function instantiate(): Promise<Engine> {
	const { module: compiled, instance } =
		url.protocol === 'file:'
			? await WebAssembly.instantiate(await globalThis.process.getBuiltinModule('node:fs/promises').readFile(url), {})
			: await fetched();
	return start(compiled, instance);
}

// streaming refuses a file a server sends as anything but application/wasm, with a TypeError before it reads the body
async function fetched() {
	const response = fetch(url);
	try {
		return await WebAssembly.instantiateStreaming(response, {});
	} catch (error) {
		if (!(error instanceof TypeError)) throw error;
		return WebAssembly.instantiate(await (await response).arrayBuffer(), {});
	}
}

// a panic traps the instance for good: a fresh one takes over, and plans are made again in it from their grammar
let generation = 0;

function guarded<T>(f: () => T): T {
	try {
		return f();
	} catch (error) {
		// a stack overflow unwinds the module mid-parse and leaves it as unusable as a trap does
		if (!(error instanceof WebAssembly.RuntimeError || error instanceof RangeError)) throw error;
		wasm = new WebAssembly.Instance(module, {}).exports as unknown as Exports;
		generation++;
		throw new Error('the engine panicked and started over', { cause: error });
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

// the layout comes first: writing it can grow the memory and detach a view taken before
function answer(status: number) {
	if (status === 2) throw new Error(text());
	if (status === 1) return text();
	layout();
	return words();
}

// a memory never shrinks, so one a huge source grew past 256 MiB goes with its instance, which costs 0.9 ms to replace
function shed() {
	if (wasm.memory.buffer.byteLength > 1 << 28) {
		wasm = new WebAssembly.Instance(module, {}).exports as unknown as Exports;
		generation++;
	}
}

const trees: ({ buffer: ArrayBuffer; at: Uint32Array; views: Tree[number][] } | undefined)[] = [undefined, undefined];
let layout_text: string | undefined;
// read once, before any view of the tree: writing it can grow the memory and detach them
function layout() {
	if (layout_text === undefined) {
		wasm.layout();
		layout_text = text();
	}
	return layout_text;
}

// the collector cannot see the module's memory, so a plan it takes frees its handle here
interface Slot {
	handle: number;
	generation: number;
}
const plans = new FinalizationRegistry<Slot>((slot) => {
	if (slot.generation === generation) wasm.plan_free(slot.handle);
});

class Plan {
	readonly slot: Slot = { handle: 0, generation: -1 };
	readonly grammar: string;
	constructor(grammar: string) {
		this.grammar = grammar;
		this.handle();
		// the held value must not reach this, or it is never collected
		plans.register(this, this.slot);
	}
	handle() {
		const { slot } = this;
		if (slot.generation !== generation) {
			slot.handle = guarded(() => opened(wasm.plan_new(...bytes(this.grammar))));
			slot.generation = generation;
		}
		return slot.handle;
	}
}

// 0 is the error answer at `text_ptr`: a grammar the engine cannot take
function opened(handle: number) {
	if (handle === 0) throw new Error(JSON.parse(text()).error.args[0]);
	return handle;
}

const engine: Engine = {
	parse(source, flags, entry, plan) {
		shed();
		const grammar = plan === undefined ? 0 : (plan as Plan).handle();
		return answer(guarded(() => wasm.parse(...bytes(source), flags, entry, grammar))) as Uint32Array;
	},
	parseAt(source, flags, entry, offset, end, stop) {
		shed();
		return answer(guarded(() => wasm.parse_at(...bytes(source), flags, entry, offset, end ?? 0, end === undefined ? 0 : 1, ...bytes(stop))));
	},
	plan: (grammar) => new Plan(grammar),
	children(plan) {
		guarded(() => wasm.plan_children((plan as Plan).handle()));
		return text();
	},
	layout,
	// the tree sits in the module's memory until the next parse: a view is made anew when its buffer moved or the memory grew
	tree(typescript, moved) {
		const known = trees[+typescript];
		if (!moved && known !== undefined && known.buffer === wasm.memory.buffer) return known.views;
		// the count of views, then each one's address, room in bytes and element size; the call allocates nothing, so the words' view stays
		const address = wasm.tree();
		const { buffer } = wasm.memory;
		const at = new Uint32Array(buffer, address, 148);
		const held = (trees[+typescript] ??= { buffer, at: new Uint32Array(148), views: [+typescript] });
		const same = held.buffer === buffer;
		held.buffer = buffer;
		// a new array: whoever kept the last one sees that a view in it changed
		held.views = held.views.slice();
		for (let i = 1; i < 1 + 3 * at[0]; i += 3) {
			const ptr = at[i], bytes = at[i + 1], size = at[i + 2];
			if (same && held.at[i] === ptr && held.at[i + 1] === bytes) continue;
			held.at[i] = ptr;
			held.at[i + 1] = bytes;
			held.views[1 + (i - 1) / 3] = ptr === 0 ? undefined : size === 8 ? new Float64Array(buffer, ptr, bytes >> 3) : size === 1 ? new Uint8Array(buffer, ptr, bytes) : new Uint32Array(buffer, ptr, bytes >> 2);
		}
		return held.views;
	},
};
