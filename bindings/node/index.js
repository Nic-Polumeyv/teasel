import * as native from './binding.cjs';

export { isIdentifierStart, isIdentifierChar } from './identifier.js';

/** @typedef {import('./index.d.ts').Options} Options */

/** Rejects the acorn options teasel cannot honor and the values it cannot take. */
function check(options) {
	if (options === undefined) return options;
	for (const key of ['ranges', 'onComment', 'onToken', 'onInsertedSemicolon', 'onTrailingComma']) {
		if (key in options) throw new TypeError(`the ${key} option is not supported`);
	}
	if ('sourceType' in options && options.sourceType !== 'script' && options.sourceType !== 'module') {
		throw new TypeError(`sourceType must be "script" or "module", not ${JSON.stringify(options.sourceType)}`);
	}
	if ('until' in options && options.until !== 'as') {
		throw new TypeError(`until must be "as", not ${JSON.stringify(options.until)}`);
	}
	if (options.typescript === 'erase') return { ...options, typescript: true, erase: true };
	return options;
}

/**
 * Turns the addon's JSON into a tree, or into the `SyntaxError` acorn would throw, with `pos`
 * and `loc` on it.
 * @param {string} json
 */
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

/** @param {string} source @param {Options} [options] */
export function parse(source, options) {
	return result(native.parse(source, check(options)));
}

/**
 * The parse-at functions return the node with `end`, the offset after everything the parse
 * consumed (closing parens and trailing comments included), and the comments read when
 * `comments` is on.
 * @param {string} source @param {number} offset @param {Options} [options]
 */
export function parseExpressionAt(source, offset, options) {
	return result(native.parseExpressionAt(source, offset, check(options)));
}

/** @param {string} source @param {number} offset @param {Options} [options] */
export function parsePatternAt(source, offset, options) {
	return result(native.parsePatternAt(source, offset, check(options)));
}

/** @param {string} source @param {number} offset @param {Options} [options] */
export function parseParamsAt(source, offset, options) {
	return result(native.parseParamsAt(source, offset, check(options)));
}

/** @param {string} source @param {number} offset @param {Options} [options] */
export function parseStatementAt(source, offset, options) {
	return result(native.parseStatementAt(source, offset, check(options)));
}

/**
 * A source kept with its options: the parses out of it share the source copy and the position
 * tables, which is what a host parsing every expression of a template wants.
 */
export class Source {
	#native;

	/** @param {string} source @param {Options} [options] */
	constructor(source, options) {
		this.#native = new native.Source(source, check(options));
	}

	/**
	 * The whole source, or the program that spans `start..end` of it, such as a script inside a
	 * template: positions stay those of the whole source.
	 * @param {number} [start] @param {number} [end]
	 */
	parse(start, end) {
		return result(this.#native.parse(start, end));
	}

	/** @param {number} offset @param {'as'} [until] when the host's `as` follows the expression */
	parseExpressionAt(offset, until) {
		return result(this.#native.parseExpressionAt(offset, until));
	}

	/** @param {number} offset */
	parsePatternAt(offset) {
		return result(this.#native.parsePatternAt(offset));
	}

	/** @param {number} offset */
	parseParamsAt(offset) {
		return result(this.#native.parseParamsAt(offset));
	}

	/** @param {number} offset */
	parseStatementAt(offset) {
		return result(this.#native.parseStatementAt(offset));
	}
}
