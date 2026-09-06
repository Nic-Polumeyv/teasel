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
const little = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;
const utf16 = new TextDecoder(little ? 'utf-16le' : 'utf-16be', { ignoreBOM: true });

/** @type {WeakMap<Function, string[]>} constants per engine, by the function that fetches them */
const tables = new WeakMap();

function unaligned_floats(buffer, start, count) {
	const view = new DataView(buffer, start, count * 8);
	const floats = new Float64Array(count);
	for (let i = 0; i < count; i++) floats[i] = view.getFloat64(i * 8, little);
	return floats;
}

/**
 * @param {ArrayBuffer | Uint32Array} answer the words, or a view of them inside a larger buffer
 * @param {string} source
 * @param {() => string[]} fetch the constant strings, when an answer refers past the known ones
 * @param {boolean} [link] replace the scope and binding numbers with the objects they index
 */
export function decode(answer, source, fetch, link = true) {
	const words = answer instanceof Uint32Array ? answer : new Uint32Array(answer);
	const { buffer, byteOffset } = words;
	const [tokens_count, ends_count, floats_count, units, known] = words;
	let constants = tables.get(fetch);
	if (constants === undefined || known > constants.length) tables.set(fetch, (constants = fetch()));
	const tokens = words.subarray(5, 5 + tokens_count);
	const ends = words.subarray(5 + tokens_count, 5 + tokens_count + ends_count);
	const text_at = 5 + tokens_count + ends_count;
	const text = units ? utf16.decode(new Uint16Array(buffer, byteOffset + text_at * 4, units)) : '';
	let floats_at = text_at + ((units + 1) >> 1);
	if (floats_at % 2 === 1) floats_at++;
	const floats_start = byteOffset + floats_at * 4;
	const floats = !floats_count ? null : floats_start % 8 === 0 ? new Float64Array(buffer, floats_start, floats_count) : unaligned_floats(buffer, floats_start, floats_count);
	const strings = new Array(ends_count);
	let from = 0;
	for (let i = 0; i < ends_count; i++) {
		strings[i] = text.slice(from, ends[i]);
		from = ends[i];
	}
	let at = 0;
	/** @type {any[]} node, key, value for every scope fact met, in stream order */
	const facts = [];

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
				// a node's scope facts go to the side tables; the tables' own entries keep theirs
				if (link && typeof object.type === 'string' && FACTS.has(key)) facts.push(object, key, value());
				else object[key] = value();
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
	if (link) linkScopes(root, facts);
	return root;
}

const FACTS = new Set(['scope', 'binding', 'declares', 'write', 'mutate']);
const scopes_of = new WeakMap();
const bindings_of = new WeakMap();

/** @param {import('estree').Node} node @returns {import('./index.js').Scope | undefined} the scope the node opens */
export const scopeOf = (node) => scopes_of.get(node);
/** @param {import('estree').Node} node @returns {import('./index.js').Binding | null | undefined} what the identifier declares or refers to; null for a global, undefined when it names no value */
export const bindingOf = (node) => bindings_of.get(node);

/**
 * Turns the answer's scope and binding tables into objects and files each node's facts in the
 * side tables: `binding.node`, `binding.references`, `scope.node`, `scope.parent`,
 * `scope.bindings`, `scope.declarations`, `scope.through`.
 * @param {any} answer a program or a parse-at answer carrying `scopes` and `bindings`
 * @param {any[]} facts node, key, value triples in stream order
 */
export function linkScopes(answer, facts) {
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
	let reference = null;
	for (let i = 0; i < facts.length; i += 3) {
		const node = facts[i], key = facts[i + 1], value = facts[i + 2];
		if (key === 'scope') {
			const scope = scopes[value];
			scopes_of.set(node, scope);
			scope.node = node;
		} else if (key === 'declares') {
			const binding = bindings[value];
			bindings_of.set(node, binding);
			if (binding.node === null) binding.node = node;
		} else if (key === 'binding') {
			reference = null;
			if (value === null) {
				bindings_of.set(node, null);
			} else {
				const binding = bindings[value];
				bindings_of.set(node, binding);
				binding.references.push((reference = { node, write: false, mutate: false }));
			}
		} else if (reference !== null) {
			reference[key] = true;
		}
	}
	answer.scopes = scopes;
	answer.bindings = bindings;
}
