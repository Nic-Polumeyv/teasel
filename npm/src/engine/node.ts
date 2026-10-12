// the source goes over as bytes: V8's encoder is 14x faster than the host reading a string out
import { readFileSync } from 'node:fs';
import type { Engine, Tree } from '../types.ts';
import { load } from './addon.ts';
import { start, url } from './wasm.ts';

const native = load();
const encoder = new TextEncoder();
const trees: (Tree | undefined)[] = [undefined, undefined];
let scratch = new Uint8Array(1 << 16);

function bytes(text: string) {
	const size = text.length * 3;
	if (scratch.length < size && size <= 1 << 20) scratch = new Uint8Array(size);
	const room = size <= scratch.length ? scratch : new Uint8Array(size);
	const { written } = encoder.encodeInto(text, room);
	return room.subarray(0, written);
}

// synchronous: 7 ms against 13 async for the 1.1 MB module, and require() refuses a graph with top-level await
// TODO: `import source` once node >= 22.19, tsc and bundlers accept it: no file read
export const engine: Engine = native === undefined ? start(new WebAssembly.Module(readFileSync(url))) : {
	create: (source, flags) => native.create(bytes(source), flags),
	parse: native.parse,
	free: native.free,
	once: (source, flags, entry, stop, plan) => native.once(bytes(source), flags, entry, stop, plan),
	plan: native.plan,
	children: native.children,
	layout: native.layout,
	// the addon keeps one array of views a tree and sets what moved: asked only then
	tree: (typescript, moved) => (moved || trees[+typescript] === undefined ? (trees[+typescript] = native.tree()!) : trees[+typescript]!),
};
