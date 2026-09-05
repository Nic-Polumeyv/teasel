import * as native from './binding.js';

/** @typedef {import('./index.d.ts').Options} Options */

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
		error.raisedAt = pos;
		error.loc = loc;
		throw error;
	}
	return value;
}

/** @param {string} source @param {Options} [options] */
export function parse(source, options) {
	return result(native.parse(source, options));
}

/** @param {string} source @param {number} offset @param {Options} [options] */
export function parseExpressionAt(source, offset, options) {
	return result(native.parseExpressionAt(source, offset, options));
}

/** @param {string} source @param {number} offset @param {Options} [options] */
export function parsePatternAt(source, offset, options) {
	return result(native.parsePatternAt(source, offset, options));
}

/** @param {string} source @param {number} offset @param {Options} [options] */
export function parseParamsAt(source, offset, options) {
	return result(native.parseParamsAt(source, offset, options));
}

/** @param {string} source @param {number} offset @param {Options} [options] */
export function parseStatementAt(source, offset, options) {
	return result(native.parseStatementAt(source, offset, options));
}
