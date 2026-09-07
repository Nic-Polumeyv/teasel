// Turns the addon's shape-coded stream into ESTree objects: what `JSON.parse` did, without the
// text. The layout is `teasel::estree::Binary`, the kinds `teasel::estree::kind`.
const HEADER = 7;
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
// a reference's write and mutate bits, until someone asks for the reference
const FACTS = Symbol('facts');
// the answer's tables, on each scope and binding, for what they derive from them
const TABLES = Symbol('tables');
const THROUGH = Symbol('through');
const OWN_BINDINGS = Symbol('bindings');
const OWN_REFERENCES = Symbol('references');

/** @param {import('estree').Node} node @returns {import('./index.js').Scope | undefined} the scope the node opens */
export const scopeOf = (node) => (node == null ? undefined : node[SCOPE]);
/** @param {import('estree').Node} node @returns {import('./index.js').Binding | null | undefined} what the identifier declares or refers to; null for a global, undefined when it names no value */
export const bindingOf = (node) => (node == null ? undefined : node[BINDING]);
/** @param {import('estree').Node} node @returns {import('./index.js').Reference | undefined} the reference an identifier makes, a global's included */
export function referenceOf(node) {
	if (node == null) return undefined;
	let reference = node[REFERENCE];
	if (reference === undefined && node[FACTS] !== undefined) {
		const facts = node[FACTS];
		reference = node[REFERENCE] = { node, binding: node[BINDING], write: (facts & 1) !== 0, mutate: (facts & 2) !== 0 };
	}
	return reference;
}

/** One pass over an answer's tables, the first time anything derived from them is asked for. */
function index(tables) {
	if (tables.indexed) return;
	tables.indexed = true;
	for (const binding of tables.bindings) (binding.scope[OWN_BINDINGS] ??= []).push(binding);
	for (const node of tables.references) {
		const binding = node[BINDING];
		if (binding !== null) (binding[OWN_REFERENCES] ??= []).push(node);
	}
}

/**
 * A property derived on first read and kept under a symbol, so the keys of a scope or binding
 * never change; a host may assign its own.
 * @param {object} proto @param {string} key @param {(self: any) => any} derive
 */
function derived(proto, key, derive) {
	const slot = Symbol(key);
	Object.defineProperty(proto, key, {
		get() {
			return (this[slot] ??= derive(this));
		},
		set(value) {
			this[slot] = value;
		}
	});
}

// what a host reads is on the object; what it derives is paid once by whoever asks
const Scope = { node: null };
derived(Scope, 'bindings', (s) => (index(s[TABLES]), s[OWN_BINDINGS] ?? []));
derived(Scope, 'declarations', (s) => new Map(s.bindings.map((b) => [b.name, b])));
derived(Scope, 'through', (s) => s[THROUGH].map((i) => s[TABLES].bindings[i]));
const Binding = { node: null };
derived(Binding, 'references', (b) => (index(b[TABLES]), (b[OWN_REFERENCES] ?? []).map(referenceOf)));

const FACT_KEYS = new Set(['scope', 'binding', 'declares', 'write', 'mutate']);

/**
 * One decode at a time; the builders are generated once and read through this.
 * @type {{ w: Uint32Array, at: number, strings: string[], floats: Float64Array | null, source: string, constants: string[], scopes: any[], bindings: any[], references: any[], build: (() => any)[] }}
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
	references: [],
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
		n[BINDING] = binding === null ? null : S.bindings[binding];
		n[FACTS] = (write ? 1 : 0) | (mutate ? 2 : 0);
		S.references.push(n);
	}
}

/** @typedef {{ type: string | null, keys: string[], kinds: number[] }} Shape */

// one reader per kind, as source for the generated builders and as a function for the interpreter
const READ = ['node()', 'S.w[S.at++]', 'S.floats[S.w[S.at++]]', 'S.w[S.at++] === 1', 'S.constants[S.w[S.at++]]', 'S.strings[S.w[S.at++]]', 'S.source.slice(S.w[S.at++], S.w[S.at++])', '{ start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } }', 'nodes()', 'ints()'];
const READERS = [node, () => S.w[S.at++], () => /** @type {Float64Array} */ (S.floats)[S.w[S.at++]], () => S.w[S.at++] === 1, () => S.constants[S.w[S.at++]], () => S.strings[S.w[S.at++]], () => S.source.slice(S.w[S.at++], S.w[S.at++]), () => ({ start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } }), nodes, ints];

/**
 * One object literal per shape: V8 allocates it in one hidden class. Facts, and everything the
 * stream puts before the last of them, are read into locals first, so the object never holds them.
 * @param {Shape} shape @param {boolean} link
 */
