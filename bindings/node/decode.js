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

// the text is in the host's byte order like the words, and a leading U+FEFF is text, not a mark
const utf16 = new TextDecoder(new Uint8Array(new Uint16Array([1]).buffer)[0] === 1 ? 'utf-16le' : 'utf-16be', { ignoreBOM: true });

/** @type {string[]} */
let constants = [];

/**
 * @param {ArrayBuffer} buffer
 * @param {string} source
 * @param {() => string[]} fetch the addon's constant strings, when an answer refers past the known ones
 * @param {boolean} [link] replace the scope and binding numbers with the objects they index
 */
export function decode(buffer, source, fetch, link = true) {
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
	/** @type {any[]} nodes with a `scope`, `declares` or `binding` key, for `link` */
	const indexed = [];

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
				const key = constants[tokens[at++]];
				object[key] = value();
				if (key === 'scope' || key === 'binding' || key === 'declares') indexed.push(object);
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

	const root = value();
	if (link) linkScopes(root, indexed);
	return root;
}

/**
 * Turns the answer's scope and binding tables into objects and the numbers on nodes into
 * references to them: `node.scope`, `identifier.binding`, `binding.node`, `binding.references`,
 * `scope.node`, `scope.parent`, `scope.bindings`, `scope.through`.
 * @param {any} answer a program or a parse-at answer carrying `scopes` and `bindings`
 * @param {any[]} indexed the nodes carrying a `scope`, `declares` or `binding` number
 */
export function linkScopes(answer, indexed) {
	if (!answer || !answer.scopes) return;
	const scopes = answer.scopes.map((scope) => ({ ...scope, node: null, bindings: [], declarations: new Map() }));
	const bindings = answer.bindings.map((binding) => ({ ...binding, node: null, references: [] }));
	for (const scope of scopes) {
		scope.parent = scope.parent === null ? null : scopes[scope.parent];
		scope.through = scope.through.map((index) => bindings[index]);
	}
	for (const binding of bindings) {
		binding.scope = scopes[binding.scope];
		binding.scope.bindings.push(binding);
		binding.scope.declarations.set(binding.name, binding);
	}
	for (const node of indexed) {
		// the tables' own entries carry `scope` too, and are not nodes
		if (typeof node.type !== 'string') continue;
		if (node.scope !== undefined) {
			node.scope = scopes[node.scope];
			node.scope.node = node;
		}
		if (node.declares !== undefined) {
			const binding = bindings[node.declares];
			delete node.declares;
			node.binding = binding;
			if (binding.node === null) binding.node = node;
		} else if (node.binding !== undefined && node.binding !== null) {
			node.binding = bindings[node.binding];
			node.binding.references.push(node);
		}
	}
	answer.scopes = scopes;
	answer.bindings = bindings;
}
