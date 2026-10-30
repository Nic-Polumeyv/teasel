// the source goes over as bytes because V8's encoder beat the host reading a string out 3.4x at 1 MB and 10x at 56 KB on Node 26.8.1 (2026-10-28)
import type { Engine, Tree } from '../types.ts';
import { load } from './addon.ts';
import { compile } from './wasm.ts';

const native = load();
const encoder = new TextEncoder();
const trees: (Tree | undefined)[] = [undefined, undefined];
let scratch = new Uint8Array(1 << 16);

function bytes(text: string) {
	const size = text.length * 3;
	// a fresh buffer per parse cost a 1 MB source 566 µs against 264 in the kept one, which stops at 16 MiB so a huge source leaves nothing behind
	if (scratch.length < size && size <= 1 << 24) scratch = new Uint8Array(size);
	const room = size <= scratch.length ? scratch : new Uint8Array(size);
	const { written } = encoder.encodeInto(text, room);
	return room.subarray(0, written);
}

export const engine: Engine = native === undefined ? compile() : {
	parse: (source, flags, entry, plan) => native.parse(bytes(source), flags, entry, plan),
	parseAt: (source, flags, entry, offset, end, stop) => native.parseAt(bytes(source), flags, entry, offset, end, stop),
	plan: native.plan,
	children: native.children,
	layout: native.layout,
	// the addon keeps one array of views a tree and sets what moved: asked only then
	tree: (typescript, moved) => (moved || trees[+typescript] === undefined ? (trees[+typescript] = native.tree()!) : trees[+typescript]!),
};
