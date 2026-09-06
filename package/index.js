// the source goes over as bytes: V8's encoder is 14x faster than napi reading a string
import { createRequire } from 'node:module';
import { bind } from './api.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';
export { scopeOf, bindingOf, referenceOf } from './decode.js';

const native = createRequire(import.meta.url)('./binding.cjs');
const encoder = new TextEncoder();
let scratch = new Uint8Array(1 << 16);

function bytes(text) {
	const size = text.length * 3;
	if (scratch.length < size && size <= 1 << 20) scratch = new Uint8Array(size);
	const room = size <= scratch.length ? scratch : new Uint8Array(size);
	const { written } = encoder.encodeInto(text, room);
	return room.subarray(0, written);
}

export const { parse, parseExpressionAt, parsePatternAt, parseParamsAt, parseStatementAt, Source } = bind({
	once: (source, bits, entry, offset, until) => native.parseAt(bytes(source), bits, entry, offset, until),
	create: (source, bits) => new native.Source(bytes(source), bits),
	parse: (held, entry, offset, until) => held.parseAt(entry, offset, until),
	parseRange: (held, start, end) => held.parseRange(start, end),
	constants: native.constants,
	shapes: native.shapes,
});
