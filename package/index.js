// `@teasel/parser` on Node: the API over the native addon.
import { createRequire } from 'node:module';
import { bind } from './api.js';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';

const native = createRequire(import.meta.url)('./binding.cjs');

export const { parse, parseExpressionAt, parsePatternAt, parseParamsAt, parseStatementAt, Source } = bind({
	once: native.parseAt,
	create: (source, bits) => new native.Source(source, bits),
	parse: (held, entry, offset, until) => held.parseAt(entry, offset, until),
	parseRange: (held, start, end) => held.parseRange(start, end),
	constants: native.constants,
});
