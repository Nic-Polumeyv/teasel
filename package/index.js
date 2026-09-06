// `@teasel/parser` on Node: the API over the native addon. The source goes over as UTF-8 bytes,
// which V8 encodes far faster than the addon could read a string, and the answer comes back as
// a view of the addon's buffer, read before the next call.
import { createRequire } from 'node:module';
import { bind } from './api.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';

const native = createRequire(import.meta.url)('./binding.cjs');
const encoder = new TextEncoder();
let scratch = new Uint8Array(1 << 16);

function bytes(text) {
	if (scratch.length < text.length * 3) scratch = new Uint8Array(text.length * 3);
	const { written } = encoder.encodeInto(text, scratch);
	return scratch.subarray(0, written);
}

export const { parse, parseExpressionAt, parsePatternAt, parseParamsAt, parseStatementAt, Source } = bind({
	once: (source, bits, entry, offset, until) => native.parseAt(bytes(source), bits, entry, offset, until),
	create: (source, bits) => new native.Source(bytes(source), bits),
	parse: (held, entry, offset, until) => held.parseAt(entry, offset, until),
	parseRange: (held, start, end) => held.parseRange(start, end),
	constants: native.constants,
});
