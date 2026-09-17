// Turns the addon's shape-coded stream into ESTree objects: what `JSON.parse` did, without the
// text. The layout is `teasel::estree::Binary`, the kinds `teasel::estree::kind`.
import { PARENT, REFERENCE, SCOPE, build_roots, comments, compile as layout_of, kept, strings as interned_strings, type Decoded, type Tree } from './arena.js';

export { PARENT, REFERENCE, SCOPE, type Decoded, type Tree };

const HEADER = 7;
// in a node's place
const NULL = 0;
const END = 1;

const little = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;
// a leading U+FEFF is text, not a mark
const utf8 = new TextDecoder('utf-8', { ignoreBOM: true });

const FACTS = new Set(['scope', 'declares', 'reference', 'defines', 'writes', 'root']);

type Builder = (S: State) => Decoded;
type Reader = (S: State) => any;

/** The engine's numbering, which the stream refers to, and the tree with its layout. */
export interface Tables {
	readonly constants: () => string[];
	readonly shapes: () => ArrayLike<number>;
	readonly layout: () => string;
	/** The views of the last parse's tree, JavaScript's or TypeScript's; `moved` when a buffer of it has since the engine last gave them. */
	readonly tree: (typescript: boolean, moved: boolean) => Tree;
}

// todo: gone with the stream's tree half, once every answer comes this way
export const mode = { arena: true };

/** A source the engine prepared: it parses at an entry and offset, cut at `end`, the stop tokens as one string; the answer is the words, or an error as JSON. */
export interface Prepared {
	readonly parse: (entry: number, offset: number, end: number | undefined, stop: string) => Uint32Array | string;
	readonly free: () => void;
}

/** What parses: the addon or the WebAssembly module, each bound to a `Source` class of its own. */
export interface Engine extends Tables {
	readonly create: (source: string, flags: number, host: string) => Prepared;
}

// one decode at a time; the builders are generated once and read through this
interface State {
	w: Uint32Array;
	at: number;
	strings: string[];
	floats: Float64Array | null;
	source: string;
	constants: string[];
	scopes: Decoded[];
	bindings: Decoded[];
	references: Decoded[];
	roots: Decoded[];
	build: Builder[];
}
const EMPTY: Decoded[] = [];

function node(S: State): Decoded | null {
	const id = S.w[S.at++];
	return id === NULL ? null : S.build[id](S);
}

function nodes(S: State): (Decoded | null)[] {
	const list = [];
	for (;;) {
		const id = S.w[S.at++];
		if (id === END) return list;
		list.push(id === NULL ? null : S.build[id](S));
	}
}

function ints(S: State): number[] {
	const n = S.w[S.at++];
	const list = new Array<number>(n);
	for (let i = 0; i < n; i++) list[i] = S.w[S.at++];
	return list;
}

function strs(S: State): string[] {
	const n = S.w[S.at++];
	const list = new Array<string>(n);
	for (let i = 0; i < n; i++) list[i] = S.strings[S.w[S.at++]];
	return list;
}

interface Shape {
	type: string | null;
	keys: string[];
	kinds: number[];
}

// one reader per kind, as source for the generated builders and as a function for the interpreter
const READ = ['node(S)', 'S.w[S.at++]', 'S.floats[S.w[S.at++]]', 'S.w[S.at++] === 1', 'S.constants[S.w[S.at++]]', 'S.strings[S.w[S.at++]]', 'S.source.slice(S.w[S.at++], S.w[S.at++])', '{ start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } }', 'nodes(S)', 'ints(S)', 'strs(S)'];
const READERS: Reader[] = [node, (S) => S.w[S.at++], (S) => S.floats![S.w[S.at++]], (S) => S.w[S.at++] === 1, (S) => S.constants[S.w[S.at++]], (S) => S.strings[S.w[S.at++]], (S) => S.source.slice(S.w[S.at++], S.w[S.at++]), (S) => ({ start: { line: S.w[S.at++], column: S.w[S.at++] }, end: { line: S.w[S.at++], column: S.w[S.at++] } }), nodes, ints, strs];

