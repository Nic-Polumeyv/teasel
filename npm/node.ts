// the source goes over as bytes: V8's encoder is 14x faster than the host reading a string out
import type { Program } from 'estree';
import { Source as Base, type Engine, type Options } from './lib/api.js';
import { load } from './lib/native.js';

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
};

export class Source<Root = Program> extends Base<Root> {
	constructor(source: string, options?: Options) {
		super(engine, source, options);
	}
}
