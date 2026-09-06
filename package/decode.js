// Turns the addon's shape-coded stream into ESTree objects: what `JSON.parse` did, without the
// text. The layout is `teasel::estree::Binary`, the kinds `teasel::estree::kind`.
const HEADER = 7;
const NODE = 0;
const INT = 1;
const FLOAT = 2;
const BOOL = 3;
const CONST = 4;
const STR = 5;
const SLICE = 6;
const LOC = 7;
const NODES = 8;
const INTS = 9;
// in a node's place
const NULL = 0;
const END = 1;

const little = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;
// a leading U+FEFF is text, not a mark
const utf8 = new TextDecoder('utf-8', { ignoreBOM: true });

// symbol keys: ten times cheaper than a WeakMap entry, and skipped by JSON, Object.keys and for-in
const SCOPE = Symbol('scope');
const BINDING = Symbol('binding');
const REFERENCE = Symbol('reference');

/** @param {import('estree').Node} node @returns {import('./index.js').Scope | undefined} the scope the node opens */
export const scopeOf = (node) => (node == null ? undefined : node[SCOPE]);
/** @param {import('estree').Node} node @returns {import('./index.js').Binding | null | undefined} what the identifier declares or refers to; null for a global, undefined when it names no value */
export const bindingOf = (node) => (node == null ? undefined : node[BINDING]);
/** @param {import('estree').Node} node @returns {import('./index.js').Reference | undefined} the reference an identifier makes, a global's included */
export const referenceOf = (node) => (node == null ? undefined : node[REFERENCE]);

const FACTS = new Set(['scope', 'binding', 'declares', 'write', 'mutate']);

/**
 * One decode at a time; the builders are generated once and read through this.
 * @type {{ w: Uint32Array, at: number, strings: string[], floats: Float64Array | null, source: string, constants: string[], scopes: any[], bindings: any[], build: (() => any)[] }}
 */
const S = {
	w: new Uint32Array(0),
	at: 0,
	strings: [],
	floats: null,
	source: '',
	constants: [],
	scopes: [],
	bindings: [],
	build: []
};

function node() {
	const id = S.w[S.at++];
	return id === NULL ? null : S.build[id]();
}

function nodes() {
	const list = [];
	for (;;) {
		const id = S.w[S.at++];
		if (id === END) return list;
		list.push(id === NULL ? null : S.build[id]());
	}
}

function ints() {
	const n = S.w[S.at++];
	const list = new Array(n);
	for (let i = 0; i < n; i++) list[i] = S.w[S.at++];
	return list;
}

/** @param {number} kind */
function read(kind) {
	switch (kind) {
		case NODE:
			return node();
		case INT:
			return S.w[S.at++];
		case FLOAT:
			return /** @type {Float64Array} */ (S.floats)[S.w[S.at++]];
		case BOOL:
			return S.w[S.at++] === 1;
		case CONST:
			return S.constants[S.w[S.at++]];
		case STR:
			return S.strings[S.w[S.at++]];
		case SLICE:
			return S.source.slice(S.w[S.at++], S.w[S.at++]);
		case LOC:
			return { start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } };
		case NODES:
			return nodes();
		case INTS:
			return ints();
		default:
			throw new Error(`bad kind ${kind}`);
	}
}

/**
 * @param {any} n
 * @param {number | undefined} scope
 * @param {number | undefined} declares
 * @param {number | null | undefined} binding
 * @param {boolean} write
 * @param {boolean} mutate
 */
function file(n, scope, declares, binding, write, mutate) {
	if (scope !== undefined) {
		const s = S.scopes[scope];
		n[SCOPE] = s;
		s.node = n;
	}
	if (declares !== undefined) {
		const d = S.bindings[declares];
		n[BINDING] = d;
		if (d.node === null) d.node = n;
	}
	if (binding !== undefined) {
		const b = binding === null ? null : S.bindings[binding];
		n[BINDING] = b;
		const r = { node: n, binding: b, write, mutate };
		n[REFERENCE] = r;
		if (b !== null) b.references.push(r);
	}
}

/** @typedef {{ type: string | null, keys: string[], kinds: number[] }} Shape */

const READ = ['node()', 'S.w[S.at++]', 'S.floats[S.w[S.at++]]', 'S.w[S.at++] === 1', 'S.constants[S.w[S.at++]]', 'S.strings[S.w[S.at++]]', 'S.source.slice(S.w[S.at++], S.w[S.at++])', '{ start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } }', 'nodes()', 'ints()'];

/**
 * One object literal per shape: V8 allocates it in one hidden class. Facts, and everything the
 * stream puts before the last of them, are read into locals first, so the object never holds them.
 * @param {Shape} shape @param {boolean} link
 */
function generate({ type, keys, kinds }, link) {
	let last = -1;
	if (link && type !== null) for (let i = 0; i < keys.length; i++) if (FACTS.has(keys[i])) last = i;
	const lead = [];
	const props = type === null ? [] : [`type: ${JSON.stringify(type)}`];
	const facts = { scope: 'undefined', declares: 'undefined', binding: 'undefined', write: 'false', mutate: 'false' };
	for (let i = 0; i < keys.length; i++) {
		const key = keys[i];
		if (i > last) props.push(`${JSON.stringify(key)}: ${READ[kinds[i]]}`);
		else {
			lead.push(`const v${i} = ${READ[kinds[i]]};`);
			if (FACTS.has(key)) facts[key] = `v${i}`;
			else props.push(`${JSON.stringify(key)}: v${i}`);
		}
	}
	const body = `${lead.join(' ')} const n = { ${props.join(', ')} }; ${last < 0 ? '' : `file(n, ${facts.scope}, ${facts.declares}, ${facts.binding}, ${facts.write}, ${facts.mutate});`} return n;`;
	return new Function('S', 'node', 'nodes', 'ints', 'file', `return () => { ${body} };`)(S, node, nodes, ints, file);
}

