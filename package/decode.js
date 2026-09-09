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
const PARENT = Symbol('parent');


/** @param {import('estree').Node} node @returns {import('./index.js').Scope | undefined} the scope the node opens */
export const scopeOf = (node) => (node == null ? undefined : node[SCOPE]);
/** @param {import('estree').Node} node @returns {import('./index.js').Binding | null | undefined} what the identifier declares or refers to; null for a global, undefined when it names no value */
export const bindingOf = (node) => (node == null ? undefined : node[BINDING]);
/** @param {import('estree').Node} node @returns {import('./index.js').Reference | undefined} the reference an identifier makes, a global's included */
export const referenceOf = (node) => (node == null ? undefined : node[REFERENCE]);
/** @param {import('estree').Node} node @returns {import('estree').Node | undefined} the node it is a child of; undefined for the root of an answer */
export const parentOf = (node) => (node == null ? undefined : node[PARENT]);

const FACTS = new Set(['scope', 'declares', 'reference', 'defines', 'writes']);

/** Files a node built outside the stream as the decoder would: no parent, and its facts when it has them. */
export function facts(node, scope, binding, reference) {
	node[PARENT] = undefined;
	if (scope !== undefined) node[SCOPE] = scope;
	if (node.type === 'Identifier' || binding !== undefined) node[BINDING] = binding === null ? null : binding;
	if (node.type === 'Identifier' || reference !== undefined) node[REFERENCE] = reference === null ? undefined : reference;
}

/**
 * One decode at a time; the builders are generated once and read through this.
 * @type {{ w: Uint32Array, at: number, strings: string[], floats: Float64Array | null, source: string, constants: string[], scopes: any[], bindings: any[], build: (() => any)[] }}
 */
const EMPTY = [];

function node(S) {
	const id = S.w[S.at++];
	return id === NULL ? null : S.build[id](S);
}

function nodes(S) {
	const list = [];
	for (;;) {
		const id = S.w[S.at++];
		if (id === END) return list;
		list.push(id === NULL ? null : S.build[id](S));
	}
}

function ints(S) {
	const n = S.w[S.at++];
	const list = new Array(n);
	for (let i = 0; i < n; i++) list[i] = S.w[S.at++];
	return list;
}

/** @typedef {{ type: string | null, keys: string[], kinds: number[] }} Shape */

// one reader per kind, as source for the generated builders and as a function for the interpreter
const READ = ['node(S)', 'S.w[S.at++]', 'S.floats[S.w[S.at++]]', 'S.w[S.at++] === 1', 'S.constants[S.w[S.at++]]', 'S.strings[S.w[S.at++]]', 'S.source.slice(S.w[S.at++], S.w[S.at++])', '{ start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } }', 'nodes(S)', 'ints(S)'];
const READERS = [node, (S) => S.w[S.at++], (S) => /** @type {Float64Array} */ (S.floats)[S.w[S.at++]], (S) => S.w[S.at++] === 1, (S) => S.constants[S.w[S.at++]], (S) => S.strings[S.w[S.at++]], (S) => S.source.slice(S.w[S.at++], S.w[S.at++]), (S) => ({ start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } }), nodes, ints];

/**
 * One object literal per shape, its facts and its parent link as symbol slots of the literal:
 * V8 allocates it in one hidden class with nothing added later. Facts, and everything the stream
 * puts before the last of them, are read into locals first.
 * @param {Shape} shape @param {boolean} link
 */
