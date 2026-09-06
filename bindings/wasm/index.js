// The same API as `@teasel/parser` over the WebAssembly module; call `init` first.
import init, * as wasm from './pkg/teasel.js';

export { init };
export { isIdentifierStart, isIdentifierChar } from './identifier.js';

const FLAGS = ['typescript', 'comments', 'locations', 'preserveParens', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports'];

function flags(options = {}) {
	for (const key of ['ranges', 'onComment', 'onToken', 'onInsertedSemicolon', 'onTrailingComma']) {
		if (key in options) throw new TypeError(`the ${key} option is not supported`);
	}
	if ('sourceType' in options && options.sourceType !== 'script' && options.sourceType !== 'module') {
		throw new TypeError(`sourceType must be "script" or "module", not ${JSON.stringify(options.sourceType)}`);
	}
	if ('until' in options && options.until !== 'as') {
		throw new TypeError(`until must be "as", not ${JSON.stringify(options.until)}`);
	}
	const on = FLAGS.filter((flag) => options[flag] === true);
	if (options.sourceType !== 'module') on.push('script');
	if (options.until === 'as') on.push('untilAs');
	return on.join(',');
}

function result(json) {
	const value = JSON.parse(json);
	if (value.error) {
		const { message, pos, loc } = value.error;
		const error = new SyntaxError(loc ? `${message} (${loc.line}:${loc.column})` : message);
		error.pos = pos;
		error.loc = loc;
		throw error;
	}
	return value;
}

export const parse = (source, options) => result(wasm.parse(source, flags(options)));
export const parseExpressionAt = (source, offset, options) => result(wasm.parseExpressionAt(source, offset, flags(options)));
export const parsePatternAt = (source, offset, options) => result(wasm.parsePatternAt(source, offset, flags(options)));
export const parseParamsAt = (source, offset, options) => result(wasm.parseParamsAt(source, offset, flags(options)));
export const parseStatementAt = (source, offset, options) => result(wasm.parseStatementAt(source, offset, flags(options)));

// A source kept with its options, so the parses out of it share its tables.
export class Source {
	#wasm;

	constructor(source, options) {
		this.#wasm = new wasm.Source(source, flags(options));
	}

	parse() {
		return result(this.#wasm.parse());
	}

	parseExpressionAt(offset, until) {
		return result(this.#wasm.parseExpressionAt(offset, until));
	}

	parsePatternAt(offset) {
		return result(this.#wasm.parsePatternAt(offset));
	}

	parseParamsAt(offset) {
		return result(this.#wasm.parseParamsAt(offset));
	}

	parseStatementAt(offset) {
		return result(this.#wasm.parseStatementAt(offset));
	}

	/** Frees the WebAssembly memory the source holds. */
	free() {
		this.#wasm.free();
	}
}
