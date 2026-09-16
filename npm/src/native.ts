// the source goes over as bytes: V8's encoder is 14x faster than the host reading a string out
import type { Program } from 'estree';
import { Source as Base, type Engine, type Options, type Tree } from './lib/api.js';
import { load } from './lib/addon.js';

export { parentOf, referenceOf, scopeOf } from './lib/api.js';
export type * from './lib/api.js';

const native = load();
const encoder = new TextEncoder();
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
	constants: native.constants,
	shapes: native.shapes,
	layout: native.layout,
	tree(): Tree | undefined {
		const t = native.tree();
		if (t === undefined) return undefined;
		return { nodes: t[0].subarray(0, t[1]), lists: t[2].subarray(0, t[3]), numbers: t[4].subarray(0, t[5]), text: t[6].subarray(0, t[7]), starts: t[8].subarray(0, t[9]) };
	},
};

export class Source<Root = Program> extends Base<Root> {
	constructor(source: string, options?: Options) {
		super(engine, source, options);
	}
}