function generate({ type, keys, kinds }, link) {
	let last = -1;
	if (link && type !== null) for (let i = 0; i < keys.length; i++) if (FACTS.has(keys[i]) || kinds[i] === 0 || kinds[i] === 8) last = i;
	const lead = [];
	const props = type === null ? [] : [`type: ${JSON.stringify(type)}`];
	// what the node points at, set once it exists
	const after = [];
	let scope = null, binding = null, reference = null;
	for (let i = 0; i < keys.length; i++) {
		const key = keys[i];
		if (i > last) props.push(`${JSON.stringify(key)}: ${READ[kinds[i]]}`);
		else {
			lead.push(`const v${i} = ${READ[kinds[i]]};`);
			if (key === 'scope') { scope = `S.scopes[v${i}]`; lead.push(`const s = ${scope};`); after.push('s.node = n;'); }
			else if (key === 'declares') { binding = 'd'; lead.push(`const d = S.bindings[v${i}];`); after.push('if (d.node === null) d.node = n;'); }
			else if (key === 'reference') { reference = 'r'; binding = 'r.binding'; lead.push(`const r = S.references[v${i}];`); after.push('r.node = n;'); }
			else if (key === 'defines') after.push(`for (let i = 0; i < v${i}.length; i++) S.bindings[v${i}[i]].declaration = n;`);
			else if (key === 'writes') after.push(`for (let i = 0; i < v${i}.length; i++) S.references[v${i}[i]].writeExpr = n;`);
			else {
				props.push(`${JSON.stringify(key)}: v${i}`);
				// a child with a type is a node; a literal's regex or a template element's value is not
				if (link && kinds[i] === 0) after.push(`if (v${i} !== null && v${i}.type !== undefined) v${i}[PARENT] = n;`);
				else if (link && kinds[i] === 8) after.push(`for (let i = 0; i < v${i}.length; i++) if (v${i}[i] !== null) v${i}[i][PARENT] = n;`);
			}
		}
	}
	if (link && type !== null) {
		props.push('[PARENT]: undefined');
		if (scope !== null) props.push('[SCOPE]: s');
		// every identifier has the two slots, so those with facts and those without share a class
		if (type === 'Identifier' || binding !== null) props.push(`[BINDING]: ${binding ?? 'undefined'}`);
		if (type === 'Identifier' || reference !== null) props.push(`[REFERENCE]: ${reference ?? 'undefined'}`);
	}
	const body = `${lead.join(' ')} const n = { ${props.join(', ')} }; ${after.join(' ')} return n;`;
	return new Function('node', 'nodes', 'ints', 'PARENT', 'SCOPE', 'BINDING', 'REFERENCE', `return (S) => { ${body} };`)(node, nodes, ints, PARENT, SCOPE, BINDING, REFERENCE);
}

/** The same without code generation, for a host whose policy forbids it. @param {Shape} shape @param {boolean} link */
function interpret({ type, keys, kinds }, link) {
	const linked = link && type !== null;
	return (S) => {
		const n = type === null ? {} : linked ? { type, [PARENT]: undefined, [SCOPE]: undefined, [BINDING]: undefined, [REFERENCE]: undefined } : { type };
		for (let i = 0; i < keys.length; i++) {
			const key = keys[i];
			const value = READERS[kinds[i]](S);
			if (linked && kinds[i] === 0 && value !== null && value.type !== undefined) value[PARENT] = n;
			else if (linked && kinds[i] === 8) for (const child of value) if (child !== null) child[PARENT] = n;
			if (!linked || !FACTS.has(key)) n[key] = value;
			else if (key === 'scope') { const s = S.scopes[value]; n[SCOPE] = s; s.node = n; }
			else if (key === 'declares') { const d = S.bindings[value]; n[BINDING] = d; if (d.node === null) d.node = n; }
			else if (key === 'reference') { const r = S.references[value]; n[REFERENCE] = r; n[BINDING] = r.binding; r.node = n; }
			else if (key === 'defines') for (const b of value) S.bindings[b].declaration = n;
			else for (const w of value) S.references[w].writeExpr = n;
		}
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

/** @param {any[]} scopes @param {any[]} bindings @param {any[]} references */
function link_tables(scopes, bindings, references) {
	for (const scope of scopes) {
		scope.parent = scope.parent === null ? null : scopes[scope.parent];
		scope.node = null;
	}
	for (const binding of bindings) {
		binding.scope = scopes[binding.scope];
		binding.node = null;
		binding.declaration = null;
	}
	for (const reference of references) {
		reference.scope = scopes[reference.scope];
		reference.binding = reference.binding === null ? null : bindings[reference.binding];
		reference.node = null;
		reference.writeExpr = null;
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
	// read by index: destructuring a typed array goes through its iterator, a tenth of a small decode
	const tree = words[0], ends_count = words[1], floats_count = words[2], bytes = words[3], known = words[4], known_shapes = words[5], tables_at = words[6];
	const table = table_of(engine, known, known_shapes);
	const text_at = HEADER + tree + ends_count;
	const text = bytes ? utf8.decode(new Uint8Array(buffer, byteOffset + text_at * 4, bytes)) : '';
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
	// one state object per decode, young like everything it points at: no write barriers
	const S = { w: words, at: HEADER, strings, floats, source, constants: table.constants, scopes: EMPTY, bindings: EMPTY, references: EMPTY, build: builders(table, link) };
	let scopes = null, bindings = null, references = null;
	if (tables_at !== 0) {
		// the writer's `all_scopes` order; a fourth table would have to carry its key
		S.at = HEADER + tables_at;
		scopes = nodes(S);
		bindings = nodes(S);
		references = nodes(S);
		if (link) link_tables(scopes, bindings, references);
		S.scopes = scopes;
		S.bindings = bindings;
		S.references = references;
		S.at = HEADER;
	}
	const root = node(S);
	if (scopes !== null) {
		root.scopes = scopes;
		root.bindings = bindings;
		root.references = references;
	}
	return root;
}
