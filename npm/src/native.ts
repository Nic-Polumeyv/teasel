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
		const views: Tree['views'][number][] = [];
		for (let i = 1; i < t.length; i += 2) views.push((t[i] as Uint32Array | undefined)?.subarray(0, t[i + 1] as number));
		return { typescript: t[0] === 1, views };
	},
};

export class Source<Root = Program> extends Base<Root> {
	constructor(source: string, options?: Options) {
		super(engine, source, options);
	}
}