/** The same without code generation, for a host whose policy forbids it. @param {Shape} shape @param {boolean} link */
function interpret({ type, keys, kinds }, link) {
	const facts = link && type !== null && keys.some((key) => FACTS.has(key));
	return () => {
		const n = type === null ? {} : { type };
		let scope, declares, binding, write = false, mutate = false;
		for (let i = 0; i < keys.length; i++) {
			const key = keys[i];
			const value = read(kinds[i]);
			if (!facts || !FACTS.has(key)) n[key] = value;
			else if (key === 'scope') scope = value;
			else if (key === 'declares') declares = value;
			else if (key === 'binding') binding = value;
			else if (key === 'write') write = value;
			else mutate = value;
		}
		if (facts) file(n, scope, declares, binding, write, mutate);
		return n;
	};
}

const compile = (() => {
	try {
		new Function('');
		return generate;
	} catch {
		return interpret;
	}
})();

/**
 * @typedef {{ constants: () => string[], shapes: () => ArrayLike<number> }} Tables the engine's numbering
 * @type {WeakMap<Tables, { constants: string[], shapes: (Shape | null)[], linked: (() => any)[], plain: (() => any)[] }>}
 */
const tables = new WeakMap();

/** @param {Tables} engine @param {number} known constants @param {number} known_shapes */
function table_of(engine, known, known_shapes) {
	let table = tables.get(engine);
	if (table === undefined) tables.set(engine, (table = { constants: [], shapes: [null, null], linked: [], plain: [] }));
	if (known > table.constants.length) table.constants = engine.constants();
	if (known_shapes > table.shapes.length) {
		const { constants, shapes } = table;
		const flat = engine.shapes();
		let at = 0;
		for (let id = 2; at < flat.length; id++) {
			const n = flat[at++];
			if (id === shapes.length) {
				const keys = [], kinds = [];
				for (let i = 1; i < n; i++) {
					keys.push(constants[flat[at + i] >>> 4]);
					kinds.push(flat[at + i] & 15);
				}
				shapes.push({ type: flat[at] === 0 ? null : constants[flat[at] - 1], keys, kinds });
			}
			at += n;
		}
	}
	return table;
}

/** @param {ReturnType<typeof table_of>} table @param {boolean} link */
function builders(table, link) {
	const list = link ? table.linked : table.plain;
	if (list.length === 0) list.push(() => null, () => []);
	while (list.length < table.shapes.length) list.push(compile(/** @type {Shape} */ (table.shapes[list.length]), link));
	return list;
}

function unaligned_floats(buffer, start, count) {
	const view = new DataView(buffer, start, count * 8);
	const floats = new Float64Array(count);
	for (let i = 0; i < count; i++) floats[i] = view.getFloat64(i * 8, little);
	return floats;
}

/** @param {any[]} scopes @param {any[]} bindings */
function link_tables(scopes, bindings) {
	for (const scope of scopes) {
		scope.parent = scope.parent === null ? null : scopes[scope.parent];
		scope.through = scope.through.map((index) => bindings[index]);
		scope.node = null;
		scope.bindings = [];
		scope.declarations = new Map();
	}
	for (const binding of bindings) {
		binding.scope = scopes[binding.scope];
		binding.scope.bindings.push(binding);
		binding.scope.declarations.set(binding.name, binding);
		binding.node = null;
		binding.references = [];
	}
}

/**
 * @param {ArrayBuffer | Uint32Array} answer the words, or a view of them inside a larger buffer
 * @param {string} source
 * @param {Tables} engine
 * @param {boolean} [link] replace the scope and binding numbers with the objects they index
 */
export function decode(answer, source, engine, link = true) {
	const words = answer instanceof Uint32Array ? answer : new Uint32Array(answer);
	const { buffer, byteOffset } = words;
	const [tree, ends_count, floats_count, bytes, known, known_shapes, tables_at] = words;
	const table = table_of(engine, known, known_shapes);
	const ends = words.subarray(HEADER + tree, HEADER + tree + ends_count);
	const text_at = HEADER + tree + ends_count;
	const text = bytes ? utf8.decode(new Uint8Array(buffer, byteOffset + text_at * 4, bytes)) : '';
	let floats_at = text_at + ((bytes + 3) >> 2);
	if (floats_at % 2 === 1) floats_at++;
	const floats_start = byteOffset + floats_at * 4;
	const floats = !floats_count ? null : floats_start % 8 === 0 ? new Float64Array(buffer, floats_start, floats_count) : unaligned_floats(buffer, floats_start, floats_count);
	const strings = new Array(ends_count);
	let from = 0;
	for (let i = 0; i < ends_count; i++) {
		strings[i] = text.slice(from, ends[i]);
		from = ends[i];
	}
	S.w = words;
	S.strings = strings;
	S.floats = floats;
	S.source = source;
	S.constants = table.constants;
	S.build = builders(table, link);
	let scopes = null, bindings = null;
	if (tables_at !== 0) {
		// the writer's `all_scopes` order; a third table would have to carry its key
		S.at = HEADER + tables_at;
		scopes = nodes();
		bindings = nodes();
		if (link) link_tables(scopes, bindings);
		S.scopes = scopes;
		S.bindings = bindings;
	}
	S.at = HEADER;
	const root = node();
	if (scopes !== null) {
		root.scopes = scopes;
		root.bindings = bindings;
	}
	S.strings = S.scopes = S.bindings = [];
	S.source = '';
	return root;
}
