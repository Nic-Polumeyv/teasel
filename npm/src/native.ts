// the source goes over as bytes: V8's encoder is 14x faster than the host reading a string out
import type { Engine, Held, Tree } from './lib/decode.js';
import type { External } from './lib/addon.js';
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

// a plan is the external the addon holds the grammar in; V8 lets go of it, nothing to free
class Plan implements Held {
	readonly external: External;
	constructor(grammar: string) {
		this.external = native.plan(grammar);
	}
	free() {}
}

export const engine: Engine = {
	create(source, flags) {
		const held = native.create(bytes(source), flags);
		return { parse: (entry, offset, end, stop, plan) => native.parse(held, entry, offset, end, stop, (plan as Plan | undefined)?.external), free: () => native.free(held) };
	},
	plan: (grammar) => new Plan(grammar),
	layout: native.layout,
	// the addon keeps one array of views a tree and sets what moved: asked only then
	tree: (typescript, moved) => (moved || trees[+typescript] === undefined ? (trees[+typescript] = native.tree()!) : trees[+typescript]!),
};