// One object literal per shape, its facts and its parent link as symbol slots of the literal:
// V8 allocates it in one hidden class with nothing added later. Facts, and everything the stream
// puts before the last of them, are read into locals first.
function generate({ type, keys, kinds }: Shape, link: boolean): Builder {
	let last = -1;
	if (link && type !== null) for (let i = 0; i < keys.length; i++) if (FACTS.has(keys[i]) || kinds[i] === 0 || kinds[i] === 8) last = i;
	const lead = [];
	const props = type === null ? [] : [`type: ${JSON.stringify(type)}`];
	const linked = link && type === null ? LINKED.find((row) => row.mark.every((key) => keys.includes(key))) : undefined;
	// what the node points at, set once it exists
	const after = [];
	let scope = null, reference = null;
	for (let i = 0; i < keys.length; i++) {
		const key = keys[i];
		if (i > last) props.push(`${JSON.stringify(key)}: ${READ[kinds[i]]}`);
		else {
			lead.push(`const v${i} = ${READ[kinds[i]]};`);
			if (key === 'scope') { scope = `S.scopes[v${i}]`; lead.push(`const s = ${scope};`); after.push('s.node = n;'); }
			else if (key === 'declares') { reference = 'd'; lead.push(`const d = S.bindings[v${i}];`); after.push('if (d.node === null) d.node = n;'); }
			else if (key === 'reference') { reference = 'r'; lead.push(`const r = S.references[v${i}];`); after.push('r.node = n;'); }
			else if (key === 'defines') after.push(`for (let i = 0; i < v${i}.length; i++) { const d = S.bindings[v${i}[i]]; d.declaration = n; if (n.init !== undefined) d.writeExpr = n.init; }`);
			else if (key === 'writes') after.push(`for (let i = 0; i < v${i}.length; i++) S.references[v${i}[i]].writeExpr = n;`);
			else if (key === 'root') after.push(`S.roots[v${i}].node = n;`);
			else {
				props.push(`${JSON.stringify(key)}: v${i}`);
				// a child with a type is a node; a literal's regex or a template element's value is not
				if (link && kinds[i] === 0) after.push(`if (v${i} !== null && v${i}.type !== undefined) v${i}[PARENT] = n;`);
				else if (link && kinds[i] === 8) after.push(`for (let i = 0; i < v${i}.length; i++) if (v${i}[i] !== null) v${i}[i][PARENT] = n;`);
			}
		}
	}
	if (linked !== undefined) props.push(...linked.props);
	if (link && type !== null) {
		props.push('[PARENT]: undefined');
		if (scope !== null) props.push('[SCOPE]: s');
		// every identifier has the two slots, so those with facts and those without share a class
		if (type === 'Identifier' || reference !== null) props.push(`[REFERENCE]: ${reference ?? 'undefined'}`);
	}
	const body = `${lead.join(' ')} const n = { ${props.join(', ')} }; ${after.join(' ')} return n;`;
	return new Function('node', 'nodes', 'ints', 'strs', 'PARENT', 'SCOPE', 'REFERENCE', `return (S) => { ${body} };`)(node, nodes, ints, strs, PARENT, SCOPE, REFERENCE);
}

// what a table row points at once the tree is built, in its literal from the start so nothing is
// added later; a binding is the reference its declaring identifier makes, `binding` itself
const LINKED = [
	{ mark: ['topLevelAwait'], props: ['node: null'], values: { node: null } },
	{ mark: ['name', 'kind'], props: ['node: null', 'declaration: null', 'binding: null', 'declares: true', 'read: false', 'mutate: false', 'writeExpr: null'], values: { node: null, declaration: null, binding: null, declares: true, read: false, mutate: false, writeExpr: null } },
	{ mark: ['mutate'], props: ['node: null', 'writeExpr: null'], values: { node: null, writeExpr: null } },
];

// the same without code generation, for a host whose policy forbids it
function interpret({ type, keys, kinds }: Shape, link: boolean): Builder {
	const linked = link && type !== null;
	const row = link && type === null ? LINKED.find((row) => row.mark.every((key) => keys.includes(key))) : undefined;
	return (S) => {
		const n: Decoded = type === null ? {} : linked ? { type, [PARENT]: undefined, [SCOPE]: undefined, [REFERENCE]: undefined } : { type };
		// what a declaration initializes is a child of it, in place only once every key is read
		let defines = null;
		for (let i = 0; i < keys.length; i++) {
			const key = keys[i];
			const value = READERS[kinds[i]](S);
			if (linked && kinds[i] === 0 && value !== null && value.type !== undefined) value[PARENT] = n;
			else if (linked && kinds[i] === 8) for (const child of value) if (child !== null) child[PARENT] = n;
			if (!linked || !FACTS.has(key)) n[key] = value;
			else if (key === 'scope') { const s = S.scopes[value]; n[SCOPE] = s; s.node = n; }
			else if (key === 'declares') { const d = S.bindings[value]; n[REFERENCE] = d; if (d.node === null) d.node = n; }
			else if (key === 'reference') { const r = S.references[value]; n[REFERENCE] = r; r.node = n; }
			else if (key === 'defines') defines = value;
			else if (key === 'root') S.roots[value].node = n;
			else for (const w of value) S.references[w].writeExpr = n;
		}
		if (defines !== null) for (const b of defines) { const d = S.bindings[b]; d.declaration = n; if (n.init !== undefined) d.writeExpr = n.init; }
		if (row !== undefined) Object.assign(n, row.values);
		return n;
	};
}

const compile: (shape: Shape, link: boolean) => Builder = (() => {
	try {
		new Function('');
		return generate;
	} catch {
		return interpret;
	}
})();

