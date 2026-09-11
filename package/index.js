// the source goes over as bytes: V8's encoder is 14x faster than the host reading a string out
import { bind } from './api.js';
import { load } from './native.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';
export { scopeOf, bindingOf, referenceOf, parentOf } from './decode.js';

const native = load();
const encoder = new TextEncoder();
let scratch = new Uint8Array(1 << 16);

function bytes(text) {
	const size = text.length * 3;
	if (scratch.length < size && size <= 1 << 20) scratch = new Uint8Array(size);
	const room = size <= scratch.length ? scratch : new Uint8Array(size);
	const { written } = encoder.encodeInto(text, room);
	return room.subarray(0, written);
}

export const Source = bind({
	create: (source, names, host) => native.create(bytes(source), names, host),
	parse: native.parse,
	free: native.free,
	constants: native.constants,
	shapes: native.shapes,
});