function generate({ type, keys, kinds }, link) {
	let last = -1;
	if (link && type !== null) for (let i = 0; i < keys.length; i++) if (FACT_KEYS.has(keys[i])) last = i;
	const lead = [];
	const row = link && type === null ? row_proto(keys) : null;
	const props = row !== null ? [`__proto__: ${row}`] : type === null ? [] : [`type: ${JSON.stringify(type)}`];
	const facts = { scope: 'undefined', declares: 'undefined', binding: 'undefined', write: 'false', mutate: 'false' };
	const name = (key) => (row !== null && key === 'through' ? '[THROUGH]' : JSON.stringify(key));
	for (let i = 0; i < keys.length; i++) {
		const key = keys[i];
		if (i > last) props.push(`${name(key)}: ${READ[kinds[i]]}`);
		else {
			lead.push(`const v${i} = ${READ[kinds[i]]};`);
			if (FACT_KEYS.has(key)) facts[key] = `v${i}`;
			else props.push(`${name(key)}: v${i}`);
		}
	}
	const body = `${lead.join(' ')} const n = { ${props.join(', ')} }; ${last < 0 ? '' : `file(n, ${facts.scope}, ${facts.declares}, ${facts.binding}, ${facts.write}, ${facts.mutate});`} return n;`;
	return new Function('S', 'node', 'nodes', 'ints', 'file', 'Scope', 'Binding', 'THROUGH', `return () => { ${body} };`)(S, node, nodes, ints, file, Scope, Binding, THROUGH);
}

/** A shape without a type is a scope or binding row, or a plain object such as a literal's `regex`. */
const ROWS = { 'kind,parent,functionDepth,through': 'Scope', 'name,kind,scope': 'Binding' };
const row_proto = (keys) => ROWS[keys.join(',')] ?? null;

/** The same without code generation, for a host whose policy forbids it. @param {Shape} shape @param {boolean} link */
function interpret({ type, keys, kinds }, link) {
	const facts = link && type !== null && keys.some((key) => FACT_KEYS.has(key));
	const row = link && type === null ? row_proto(keys) : null;
	const proto = row === 'Scope' ? Scope : row === 'Binding' ? Binding : null;
	return () => {
		const n = proto !== null ? Object.create(proto) : type === null ? {} : { type };
		let scope, declares, binding, write = false, mutate = false;
		for (let i = 0; i < keys.length; i++) {
			const key = keys[i];
			const value = READERS[kinds[i]]();
			if (proto !== null && key === 'through') n[THROUGH] = value;
			else if (!facts || !FACT_KEYS.has(key)) n[key] = value;
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
	if (list.length === 0) list.push(null, null);
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
	const tables = { bindings, references: [], indexed: false };
	for (const scope of scopes) {
		scope.parent = scope.parent === null ? null : scopes[scope.parent];
		scope[TABLES] = tables;
	}
	for (const binding of bindings) {
		binding.scope = scopes[binding.scope];
		binding[TABLES] = tables;
	}
	return tables.references;
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
	const text_at = HEADER + tree + ends_count;
	const text = bytes ? decode_text(buffer, byteOffset + text_at * 4, bytes) : '';
	let floats_at = text_at + ((bytes + 3) >> 2);
	if (floats_at % 2 === 1) floats_at++;
	const floats_start = byteOffset + floats_at * 4;
	const floats = !floats_count ? null : floats_start % 8 === 0 ? new Float64Array(buffer, floats_start, floats_count) : unaligned_floats(buffer, floats_start, floats_count);
	const strings = new Array(ends_count);
	let from = 0;
	for (let i = 0; i < ends_count; i++) {
		const end = words[HEADER + tree + i];
		strings[i] = text.slice(from, end);
		from = end;
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
		if (link) S.references = link_tables(scopes, bindings);
		S.scopes = scopes;
		S.bindings = bindings;
	}
	S.at = HEADER;
	const root = node();
	if (scopes !== null) {
		root.scopes = scopes;
		root.bindings = bindings;
	}
	S.strings = [];
	S.scopes = [];
	S.bindings = [];
	S.references = [];
	S.source = '';
	return root;
}

let bytes_view = new Uint8Array(0);

/** The text block as a string; a short ASCII one is read byte by byte, under the decoder's fixed cost. */
function decode_text(buffer, at, length) {
	if (bytes_view.buffer !== buffer) bytes_view = new Uint8Array(buffer);
	if (length <= 24) {
		let text = '';
		for (let i = at; i < at + length; i++) {
			const byte = bytes_view[i];
			if (byte >= 0x80) return utf8.decode(bytes_view.subarray(at, at + length));
			text += String.fromCharCode(byte);
		}
		return text;
	}
	return utf8.decode(bytes_view.subarray(at, at + length));
}
