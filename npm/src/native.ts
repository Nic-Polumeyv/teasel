// the source goes over as bytes: V8's encoder is 14x faster than the host reading a string out
import type { Engine, Tree } from './lib/decode.js';
import { load } from './lib/addon.js';

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

export const engine: Engine = {
	create(source, flags, host) {
		const held = native.create(bytes(source), flags, host);
		return { parse: (entry, offset, end, stop) => native.parse(held, entry, offset, end, stop), free: () => native.free(held) };
	},
	layout: native.layout,
	// the addon keeps one array of views a tree and sets what moved: asked only then
	tree: (typescript, moved) => (moved || trees[+typescript] === undefined ? (trees[+typescript] = native.tree()!) : trees[+typescript]!),
};