interface Table {
	constants: string[];
	shapes: Shape[];
	linked: Builder[];
	plain: Builder[];
}
// ids 0 and 1 are NULL and END, shapes of nothing
const NONE: Shape = { type: null, keys: [], kinds: [] };
const tables = new WeakMap<Tables, Table>();

function table_of(engine: Tables, known: number, known_shapes: number) {
	let table = tables.get(engine);
	if (table === undefined) tables.set(engine, (table = { constants: [], shapes: [NONE, NONE], linked: [], plain: [] }));
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

function builders(table: Table, link: boolean) {
	const list = link ? table.linked : table.plain;
	while (list.length < table.shapes.length) list.push(compile(table.shapes[list.length], link));
	return list;
}

function unaligned_floats(buffer: ArrayBufferLike, start: number, count: number) {
	const view = new DataView(buffer, start, count * 8);
	const floats = new Float64Array(count);
	for (let i = 0; i < count; i++) floats[i] = view.getFloat64(i * 8, little);
	return floats;
}

// every row arrives with its links in place as nulls, so nothing here adds a property
function link_tables(scopes: Decoded[], bindings: Decoded[], references: Decoded[]) {
	for (const scope of scopes) scope.parent = scope.parent === null ? null : scopes[scope.parent];
	// a binding is its own first declaration: the reference the declaring identifier makes
	for (const binding of bindings) {
		binding.scope = scopes[binding.scope];
		binding.binding = binding;
	}
	for (const reference of references) {
		reference.scope = scopes[reference.scope];
		reference.binding = reference.binding === null ? null : bindings[reference.binding];
	}
}

function link_roots(roots: Decoded[], scopes: Decoded[], bindings: Decoded[], references: Decoded[]) {
	for (const root of roots) {
		root.node = null;
		root.scope = scopes[root.scope];
		root.scopes = scopes.slice(root.scopes[0], root.scopes[1]);
		root.bindings = bindings.slice(root.bindings[0], root.bindings[1]);
		root.references = references.slice(root.references[0], root.references[1]);
	}
}

/** The answer's words, or a view of them inside a larger buffer; `link` replaces the scope and binding numbers with the objects they index. */
export function decode(words: Uint32Array, source: string, engine: Tables, link = true, arena = false): Decoded {
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
	// the tree's strings come first, then the words' own
	// an answer read in place ends with the views: a word of what they are, then each one's length
	const lens = floats_at + 2 * floats_count;
	const C = arena ? layout_of(engine) : null;
	const arena_tree = C === null ? null : engine.tree((words[lens] & 2) !== 0, (words[lens] & 1) !== 0);
	const interned = C === null ? null : interned_strings(C, arena_tree!, words, lens);
	const strings = interned === null ? new Array<string>(ends_count) : interned.slice();
	const first = interned === null ? 0 : interned.length;
	let from = 0;
	for (let i = 0; i < ends_count; i++) {
		const end = words[HEADER + tree + i];
		strings[first + i] = text.slice(from, end);
		from = end;
	}
	// one state object per decode, young like everything it points at: no write barriers
	const S: State = { w: words, at: HEADER, strings, floats, source, constants: table.constants, scopes: EMPTY, bindings: EMPTY, references: EMPTY, roots: EMPTY, build: builders(table, link) };
	let scopes = null, bindings = null, references = null, roots = null;
	if (tables_at !== 0) {
		// the writer's `all_scopes` order; the roots table is there when a host document has pieces of JavaScript
		S.at = HEADER + tables_at;
		scopes = nodes(S) as Decoded[];
		bindings = nodes(S) as Decoded[];
		references = nodes(S) as Decoded[];
		if (S.at < HEADER + tree) roots = nodes(S) as Decoded[];
		if (link) {
			link_tables(scopes, bindings, references);
			if (roots !== null) link_roots(roots, scopes, bindings, references);
		}
		S.scopes = scopes;
		S.bindings = bindings;
		S.references = references;
		if (roots !== null) S.roots = roots;
		S.at = HEADER;
	}
	let root = node(S)!;
	if (C !== null) {
		// the answer's `output`: every comment listed, TypeScript erased, lines, the roots a list
		const listed = (root.output & 8) !== 0;
		const built = build_roots(C, arena_tree!, words, lens, root.node, listed, interned!, { source, constants: table.constants, link, erase: (root.output & 2) !== 0, lines: (root.output & 4) !== 0, scopes: S.scopes, bindings: S.bindings, references: S.references, roots: S.roots });
		const answer: Decoded = { node: listed ? built.nodes : built.nodes[0], end: root.end };
		if ((root.output & 1) !== 0) answer.comments = comments(built);
		if (root.errors !== undefined) answer.errors = root.errors;
		if ((root.output & 2) !== 0) answer.typescript = kept(built);
		root = answer;
	}
	if (scopes !== null) {
		root.scopes = scopes;
		root.bindings = bindings;
		root.references = references;
		if (roots !== null) root.roots = roots;
	}
	return root;
}
