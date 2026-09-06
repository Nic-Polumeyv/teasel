// Turns the addon's packed token stream into ESTree objects: what `JSON.parse` did, without the
// text. The layout is `teasel::estree::Binary`, the tags `teasel::estree::token`.
const BEGIN = 1;
const END = 2;
const OBJECT = 3;
const LIST = 4;
const KEY = 5;
const INT = 6;
const FLOAT = 7;
const TRUE = 8;
const FALSE = 9;
const NULL = 10;
const CONST = 11;
const STR = 12;
const SLICE = 13;
const SPAN = 14;
const LOC = 15;

const utf16 = new TextDecoder('utf-16le');

/** @type {string[]} */
let constants = [];

/**
 * @param {ArrayBuffer} buffer
 * @param {string} source
 * @param {() => string[]} fetch the addon's constant strings, when an answer refers past the known ones
 */
export function decode(buffer, source, fetch) {
	const words = new Uint32Array(buffer);
	const [tokens_count, ends_count, floats_count, units, known] = words;
	if (known > constants.length) constants = fetch();
	const tokens = words.subarray(5, 5 + tokens_count);
	const ends = words.subarray(5 + tokens_count, 5 + tokens_count + ends_count);
	const text_at = 5 + tokens_count + ends_count;
	const text = units ? utf16.decode(new Uint16Array(buffer, text_at * 4, units)) : '';
	let floats_at = text_at + ((units + 1) >> 1);
	if (floats_at % 2 === 1) floats_at++;
	const floats = floats_count ? new Float64Array(buffer, floats_at * 4, floats_count) : null;
	const strings = new Array(ends_count);
	let from = 0;
	for (let i = 0; i < ends_count; i++) {
		strings[i] = text.slice(from, ends[i]);
		from = ends[i];
	}
	let at = 0;

	function value() {
		const tag = tokens[at++];
		switch (tag) {
			case BEGIN:
				return entries({ type: constants[tokens[at++]] });
			case OBJECT:
				return entries({});
			case LIST: {
				const list = [];
				while (tokens[at] !== END) list.push(value());
				at++;
				return list;
			}
			case INT:
				return tokens[at++];
			case FLOAT:
				return floats[tokens[at++]];
			case TRUE:
				return true;
			case FALSE:
				return false;
			case NULL:
				return null;
			case CONST:
				return constants[tokens[at++]];
			case STR:
				return strings[tokens[at++]];
			case SLICE: {
				const start = tokens[at++];
				return source.slice(start, tokens[at++]);
			}
			default:
				throw new Error(`bad token ${tag} at ${at - 1}`);
		}
	}

	/** @param {any} object */
	function entries(object) {
		for (;;) {
			const tag = tokens[at++];
			if (tag === END) return object;
			if (tag === KEY) {
				object[constants[tokens[at++]]] = value();
			} else if (tag === SPAN) {
				object.start = tokens[at++];
				object.end = tokens[at++];
			} else if (tag === LOC) {
				object.loc = {
					start: { line: tokens[at++], column: tokens[at++] },
					end: { line: tokens[at++], column: tokens[at++] }
				};
			} else {
				throw new Error(`bad token ${tag} at ${at - 1}`);
			}
		}
	}

	return value();
}
