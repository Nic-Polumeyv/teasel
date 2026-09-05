// acorn's `isIdentifierStart` and `isIdentifierChar`, so a host that tokenizes around embedded
// JavaScript needs no second parser for them.
const start = /[\p{ID_Start}$_]/u;
const part = /[\p{ID_Continue}$‌‍]/u;

/** @param {number} code */
export function isIdentifierStart(code) {
	if (code < 128) return (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || code === 36 || code === 95;
	return start.test(String.fromCodePoint(code));
}

/** @param {number} code */
export function isIdentifierChar(code) {
	if (code < 128) return (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || (code >= 48 && code <= 57) || code === 36 || code === 95;
	return part.test(String.fromCodePoint(code));
}
