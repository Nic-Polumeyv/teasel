// Builds ESTree objects from the parser's own tree, read in place: the layout says where each
// kind's fields sit, the recipes how the kind is spelled.

import type { Tree, Views } from './types.ts';
import { CODES } from './types.ts';
import { SLOT, type Options } from './options.ts';

// symbol keys: ten times cheaper than a WeakMap entry, and skipped by JSON, Object.keys and for-in
export const SCOPE = Symbol('scope');
export const REFERENCE = Symbol('reference');
export const PARENT = Symbol('parent');

/** A decoded object: the tree decides its shape, `types.ts` describes it. */
type Decoded = Record<string | symbol, any>;

export interface Field {
	name: string;
	at: number;
	ty: string;
	names?: string[];
	fields?: Field[];
}
export interface Kind {
	name: string;
	fields: Field[];
}
interface Tagged {
	tag: number;
	missing: number;
}
/** How the compiler spells a missing optional of each type. */
interface Missing {
	enum: number;
	list: Tagged;
	str: Tagged;
	int: Tagged;
}
export type RawOp = [string, ...any[]];
export type RawRecipes = [string, RawOp[]][];
export interface Layout {
	node: { size: number; start: number; end: number; kind: number };
	kinds: Kind[];
	ts?: { size: number; kinds: Kind[] };
	extras?: { size: number; fields: Field[] };
	/** Each table's record: its size and its fields. */
	rows: Record<string, { size: number; fields: Field[] }>;
	/** The records beside the nodes and the tables, by their view's name. */
	records: Record<'comments' | 'attached' | 'errors' | 'hosts' | 'host_vals' | 'spans' | 'locs' | 'marks', { size: number; fields: Field[] }>;
	none: Missing;
	views: { js: string[]; ts?: string[] };
	/** Each type of node a stylesheet holds with the fields that hold nodes. */
	stylesheet: Record<string, string[]>;
	recipes: { js: RawRecipes; rows: RawRecipes; ts?: RawRecipes; adds?: RawRecipes; ts_adds?: RawRecipes; erased?: RawOp[] };
}

// one class for every operation: the interpreter's switch stays monomorphic
interface Op {
	op: string;
	key: string;
	/** Byte offset of the field from the record's start. */
	at: number;
	ty: string;
	names: string[];
	/** The binding's field of `othername`. */
	at2: number;
	/** What `constbool` spells, `boolnames`' name for true, or a `host` value's tag. */
	value: any;
	/** `boolnames`' name for false, or what `enumor` spells when the byte names none. */
	other: string;
	inner: Op[];
}

interface Language {
	layout: Layout;
	/** Each view's place in the tree, by name. */
	at: At;
	/** By a kind's tag, resolved the first time the kind is met. */
	recipes: (Op[] | undefined)[];
	ts: Op[][];
	/** What TypeScript adds to each kind, by the JavaScript kind's tag and by the TypeScript kind's. */
	adds: Op[][];
	ts_adds: Op[][];
	erased: Op[];
	/** The builders of each way to build: linked, with lines, with facts, erasing; the last asked for kept near. */
	sets: Map<number, Builders>;
	last: number;
	builders: Builders | undefined;
	/** The names of the hosts' types and keys, by the number the engine gave each; they only grow. */
	names: string[];
	/** The hosts' shapes as operations, by the number the engine gave each shape. */
	host_ops: Map<number, Op[]>;
	/** The state of the last answer read from `tree`, kept for the next. */
	state: State | undefined;
	tree: Tree | undefined;
	/** Where the tree's buffers sat when the state was taken, as the answer folds it. */
	sat: [number, number];
}

/** A node from its id, its TypeScript record when it has one, and the node it sits in. */
type Builder = (S: State, id: number, record: number, parent: Decoded | undefined) => Decoded;
type Table = 'scopes' | 'bindings' | 'references' | 'roots';
/** A table's row from its record, `b` words into the view, and the table it joins: a linked row reads the rows before it. */
type Row = (S: State, view: Uint32Array, b: number, table: Decoded[]) => Decoded;
/** A host's node from its fields, `from` on in the hosts' keys and values. */
type HostBuilder = (S: State, id: number, index: number, from: number, parent: Decoded | undefined) => Decoded;
interface Rows {
	plain: Row;
	linked: Row;
	words: number;
}
interface Builders {
	/** By a node's tag. */
	js: Builder[];
	/** The builder of each shape of the hosts' nodes, by the number the engine gave it. */
	hosts: (HostBuilder | undefined)[];
	/** By an extension record's tag, the record's first word given. */
	ts: Builder[];
}

interface Compiled {
	layout: Layout;
	words: number;
	kind: number;
	extension: number;
	host: number;
	identifier: number;
	name: number;
	/** An extras record with nothing set. */
	blank: Uint32Array;
	R: Records;
	/** The word of a node holding a host's or an extension's index, `Host`'s and `Extension`'s field alike. */
	payload: number;
	/** A node's `loc` as a literal's entry, read from its record in `locs`. */
	LOC: string;
	/** Each table's row builders, plain and linked, and its record's words; made on the first answer with scopes. */
	rows: Record<keyof typeof LINKED, Rows> | undefined;
	js: Language;
	/** Made on the first TypeScript answer. */
	ts: Language | undefined;
}

/** A field by its path, `a` or `a.b.c`, an inner field at its offset in the record. */
export function field(fields: Field[], path: string): Field {
	let found: Field | undefined, at = 0;
	for (const name of path.split('.')) {
		found = (found === undefined ? fields : found.fields!).find((f) => f.name === name)!;
		at += found.at;
	}
	return { ...found!, at };
}

function resolve(raw: RawOp[], fields: Field[]): Op[] {
	const find = (path: string) => field(fields, path);
	return raw.map(([op, ...rest]) => {
		const out: Op = { op, key: '', at: 0, ty: '', names: [], at2: 0, value: null, other: '', inner: [] };
		let field: string | undefined;
		if (op === 'typeof' || op === 'through') [field] = rest;
		else if (op === 'object') [out.key, out.inner] = [rest[0], resolve(rest[1], fields)];
		else if (op === 'constbool') [out.key, out.value] = rest;
		else if (op === 'boolnames') [out.key, field, out.value, out.other] = rest;
		else if (op === 'enumor') [out.key, field, out.other] = rest;
		else if (op === 'othername' || op === 'regexp') {
			[out.key, field] = rest;
			out.at2 = find(rest[2]).at;
		} else [out.key, field] = rest;
		if (field !== undefined) {
			const found = find(field);
			out.at = found.at;
			out.ty = found.ty;
			out.names = found.names ?? [];
		}
		return out;
	});
}

// a literal: an object filled by computed keys turns into a dictionary, a hash lookup per read
function places(views: string[]) {
	const of = (name: string) => 1 + views.indexOf(name);
	return { nodes: of('nodes'), lists: of('lists'), numbers: of('numbers'), text: of('text'), starts: of('starts'), marks: of('marks'), units: of('units'), spans: of('spans'), locs: of('locs'), parenthesized: of('parenthesized'), erased: of('erased'), comments: of('comments'), attached_slots: of('attached_slots'), attached: of('attached'), errors: of('errors'), hosts: of('hosts'), host_keys: of('host_keys'), host_vals: of('host_vals'), host_strings: of('host_strings'), scopes: of('scopes'), bindings: of('bindings'), references: of('references'), roots: of('roots'), of_node: of('of_node'), of_identifier: of('of_identifier'), root_of: of('root_of'), declared_by: of('declared_by'), declared_by_at: of('declared_by_at'), writes_of: of('writes_of'), writes_of_at: of('writes_of_at'), ts: of('ts'), extras_slots: of('extras_slots'), extras: of('extras'), names: of('names'), name_starts: of('name_starts'), rare: of('rare'), late: of('late'), /** Past the last view's length in the answer's words: where the tree's buffers sit, folded into two. */ sits: 1 + views.length };
}
/** Each view's place in a tree; zero for a view the tree does not have. */
type At = ReturnType<typeof places>;

function language(layout: Layout, views: string[], typescript: boolean): Language {
	const by = (recipes: RawRecipes, kinds: Kind[]) => kinds.map((kind) => resolve(recipes.find(([name]) => name === kind.name)?.[1] ?? [], kind.fields));
	const out: Language = { layout, at: places(views), recipes: new Array(layout.kinds.length), ts: [], adds: [], ts_adds: [], erased: [], sets: new Map(), last: -1, builders: undefined, names: [], host_ops: new Map(), state: undefined, tree: undefined, sat: [-1, -1] };
	if (typescript) {
		const extras = layout.extras!.fields;
		const adds = (recipes: RawRecipes, kinds: Kind[]) => kinds.map((kind) => resolve(recipes.find(([name]) => name === kind.name)?.[1] ?? [], extras));
		out.ts = by(layout.recipes.ts!, layout.ts!.kinds);
		out.adds = adds(layout.recipes.adds!, layout.kinds);
		out.ts_adds = adds(layout.recipes.ts_adds!, layout.ts!.kinds);
		out.erased = resolve(layout.recipes.erased!, extras);
	}
	return out;
}

// each record's words, and its fields' offsets in words, a byte's in bytes
function records(layout: Layout) {
	const R = layout.records;
	const of = (name: keyof Layout['records'], path: string) => field(R[name].fields, path);
	const word = (name: keyof Layout['records'], path: string) => of(name, path).at >> 2;
	const words = (name: keyof Layout['records']) => R[name].size >> 2;
	// an optional's tag word and its value's, as the layout spells a missing one of its type
	const optional = (name: keyof Layout['records'], path: string) => {
		const field = of(name, path);
		const none = field.ty === '?str' ? layout.none.str : field.ty === '?u32' ? layout.none.int : undefined;
		if (none === undefined) throw new Error(`${name}.${path} is not an optional the decoder reads`);
		const [tag, value] = tagged(none, field.at);
		return { tag: tag >> 2, value: value >> 2, missing: none.missing };
	};
	const run = (name: keyof Layout['records'], path: string) => [word(name, `${path}.start`), word(name, `${path}.len`)];
	const tag = of('host_vals', 'tag');
	return {
		loc: { words: words('locs'), start_line: word('locs', 'start.line'), start_column: word('locs', 'start.column'), end_line: word('locs', 'end.line'), end_column: word('locs', 'end.column') },
		span: { words: words('spans'), start: word('spans', 'start'), end: word('spans', 'end') },
		comment: { words: words('comments'), block: of('comments', 'block').at, text: word('comments', 'text.start'), text_end: word('comments', 'text.end'), start: word('comments', 'span.start'), end: word('comments', 'span.end'), loc: word('comments', 'loc') },
		attached: { words: words('attached'), leading: run('attached', 'leading'), trailing: run('attached', 'trailing'), inner: run('attached', 'inner') },
		error: { words: words('errors'), code: word('errors', 'code'), first: optional('errors', 'first'), second: optional('errors', 'second'), pos: word('errors', 'pos'), end: word('errors', 'end'), line: word('errors', 'line'), column: word('errors', 'column') },
		host: { words: words('hosts'), ty: optional('hosts', 'ty'), from: word('hosts', 'fields.start'), len: word('hosts', 'fields.len'), shape: word('hosts', 'shape'), span: of('hosts', 'span').at },
		value: { size: R.host_vals.size, words: words('host_vals'), tag: tag.at, tags: tag.names!, value: of('host_vals', 'value').at },
		mark: { words: words('marks'), string: word('marks', 'string'), at: word('marks', 'at'), unit: word('marks', 'unit') },
	};
}
type Records = ReturnType<typeof records>;

const recipe = (G: Language, tag: number): Op[] => (G.recipes[tag] ??= resolve(G.layout.recipes.js.find(([name]) => name === G.layout.kinds[tag].name)?.[1] ?? [], G.layout.kinds[tag].fields));

const compiled = new WeakMap<object, Compiled>();

function compile(engine: Views): Compiled {
	const known = compiled.get(engine);
	if (known !== undefined) return known;
	const layout: Layout = JSON.parse(engine.layout());
	const tag = (name: string) => layout.kinds.findIndex((kind) => kind.name === name);
	const blank = new Uint32Array((layout.extras?.size ?? 0) >> 2);
	for (const field of layout.extras?.fields ?? []) {
		if (field.ty === '?enum') blank[field.at >> 2] |= layout.none.enum << ((field.at & 3) << 3);
		else if (field.ty === '?list') blank[(field.at >> 2) + layout.none.list.tag] = layout.none.list.missing;
	}
	const R = records(layout);
	const at = (field: number) => `S.locs[id * ${R.loc.words}${field === 0 ? '' : ` + ${field}`}]`;
	const made: Compiled = {
		layout,
		words: layout.node.size >> 2,
		kind: layout.node.kind,
		extension: tag('Extension'),
		host: tag('Host'),
		identifier: tag('Identifier'),
		name: layout.kinds[tag('Identifier')].fields[0].at,
		blank,
		R,
		payload: (layout.node.kind + field(layout.kinds[tag('Host')].fields, '0').at) >> 2,
		LOC: `loc: { start: { line: ${at(R.loc.start_line)}, column: ${at(R.loc.start_column)} }, end: { line: ${at(R.loc.end_line)}, column: ${at(R.loc.end_column)} } }`,
		rows: undefined,
		js: language(layout, layout.views.js, false),
		ts: undefined,
	};
	compiled.set(engine, made);
	return made;
}

function tables(C: Compiled): NonNullable<Compiled['rows']> {
	const { layout } = C;
	const rows = (name: Table): Rows => {
		const ops = resolve(layout.recipes.rows.find(([table]) => table === name)![1], layout.rows[name].fields);
		return { plain: row(C, name, ops, undefined), linked: row(C, name, ops, LINKED[name]), words: layout.rows[name].size >> 2 };
	};
	return (C.rows = { scopes: rows('scopes'), bindings: rows('bindings'), references: rows('references'), roots: rows('roots') });
}



interface State {
	source: string;
	scopes: Decoded[];
	bindings: Decoded[];
	references: Decoded[];
	roots: Decoded[];
	link: boolean;
	erase: boolean;
	/** Lines and columns are on: every JavaScript node and comment has a `loc`. */
	lines: boolean;
	/** The host's own nodes are located too, not only the JavaScript ones. */
	host_lines: boolean;
	C: Compiled;
	G: Language;
	/** The builder of each kind, by a node's tag. */
	J: Builder[];
	/** The builders of the hosts' nodes by their shape's number, and the names by number. */
	H: (HostBuilder | undefined)[];
	names: string[];
	N: Uint32Array;
	L: Uint32Array;
	numbers: Float64Array;
	strings: string[];
	// a node's span: `P[id * ps + po]` and the word after, the nodes themselves when the source is ASCII
	P: Uint32Array;
	ps: number;
	po: number;
	locs: Uint32Array;
	/** A bit per node: those with what the literals have no room for, and those with facts linked once they are whole. */
	rare: Uint32Array;
	late: Uint32Array;
	parenthesized: Uint32Array | null;
	erased: Uint32Array | null;
	comments: Uint32Array;
	comment_count: number;
	attached_slots: Uint32Array | null;
	attached: Uint32Array;
	hosts: Uint32Array;
	host_keys: Uint32Array;
	host_vals: Uint32Array;
	host_strings: Uint32Array;
	of_node: Uint32Array | null;
	of_identifier: Uint32Array;
	root_of: Uint32Array;
	declared_by: Uint32Array;
	declared_by_at: Uint32Array;
	writes_of: Uint32Array;
	writes_of_at: Uint32Array;
	TS: Uint32Array | null;
	extras_slots: Uint32Array | null;
	extras: Uint32Array;
	/** The node being built names something bound by another node: no facts on it. */
	name_only: boolean;
	/** Nodes erasure skipped whose facts the next node built takes over. */
	adopted: number[];
	/** What erasure left in place: a name, then its node. */
	kept: (string | number)[];
	/** The nodes the node being begun stands in for, when it has facts: `late` links them and it once its keys are in. */
	pending: number[] | null;
}

// a leading U+FEFF is text, not a mark
const utf8 = new TextDecoder('utf-8', { ignoreBOM: true });
// Node reads a string out of bytes without a view of just them: half the time of a TextDecoder on a short text
const node_slices = typeof Buffer === 'undefined' ? undefined : (Buffer.prototype as unknown as Record<'latin1Slice' | 'utf8Slice', (this: Uint8Array, start: number, end: number) => string>);
const NO_WORDS = new Uint32Array(0);
const NONE_ADOPTED: number[] = [];
const NO_ROWS: Decoded[] = [];
const NO_OPS: Op[] = [];
const NO_STRINGS: string[] = [];

// the tree's interned strings, by their number
function strings(C: Compiled, tree: Tree, words: Uint32Array, lens: number, at: At, only?: number[]): string[] {
	const count = words[lens + at.starts] - 1;
	const bytes = tree[at.text] as Uint8Array, length = words[lens + at.text];
	const out = new Array<string>(count);
	if (only !== undefined) {
		// a failed parse reads its error's args alone, not a table it never walks
		const starts = tree[at.starts] as Uint32Array;
		for (const id of only) out[id] = utf8.decode(bytes.subarray(starts[id], starts[id + 1]));
	} else {
		const ascii = words[lens + at.units] === 0;
		const text = node_slices === undefined ? utf8.decode(bytes.subarray(0, length)) : (ascii ? node_slices.latin1Slice : node_slices.utf8Slice).call(bytes, 0, length);
		const ends = ascii ? (tree[at.starts] as Uint32Array) : (tree[at.units] as Uint32Array);
		for (let i = 0; i < count; i++) out[i] = text.slice(ends[i], ends[i + 1]);
	}
	// a lone surrogate stands in the text as U+FFFD
	const marked = words[lens + at.marks];
	if (marked !== 0) {
		const marks = tree[at.marks] as Uint32Array;
		const M = C.R.mark;
		// rebuilding the string once per mark was quadratic in a long literal
		for (let i = 0; i < marked; ) {
			const id = marks[i + M.string], s = out[id];
			if (s === undefined) {
				for (; i < marked && marks[i + M.string] === id; i += M.words);
				continue;
			}
			let built = '', from = 0;
			for (; i < marked && marks[i + M.string] === id; i += M.words) {
				built += s.slice(from, marks[i + M.at]) + String.fromCharCode(marks[i + M.unit]);
				from = marks[i + M.at] + 1;
			}
			out[id] = built + s.slice(from);
		}
	}
	return out;
}

const bit = (set: Uint32Array, id: number) => ((set[id >>> 5] >>> (id & 31)) & 1) === 1;
const byte = (view: Uint32Array, at: number) => (view[at >> 2] >>> ((at & 3) << 3)) & 255;

function loc(L: Records['loc'], words: Uint32Array, at: number) {
	return { start: { line: words[at + L.start_line], column: words[at + L.start_column] }, end: { line: words[at + L.end_line], column: words[at + L.end_column] } };
}

function comment(S: State, index: number, parent: Decoded | undefined): Decoded {
	const c = S.comments, K = S.C.R.comment;
	const at = index * K.words;
	const type = byte(c, at * 4 + K.block) === 1 ? 'Block' : 'Line';
	const n: Decoded = { type, value: S.source.slice(c[at + K.text], c[at + K.text_end]), start: c[at + K.start], end: c[at + K.end] };
	if (S.link) n[PARENT] = parent;
	if (S.lines) n.loc = loc(S.C.R.loc, c, at + K.loc);
	return n;
}

function run_of_comments(S: State, n: Decoded, key: string, start: number, len: number) {
	if (len === 0) return;
	const list = new Array<Decoded>(len);
	for (let i = 0; i < len; i++) list[i] = comment(S, start + i, n);
	n[key] = list;
}

function facts(S: State, n: Decoded, id: number) {
	const scope = S.of_node![id];
	if (scope !== 0) {
		if (S.link) {
			const s = S.scopes[scope - 1];
			n[SCOPE] = s;
			s.node = n;
		} else n.scope = scope - 1;
	}
	const role = S.of_identifier[id];
	if (role !== 0) {
		const index = (role - 1) >>> 1;
		if (!S.link) n[((role - 1) & 1) === 0 ? 'declares' : 'reference'] = index;
		else if (((role - 1) & 1) === 0) {
			const d = S.bindings[index];
			n[REFERENCE] = d;
			if (d.node === null) d.node = n;
		} else {
			const r = S.references[index];
			n[REFERENCE] = r;
			r.node = n;
		}
	}
	const adopted = S.adopted;
	if (S.link) {
		// the rest waits until the node is whole: `late`, for each node it stands in for and for itself
		S.pending = adopted.length === 0 ? NONE_ADOPTED : adopted.splice(0);
		return;
	}
	for (let i = 0; i <= adopted.length; i++) {
		const node = i < adopted.length ? adopted[i] : id;
		if (S.root_of[node] !== 0) n.root = S.root_of[node] - 1;
		const declared = S.declared_by_at[node];
		if (declared !== 0) n.defines = Array.from(S.declared_by.subarray(declared, declared + S.declared_by[declared - 1]));
		const written = S.writes_of_at[node];
		if (written !== 0) n.writes = Array.from(S.writes_of.subarray(written, written + S.writes_of[written - 1]));
	}
	adopted.length = 0;
}

function begin(S: State, type: string, id: number, parent: Decoded | undefined, lines: boolean): Decoded {
	const at = id * S.ps + S.po;
	const n: Decoded = { type, start: S.P[at], end: S.P[at + 1] };
	if (S.link) {
		n[PARENT] = parent;
		n[SCOPE] = undefined;
		if (type === 'Identifier') n[REFERENCE] = undefined;
	}
	if (lines) n.loc = loc(S.C.R.loc, S.locs, id * S.C.R.loc.words);
	if (S.of_node !== null && !S.name_only) facts(S, n, id);
	// the extras' children begin inside this one: what it declares waits past them
	const pending = S.pending;
	S.pending = null;
	if (S.parenthesized !== null && bit(S.parenthesized, id)) n.parenthesized = true;
	if (S.TS !== null) {
		const slot = S.extras_slots === null ? 0 : S.extras_slots[id];
		const base = (slot - 1) * S.C.layout.extras!.size;
		if (S.erase) {
			if (slot > 0) apply(S, id, n, S.G.erased, S.extras, base);
		} else {
			const tag = S.N[id * S.C.words + (S.C.kind >> 2)];
			const adds = tag === S.C.extension ? S.G.ts_adds[S.TS[ts_record(S.C, S.N, id)]] : S.G.adds[tag];
			if (adds.length !== 0) apply(S, id, n, adds, slot > 0 ? S.extras : S.C.blank, slot > 0 ? base : 0);
		}
	}
	if (S.attached_slots !== null) {
		const slot = S.attached_slots[id];
		if (slot) {
			const a = S.attached, A = S.C.R.attached;
			const at = (slot - 1) * A.words;
			run_of_comments(S, n, 'leadingComments', a[at + A.leading[0]], a[at + A.leading[1]]);
			run_of_comments(S, n, 'trailingComments', a[at + A.trailing[0]], a[at + A.trailing[1]]);
			run_of_comments(S, n, 'innerComments', a[at + A.inner[0]], a[at + A.inner[1]]);
		}
	}
	S.pending = pending;
	return n;
}

// ── What an operation reads and writes, spelled once. A `Spelling` is what spelling it takes,
// `E` a value and `St` a statement: the generator spells source, the interpreter closures.
interface Spelling<E, St> {
	/** The record's word, and byte, at a byte offset of it. */
	word(at: number): E;
	byte(at: number): E;
	lit(value: string | number | boolean | null | undefined): E;
	empty(): E;
	dec(value: E): E;
	eq(a: E, b: E): E;
	ne(a: E, b: E): E;
	/** `a ?? b` */
	or(a: E, b: E): E;
	pick(test: E, yes: E, no: E): E;
	strings(index: E): E;
	numbers(index: E): E;
	/** A constant table, and an element of one. */
	table(list: (string | true)[]): E;
	at(table: E, index: E): E;
	/** The node with an id, built into the node at hand. */
	built(id: E): E;
	items(start: E, len: E, params: boolean): E;
	pair(a: E, b: E): E;
	object(fields: [string, E][]): E;
	/** The node's own text, `back` characters short of its end; and the source between two offsets. */
	slice(back: number): E;
	text(start: E, end: E): E;
	strs(start: E, len: E): E;
	comments(): E;
	finite(value: E): E;
	bigint(text: E): E;
	bigint_value(text: E): E;
	regexp(pattern: E, flags: E): E;
	/** A value computed once and read where it is used. */
	hold(value: E): E;
	/** `hold`, the node built while `name_only` says whether it names what another declares. */
	othername(same: E, id: E): E;
	set(key: string, value: E): St;
	when(test: E, body: () => St[]): St;
	/** Under erasure: the key is what erasure keeps of the node. */
	kept(key: string): St;
}

// `raw` and `bigint` are keys of their own name
const key_of = (op: Op) => (op.op === 'raw' || op.op === 'bigint' ? op.op : op.key);

// an optional's tag word sits among its value's words, which follow their order around it
const tagged = ({ tag }: Tagged, at: number) => [at + tag * 4, at + (tag === 0 ? 4 : 0)];

const child = <E, St>(B: Spelling<E, St>, word: E, typescript = false): E => {
	const w = B.hold(word);
	return B.pick(B.eq(w, B.lit(0)), B.lit(typescript ? undefined : null), B.built(B.dec(w)));
};

/** Whether an operation's value holds nodes, which are built into the node once it exists. */
const holds_nodes = (op: Op) =>
	op.op === 'node' ||
	op.op === 'list' ||
	op.op === 'params' ||
	op.op === 'othername' ||
	(op.op === 'opt' && op.ty !== '?str' && op.ty !== '?u32') ||
	(op.op === 'maybe' && op.ty !== '?enum') ||
	(op.op === 'host' && host_holds(op.value)) ||
	(op.op === 'object' && op.inner.some(holds_nodes));

/** An operation every node of the kind has: its value. */
function value<E, St>(B: Spelling<E, St>, op: Op, none: Missing, erase: boolean): E {
	switch (op.op) {
		case 'node':
			return child(B, B.word(op.at));
		case 'opt': {
			if (op.ty === '?str' || op.ty === '?u32') {
				const which = op.ty === '?str' ? none.str : none.int;
				const [tag, from] = tagged(which, op.at);
				return B.pick(B.eq(B.word(tag), B.lit(which.missing)), B.lit(null), op.ty === '?str' ? B.strings(B.word(from)) : B.word(from));
			}
			return child(B, B.word(op.at));
		}
		case 'maybe': {
			if (op.ty === '?enum') return B.at(B.table(op.names), B.byte(op.at));
			if (op.ty === '?node') return child(B, B.word(op.at), true);
			const [tag, from] = tagged(none.list, op.at);
			return B.pick(B.eq(B.word(tag), B.lit(none.list.missing)), B.empty(), B.items(B.word(from), B.word(from + 4), false));
		}
		case 'modifier':
			return B.at(B.table(op.names.map((name) => (name === 'true' ? true : name))), B.byte(op.at));
		case 'int':
			return B.word(op.at);
		case 'pair':
			return B.pair(B.word(op.at), B.word(op.at + 4));
		case 'list':
		case 'params':
			return B.items(B.word(op.at), B.word(op.at + 4), op.op === 'params' && erase);
		case 'bool':
			return B.eq(B.byte(op.at), B.lit(1));
		case 'str':
			return B.strings(B.word(op.at));
		case 'enum':
			return B.at(B.table(op.names), B.byte(op.at));
		case 'enumor':
			return B.or(B.at(B.table(op.names), B.byte(op.at)), B.lit(op.other));
		case 'boolnames':
			return B.pick(B.eq(B.byte(op.at), B.lit(1)), B.lit(op.value), B.lit(op.other));
		case 'float':
			return B.finite(B.numbers(B.word(op.at)));
		case 'raw':
			return B.slice(0);
		case 'bigint':
			return B.bigint(B.slice(1));
		case 'bigintvalue':
			return B.bigint_value(B.slice(1));
		case 'regexp':
			return B.regexp(B.strings(B.word(op.at)), B.strings(B.word(op.at2)));
		case 'optstrkey': {
			const [tag, from] = tagged(none.str, op.at);
			return B.pick(B.eq(B.word(tag), B.lit(none.str.missing)), B.lit(undefined), B.strings(B.word(from)));
		}
		case 'constbool':
			return B.lit(op.value);
		case 'null':
			return B.lit(null);
		case 'emptylist':
			return B.empty();
		case 'object':
			return B.object(op.inner.map((inner) => [inner.key, value(B, inner, none, erase)]));
		case 'host':
			return host_value(B, op.value, op.at);
		case 'othername': {
			const w = B.hold(B.word(op.at));
			return B.othername(B.eq(w, B.word(op.at2)), B.dec(w));
		}
	}
	throw new Error(`no value for ${op.op}`);
}

/** An operation only some nodes of the kind have: what sets its key when they do. */
function conditional<E, St>(B: Spelling<E, St>, op: Op, none: Missing, erase: boolean): St | undefined {
	const key = op.key;
	switch (op.op) {
		case 'optlistkey': {
			const [tag, from] = tagged(none.list, op.at);
			return B.when(B.ne(B.word(tag), B.lit(none.list.missing)), () => [B.set(key, B.items(B.word(from), B.word(from + 4), false))]);
		}
		case 'optstrkey': {
			const [tag, from] = tagged(none.str, op.at);
			return B.when(B.ne(B.word(tag), B.lit(none.str.missing)), () => [B.set(key, B.strings(B.word(from)))]);
		}
		case 'keepif':
			return erase ? B.when(B.eq(B.byte(op.at), B.lit(1)), () => [B.kept(key)]) : undefined;
	}
	return undefined;
}

// the engine tags a host node's field by the kind of value, `HostTag` names each
const host_holds = (tag: string) => tag === 'node' || tag === 'nodes' || tag === 'comments';

/** A host node's field from the two words of its value at `at`. */
function host_value<E, St>(B: Spelling<E, St>, tag: string, at: number): E {
	const a = B.word(at), b = B.word(at + 4);
	switch (tag) {
		case 'node':
			return B.built(a);
		case 'nodes':
			return B.items(a, b, false);
		case 'comments':
			return B.comments();
		case 'str':
			return B.strings(a);
		case 'slice':
			return B.text(a, b);
		case 'strs':
			return B.strs(a, b);
		case 'bool':
			return B.eq(a, B.lit(1));
		case 'int':
			return a;
		case 'null':
			return B.lit(null);
	}
	throw new Error(`no host value ${tag}`);
}

// ── The closure spelling: what the interpreter runs where code generation is forbidden. A held
// value lives in `L`, the locals of one node's build.
type Value = (S: State, V: Uint32Array, b: number, n: Decoded, L: unknown[]) => any;
type Stmt = (S: State, V: Uint32Array, b: number, n: Decoded, L: unknown[], id: number) => void;

class Closures implements Spelling<Value, Stmt> {
	slots = 0;
	lead: Stmt[] = [];
	word = (at: number): Value => (S, V, b) => V[(b + at) >> 2];
	byte = (at: number): Value => (S, V, b) => byte(V, b + at);
	lit = (value: string | number | boolean | null | undefined): Value => () => value;
	empty = (): Value => () => [];
	dec = (value: Value): Value => (S, V, b, n, L) => value(S, V, b, n, L) - 1;
	eq = (a: Value, c: Value): Value => (S, V, b, n, L) => a(S, V, b, n, L) === c(S, V, b, n, L);
	ne = (a: Value, c: Value): Value => (S, V, b, n, L) => a(S, V, b, n, L) !== c(S, V, b, n, L);
	or = (a: Value, c: Value): Value => (S, V, b, n, L) => a(S, V, b, n, L) ?? c(S, V, b, n, L);
	pick = (test: Value, yes: Value, no: Value): Value => (S, V, b, n, L) => (test(S, V, b, n, L) ? yes(S, V, b, n, L) : no(S, V, b, n, L));
	strings = (index: Value): Value => (S, V, b, n, L) => S.strings[index(S, V, b, n, L)];
	numbers = (index: Value): Value => (S, V, b, n, L) => S.numbers[index(S, V, b, n, L)];
	table = (list: (string | true)[]): Value => () => list;
	at = (table: Value, index: Value): Value => (S, V, b, n, L) => table(S, V, b, n, L)[index(S, V, b, n, L)];
	built = (id: Value): Value => (S, V, b, n, L) => build(S, id(S, V, b, n, L), n);
	items = (start: Value, len: Value, params_: boolean): Value => (S, V, b, n, L) => (params_ ? params : items)(S, start(S, V, b, n, L), len(S, V, b, n, L), n);
	pair = (a: Value, c: Value): Value => (S, V, b, n, L) => [a(S, V, b, n, L), c(S, V, b, n, L)];
	object = (fields: [string, Value][]): Value => (S, V, b, n, L) => {
		const o: Decoded = {};
		for (const [key, field] of fields) o[key] = field(S, V, b, n, L);
		return o;
	};
	slice = (back: number): Value => (S, V, b, n) => S.source.slice(n.start, n.end - back);
	text = (start: Value, end: Value): Value => (S, V, b, n, L) => S.source.slice(start(S, V, b, n, L), end(S, V, b, n, L));
	strs = (start: Value, len: Value): Value => (S, V, b, n, L) => strs(S, start(S, V, b, n, L), len(S, V, b, n, L));
	comments = (): Value => (S, V, b, n) => comments(S, n);
	finite = (value: Value): Value => (S, V, b, n, L) => finite(value(S, V, b, n, L));
	bigint = (text: Value): Value => (S, V, b, n, L) => bigint(text(S, V, b, n, L));
	bigint_value = (text: Value): Value => (S, V, b, n, L) => bigint_value(text(S, V, b, n, L));
	regexp = (pattern: Value, flags: Value): Value => (S, V, b, n, L) => regexp(pattern(S, V, b, n, L), flags(S, V, b, n, L));
	hold(value: Value): Value {
		const slot = this.slots++;
		this.lead.push((S, V, b, n, L) => {
			L[slot] = value(S, V, b, n, L);
		});
		return (S, V, b, n, L) => L[slot];
	}
	othername = (same: Value, id: Value): Value => (S, V, b, n, L) => {
		const was = S.name_only;
		S.name_only = same(S, V, b, n, L);
		const node = build(S, id(S, V, b, n, L), n);
		S.name_only = was;
		return node;
	};
	set = (key: string, value: Value): Stmt => (S, V, b, n, L) => {
		n[key] = value(S, V, b, n, L);
	};
	when(test: Value, body: () => Stmt[]): Stmt {
		const outer = this.lead;
		const lead: Stmt[] = (this.lead = []);
		const inner = body();
		this.lead = outer;
		return (S, V, b, n, L, id) => {
			if (!test(S, V, b, n, L)) return;
			for (let i = 0; i < lead.length; i++) lead[i](S, V, b, n, L, id);
			for (let i = 0; i < inner.length; i++) inner[i](S, V, b, n, L, id);
		};
	}
	kept = (key: string): Stmt => (S, V, b, n, L, id) => {
		if (S.erase) S.kept.push(key, id);
	};
}

// a kind's operations as closures: the leads, then the keys in order
// one program for JavaScript and one for TypeScript, whose answer gives every statement a `directive`
const programs = new WeakMap<Op[], { slots: number; lead: Stmt[]; body: Stmt[] }[]>();
function program(ops: Op[], none: Missing, typescript: boolean) {
	let made = programs.get(ops);
	if (made === undefined) programs.set(ops, (made = []));
	if (made[+typescript] !== undefined) return made[+typescript];
	const B = new Closures();
	const body: Stmt[] = [];
	for (const op of ops) {
		if (op.op === 'type' || op.op === 'typeof' || op.op === 'keep' || op.op === 'through') continue;
		if (sometimes(op, typescript)) {
			const set = conditional(B, op, none, true);
			if (set !== undefined) body.push(set);
		} else body.push(B.set(key_of(op), value(B, op, none, true)));
	}
	return (made[+typescript] = { slots: B.slots, lead: B.lead, body });
}

const NO_LOCALS: unknown[] = [];

function apply(S: State, id: number, n: Decoded, ops: Op[], view: Uint32Array, base: number) {
	const { slots, lead, body } = program(ops, S.C.layout.none, S.G === S.C.ts && !S.erase);
	const L = slots === 0 ? NO_LOCALS : new Array<unknown>(slots);
	for (let i = 0; i < lead.length; i++) lead[i](S, view, base, n, L, id);
	for (let i = 0; i < body.length; i++) body[i](S, view, base, n, L, id);
}

function settle(S: State, n: Decoded, id: number, pending: number[] | null) {
	if (pending === null) return;
	for (let i = 0; i < pending.length; i++) late(S, n, pending[i]);
	late(S, n, id);
}

function run(S: State, id: number, ops: Op[], view: Uint32Array, base: number, parent: Decoded | undefined): Decoded {
	let at = 0;
	for (; ; at++) {
		const op = ops[at];
		if (op.op === 'keep') {
			if (S.erase) S.kept.push(op.key, id);
		} else if (op.op === 'through') {
			if (S.erase) {
				S.adopted.push(id);
				return build(S, view[(base + op.at) >> 2] - 1, parent);
			}
		} else break;
	}
	const head = ops[at];
	const n = begin(S, head.op === 'type' ? head.key : head.names[byte(view, base + head.at)], id, parent, S.lines);
	const pending = S.pending;
	S.pending = null;
	apply(S, id, n, ops, view, base);
	settle(S, n, id, pending);
	return n;
}

const host_tag = (S: State, field: number) => S.C.R.value.tags[byte(S.host_vals, field * S.C.R.value.size + S.C.R.value.tag)];
const host_type = (S: State, index: number) => {
	const { ty, words } = S.C.R.host, at = index * words;
	return S.hosts[at + ty.tag] === ty.missing ? null : S.names[S.hosts[at + ty.value]];
};
const host_span = (S: State, index: number) => byte(S.hosts, index * S.C.R.host.words * 4 + S.C.R.host.span) === 1;

// a host node's fields as operations, one list per shape the engine numbers
function host_ops(S: State, index: number): Op[] {
	const H = S.C.R.host, V = S.C.R.value;
	const shape = S.hosts[index * H.words + H.shape];
	let ops = S.G.host_ops.get(shape);
	if (ops === undefined) {
		const from = S.hosts[index * H.words + H.from], len = S.hosts[index * H.words + H.len];
		ops = [];
		for (let i = 0; i < len; i++) ops.push({ op: 'host', key: S.names[S.host_keys[from + i]], at: i * V.size + V.value, ty: '', names: [], at2: 0, value: host_tag(S, from + i), other: '', inner: [] });
		S.G.host_ops.set(shape, ops);
	}
	return ops;
}

function host(S: State, id: number, index: number, parent: Decoded | undefined): Decoded {
	const ty = host_type(S, index), from = S.hosts[index * S.C.R.host.words + S.C.R.host.from];
	let n: Decoded;
	if (ty === null) {
		// an object of the host's without a type, positions and all
		const at = id * S.ps + S.po;
		n = { start: S.P[at], end: S.P[at + 1] };
		if (S.host_lines) n.loc = loc(S.C.R.loc, S.locs, id * S.C.R.loc.words);
	} else if (host_span(S, index)) n = begin(S, ty, id, parent, S.host_lines);
	else {
		n = { type: ty };
		if (S.link) {
			n[PARENT] = parent;
			n[SCOPE] = undefined;
		}
		if (S.of_node !== null && !S.name_only) facts(S, n, id);
	}
	const pending = S.pending;
	S.pending = null;
	apply(S, id, n, host_ops(S, index), S.host_vals, from * S.C.R.value.size);
	settle(S, n, id, pending);
	return n;
}

const ts_record = (C: Compiled, N: Uint32Array, id: number) => N[id * C.words + C.payload] * (C.layout.ts!.size >> 2);

function build(S: State, id: number, parent: Decoded | undefined): Decoded {
	return S.J[S.N[id * S.C.words + (S.C.kind >> 2)]](S, id, 0, parent);
}

function items(S: State, start: number, len: number, parent: Decoded | undefined): (Decoded | null)[] {
	const out: (Decoded | null)[] = new Array(len);
	const L = S.L, N = S.N, J = S.J, erased = S.erased, words = S.C.words, kind = S.C.kind >> 2;
	let n = 0;
	for (let i = start, end = start + len; i < end; i++) {
		const word = L[i];
		if (word === 0) out[n++] = null;
		else if (erased === null || !bit(erased, word - 1)) out[n++] = J[N[(word - 1) * words + kind]](S, word - 1, 0, parent);
	}
	if (n !== len) out.length = n;
	return out;
}

// erasing drops TypeScript's `this` parameter
function params(S: State, start: number, len: number, parent: Decoded | undefined) {
	if (S.erase && len > 0 && S.L[start] !== 0) {
		const first = (S.L[start] - 1) * S.C.words * 4 + S.C.kind;
		if (S.N[first >> 2] === S.C.identifier && S.strings[S.N[(first + S.C.name) >> 2]] === 'this') return items(S, start + 1, len - 1, parent);
	}
	return items(S, start, len, parent);
}

const finite = (value: number) => (Number.isFinite(value) ? value : null);
const bigint_value = (digits: string) => BigInt(digits.replaceAll('_', ''));
const bigint = (digits: string) => bigint_value(digits).toString();

// an older runtime rejects syntax teasel reads, such as the v flag, and ESTree says null then
function regexp(pattern: string, flags: string): RegExp | null {
	try {
		return new RegExp(pattern, flags);
	} catch {
		return null;
	}
}

// the facts a node other than an identifier has past its scope, once it is whole
function late(S: State, n: Decoded, id: number) {
	const root = S.root_of[id];
	if (root !== 0) S.roots[root - 1].node = n;
	const declared = S.declared_by_at[id];
	if (declared !== 0) {
		for (let i = declared; i < declared + S.declared_by[declared - 1]; i++) {
			const d = S.bindings[S.declared_by[i]];
			d.declaration = n;
			if (n.init !== undefined) d.writeExpr = n.init;
		}
	}
	const written = S.writes_of_at[id];
	if (written !== 0) for (let i = written; i < written + S.writes_of[written - 1]; i++) S.references[S.writes_of[i]].writeExpr = n;
}

const LINK = 1, LINES = 2, FACTS = 4, ERASE = 8, HOST_LINES = 16;
const CONDITIONAL = new Set(['optlistkey', 'optstrkey', 'keepif']);
/** Whether an operation sets its key on only some nodes of the kind. */
const sometimes = (op: Op, typescript: boolean) => CONDITIONAL.has(op.op) && !(typescript && op.op === 'optstrkey');

// ── The source spelling: what a generated builder is made of. `word` and `byte` spell a read of
// the record at hand: a node's, a TypeScript record's, a table row's, a host node's fields.
class Source implements Spelling<string, string> {
	constants: (string | true)[][] = [];
	lead: string[] = [];
	count = 0;
	first_text: { start: string; end: string; held: string } | undefined;
	word: (at: number) => string;
	byte: (at: number) => string;
	readonly facts: boolean;
	readonly words: number;
	readonly kind: number;
	constructor(word: (at: number) => string, byte: (at: number) => string, facts: boolean, words: number, kind: number) {
		this.word = word;
		this.byte = byte;
		this.facts = facts;
		this.words = words;
		this.kind = kind;
	}
	lit = (value: string | number | boolean | null | undefined) => (value === undefined ? 'undefined' : JSON.stringify(value));
	empty = () => '[]';
	dec = (value: string) => `(${value} - 1)`;
	eq = (a: string, b: string) => `${a} === ${b}`;
	ne = (a: string, b: string) => `${a} !== ${b}`;
	or = (a: string, b: string) => `(${a} ?? ${b})`;
	pick = (test: string, yes: string, no: string) => `(${test} ? ${yes} : ${no})`;
	strings = (index: string) => `S.strings[${index}]`;
	numbers = (index: string) => `S.numbers[${index}]`;
	table = (list: (string | true)[]) => `K[${this.constants.push(list) - 1}]`;
	at = (table: string, index: string) => `${table}[${index}]`;
	// a child's builder is looked up where the child is read: each site sees its own few kinds
	built = (id: string, into = 'n') => `J[N[${id} * ${this.words} + ${this.kind >> 2}]](S, ${id}, 0, ${into})`;
	items = (start: string, len: string, params: boolean) => `${params ? 'params' : 'items'}(S, ${start}, ${len}, n)`;
	pair = (a: string, b: string) => `[${a}, ${b}]`;
	object = (fields: [string, string][]) => `{ ${fields.map(([key, field]) => `${JSON.stringify(key)}: ${field}`).join(', ')} }`;
	slice = (back: number) => `S.source.slice(S.P[p], S.P[p + 1]${back === 0 ? '' : ` - ${back}`})`;
	// a second text over the first's span, Text's `data` beside its `raw`, is the first's string
	text = (start: string, end: string) => {
		const first = this.first_text;
		if (first !== undefined) return `(${start} === ${first.start} && ${end} === ${first.end} ? ${first.held} : S.source.slice(${start}, ${end}))`;
		this.first_text = { start, end, held: this.hold(`S.source.slice(${start}, ${end})`) };
		return this.first_text.held;
	};
	strs = (start: string, len: string) => `strs(S, ${start}, ${len})`;
	comments = () => 'comments(S, n)';
	finite = (value: string) => `finite(${value})`;
	bigint = (text: string) => `bigint(${text})`;
	bigint_value = (text: string) => `bigint_value(${text})`;
	regexp = (pattern: string, flags: string) => `regexp(${pattern}, ${flags})`;
	hold(value: string) {
		const local = `v${this.count++}`;
		this.lead.push(`const ${local} = ${value};`);
		return local;
	}
	othername(same: string, id: string) {
		const local = `v${this.count++}`;
		if (this.facts) this.lead.push(`const was${local} = S.name_only; S.name_only = ${same};`);
		this.lead.push(`const ${local} = ${this.built(id)};`);
		if (this.facts) this.lead.push(`S.name_only = was${local};`);
		return local;
	}
	set = (key: string, value: string) => `n[${JSON.stringify(key)}] = ${value};`;
	when(test: string, body: () => string[]) {
		const outer = this.lead;
		const lead: string[] = (this.lead = []);
		const inner = body();
		this.lead = outer;
		const all = [...lead, ...inner].filter((statement) => statement !== '');
		return all.length === 1 ? `if (${test}) ${all[0]}` : `if (${test}) { ${all.join(' ')} }`;
	}
	kept = (key: string) => `S.kept.push(${JSON.stringify(key)}, id);`;
}

const RARE = '(S.rare[id >>> 5] >>> (id & 31) & 1) === 1';
const LATE = 'if ((S.late[id >>> 5] >>> (id & 31) & 1) === 1) late(S, n, id);';

// One object literal per kind, its parent and facts as symbol slots stored right after it. A key
// whose value holds nodes is a slot of the literal filled once the node exists, so every child is
// born with its parent. Keys a kind may leave out are set after, in the recipe's order, and a node
// with anything the literal has no room for goes to `run`.
function generate(C: Compiled, G: Language, config: number, ops: Op[], ts: boolean, adds: Op[]): Builder {
	const slow: Builder = ts ? (S, id, record, parent) => run(S, id, ops, S.TS!, record * 4, parent) : (S, id, record, parent) => run(S, id, ops, S.N, id * C.words * 4 + C.kind, parent);
	const link = (config & LINK) !== 0, facts = (config & FACTS) !== 0, erase = (config & ERASE) !== 0;
	// facts as keys, in the writer's order: only `run` spells them
	if ((facts && !link) || ops.length === 0) return slow;
	const word = (at: number) => (ts ? `T[t + ${at >> 2}]` : `N[b + ${(C.kind + at) >> 2}]`);
	const byte = (at: number) => `(${word(at)} >>> ${(((ts ? 0 : C.kind) + at) & 3) << 3} & 255)`;
	const B = new Source(word, byte, facts, C.words, C.kind);
	const none = C.layout.none;
	let at = 0;
	for (; ops[at].op === 'keep' || ops[at].op === 'through'; at++) {
		if (!erase) continue;
		if (ops[at].op === 'keep') B.lead.push(B.kept(ops[at].key));
		else B.lead.push(`S.adopted.push(id); return ${B.built(B.dec(word(ops[at].at)), 'parent')};`);
	}
	const head = ops[at++];
	const type = head.op === 'type' ? JSON.stringify(head.key) : B.at(B.table(head.names), byte(head.at));
	const identifier = head.op === 'type' && head.key === 'Identifier';
	const props = [`type: ${type}`, 'start: S.P[p]', 'end: S.P[p + 1]'];
	if ((config & LINES) !== 0) props.push(C.LOC);
	if (ops.slice(at).some((op) => op.op === 'object' && op.inner.some((inner) => CONDITIONAL.has(inner.op)))) return slow;
	// what is set once the literal exists: its nodes, then the keys some nodes of the kind leave out
	const tail: string[] = [];
	const fill = (op: Op) => {
		const lead = B.lead;
		B.lead = tail;
		const v = value(B, op, none, erase);
		B.lead = lead;
		tail.push(B.set(key_of(op), v));
	};
	if (adds.length !== 0) {
		B.lead.push(`const xs = S.extras_slots, x = xs !== null && id < xs.length ? xs[id] : 0, X = x === 0 ? S.C.blank : S.extras, xb = x === 0 ? 0 : (x - 1) * ${C.layout.extras!.size >> 2};`);
		const own = [B.word, B.byte];
		B.word = (at) => `X[xb + ${at >> 2}]`;
		B.byte = (at) => `(X[xb + ${at >> 2}] >>> ${(at & 3) << 3} & 255)`;
		for (const op of adds) {
			if (holds_nodes(op)) {
				props.push(`${JSON.stringify(key_of(op))}: undefined`);
				fill(op);
			} else props.push(`${JSON.stringify(key_of(op))}: ${value(B, op, none, erase)}`);
		}
		[B.word, B.byte] = own;
	}
	const typescript = G === C.ts && !erase;
	// a kept node sets no key: the literal goes on past it
	for (; at < ops.length && (ops[at].op === 'keepif' || !sometimes(ops[at], typescript)); at++) {
		if (ops[at].op === 'keepif') {
			const set = conditional(B, ops[at], none, erase);
			if (set !== undefined) tail.push(set);
		} else if (holds_nodes(ops[at])) {
			props.push(`${JSON.stringify(key_of(ops[at]))}: undefined`);
			fill(ops[at]);
		} else props.push(`${JSON.stringify(key_of(ops[at]))}: ${value(B, ops[at], none, erase)}`);
	}
	for (; at < ops.length; at++) {
		const op = ops[at];
		if (sometimes(op, typescript)) {
			const set = conditional(B, op, none, erase);
			if (set !== undefined) tail.push(set);
		} else fill(op);
	}
	// what the literal has no room for, the engine marks: parentheses, comments, a TypeScript extra, a binding on what is not an identifier
	const rare = [RARE];
	const before: string[] = [];
	// after the literal: a computed key in it is a runtime call until TurboFan compiles the builder, which most builders never reach; the price is a second allocation for the slots
	const slots: string[] = [];
	const after: string[] = [];
	if (link) {
		slots.push('n[PARENT] = parent;');
		if (!facts) slots.push('n[SCOPE] = undefined;');
		else {
			rare.push('S.name_only', 'S.adopted.length !== 0');
			before.push('const sc = S.of_node[id]; const s = sc === 0 ? undefined : S.scopes[sc - 1];');
			slots.push('n[SCOPE] = s;');
			after.push('if (s !== undefined) s.node = n;');
		}
		if (identifier && !facts) slots.push('n[REFERENCE] = undefined;');
		else if (identifier) {
			before.push('const ro = S.of_identifier[id] - 1; const r = ro === -1 ? undefined : (ro & 1) === 0 ? S.bindings[ro >>> 1] : S.references[ro >>> 1];');
			slots.push('n[REFERENCE] = r;');
			after.push('if (r !== undefined && ((ro & 1) === 1 || r.node === null)) r.node = n;');
		}
		if (facts) after.push(LATE);
	}
	const lead = B.lead;
	const body = `const N = S.N, J = S.J, b = id * ${C.words}${ts ? ', T = S.TS' : ''}; ${lead.length !== 0 && lead[lead.length - 1].includes('return J[') ? lead.join(' ') : `if (${rare.join(' || ')}) return slow(S, id, t, parent); ${before.join(' ')} const p = id * S.ps + S.po; ${lead.join(' ')} const n = { ${props.join(', ')} }; ${slots.join(' ')} ${tail.join(' ')} ${after.join(' ')} return n;`}`;
	return new Function('K', 'slow', 'items', 'params', 'finite', 'bigint', 'bigint_value', 'regexp', 'late', 'PARENT', 'SCOPE', 'REFERENCE', `return (S, id, t, parent) => { ${body} };`)(B.constants, slow, items, params, finite, bigint, bigint_value, regexp, late, PARENT, SCOPE, REFERENCE);
}

function strs(S: State, start: number, len: number): string[] {
	const out = new Array<string>(len);
	for (let i = 0; i < len; i++) out[i] = S.strings[S.host_strings[start + i]];
	return out;
}

// The engine numbers the shapes of a host's nodes, a type with its fields' keys and kinds of value:
// one literal each, as the kinds have.
function host_by_shape(S: State, id: number, index: number, config: number, parent: Decoded | undefined): Decoded {
	const H = S.C.R.host;
	const from = S.hosts[index * H.words + H.from], shape = S.hosts[index * H.words + H.shape];
	let build = S.H[shape];
	if (build === undefined) {
		const len = S.hosts[index * H.words + H.len];
		const keys: string[] = [], tags: string[] = [];
		for (let i = from; i < from + len; i++) {
			keys.push(S.names[S.host_keys[i]]);
			tags.push(host_tag(S, i));
		}
		build = S.H[shape] = generate_host(S.C, config, host_type(S, index), host_span(S, index), keys, tags);
	}
	return build(S, id, index, from, parent);
}

function generate_host(C: Compiled, config: number, type: string | null, span: boolean, keys: string[], tags: string[]): HostBuilder {
	const link = (config & LINK) !== 0 && type !== null, facts = (config & FACTS) !== 0 && type !== null;
	if (facts && (config & LINK) === 0) return (S, id, index, _from, parent) => host(S, id, index, parent);
	const V = C.R.value;
	const B = new Source((at) => `HV[from * ${V.words} + ${at >> 2}]`, () => '', facts, C.words, C.kind);
	const props = type === null ? [] : [`type: ${JSON.stringify(type)}`];
	if (type === null || span) {
		props.push('start: S.P[p]', 'end: S.P[p + 1]');
		if ((config & HOST_LINES) !== 0) props.push(C.LOC);
	}
	const tail: string[] = [];
	keys.forEach((key, i) => {
		if (host_holds(tags[i])) {
			props.push(`${JSON.stringify(key)}: undefined`);
			tail.push(B.set(key, host_value(B, tags[i], i * V.size + V.value)));
		} else props.push(`${JSON.stringify(key)}: ${host_value(B, tags[i], i * V.size + V.value)}`);
	});
	// what the literal has no room for, as for the kinds; a node without a span has only its facts
	const rare = span ? [RARE] : [];
	const before: string[] = [], slots: string[] = [], after: string[] = [];
	if (link) {
		slots.push('n[PARENT] = parent;');
		if (!facts) slots.push('n[SCOPE] = undefined;');
		else {
			rare.push('S.name_only', 'S.adopted.length !== 0');
			if (!span) rare.push(RARE);
			before.push('const sc = S.of_node[id]; const s = sc === 0 ? undefined : S.scopes[sc - 1];');
			slots.push('n[SCOPE] = s;');
			after.push('if (s !== undefined) s.node = n;', LATE);
		}
	}
	const body = `const N = S.N, J = S.J, HV = S.host_vals; ${rare.length === 0 ? '' : `if (${rare.join(' || ')}) return host(S, id, index, parent);`} ${before.join(' ')} const p = id * S.ps + S.po; ${B.lead.join(' ')} const n = { ${props.join(', ')} }; ${slots.join(' ')} ${tail.join(' ')} ${after.join(' ')} return n;`;
	return new Function('K', 'host', 'items', 'comments', 'strs', 'late', 'PARENT', 'SCOPE', `return (S, id, index, from, parent) => { ${body} };`)(B.constants, host, items, comments, strs, late, PARENT, SCOPE);
}

const generated = (() => {
	try {
		new Function('');
		return true;
	} catch {
		return false;
	}
})();

// A linked row is whole from its literal, so nothing is added to it later: the rows its numbers
// index are read as the objects (a scope's parent is numbered before it), and what the tree fills in
// once built is there as null. A binding is the reference its declaring identifier makes, `binding` itself.
interface Linked {
	into: Record<string, Table>;
	held: Record<string, unknown>;
	self?: string;
}
const LINKED: Record<Table, Linked> = {
	scopes: { into: { parent: 'scopes' }, held: { node: null } },
	bindings: { into: { scope: 'scopes' }, held: { node: null, declaration: null, binding: null, declares: true, read: false, mutate: false, writeExpr: null }, self: 'binding' },
	references: { into: { scope: 'scopes', binding: 'bindings' }, held: { node: null, writeExpr: null } },
	roots: { into: { scope: 'scopes', scopes: 'scopes', bindings: 'bindings', references: 'references' }, held: { node: null } },
};

function row(C: Compiled, name: Table, ops: Op[], linked: Linked | undefined): Row {
	if (!generated) {
		return (S, view, b, T) => {
			const n: Decoded = {};
			apply(S, 0, n, ops, view, b * 4);
			if (linked === undefined) return n;
			for (const key in linked.into) {
				const v = n[key], t = linked.into[key] === name ? T : S[linked.into[key]];
				if (v !== null) n[key] = Array.isArray(v) ? t.slice(v[0], v[1]) : t[v];
			}
			Object.assign(n, linked.held);
			if (linked.self !== undefined) n[linked.self] = n;
			return n;
		};
	}
	const B = new Source((at) => `V[b + ${at >> 2}]`, (at) => `(V[b + ${at >> 2}] >>> ${(at & 3) << 3} & 255)`, false, C.words, C.kind);
	const none = C.layout.none;
	const props = ops.map((op) => {
		const key = JSON.stringify(op.key), into = linked?.into[op.key];
		if (into === undefined) return `${key}: ${value(B, op, none, false)}`;
		const t = into === name ? 'T' : `S.${into}`;
		if (op.op === 'pair') return `${key}: ${t}.slice(${B.word(op.at)}, ${B.word(op.at + 4)})`;
		if (op.op === 'int') return `${key}: ${t}[${B.word(op.at)}]`;
		const v = B.hold(value(B, op, none, false));
		return `${key}: ${v} === null ? null : ${t}[${v}]`;
	});
	if (linked !== undefined) for (const key in linked.held) props.push(`${JSON.stringify(key)}: ${JSON.stringify(linked.held[key])}`);
	const self = linked?.self === undefined ? '' : ` o.${linked.self} = o;`;
	return new Function('K', `return (S, V, b, T) => { ${B.lead.join(' ')} const o = { ${props.join(', ')} };${self} return o; };`)(B.constants);
}

// each builder is made the first time its kind is met
function builders(C: Compiled, G: Language, config: number, typescript: boolean): Builders {
	let B = G.sets.get(config);
	G.last = config;
	if (B !== undefined) return (G.builders = B);
	// one closure stands in for every kind not met yet: it makes the kind's builder and takes its place
	const js = (tag: number): Builder => {
		const ops = recipe(G, tag);
		return generated ? generate(C, G, config, ops, false, typescript && (config & ERASE) === 0 ? G.adds[tag] : NO_OPS) : (S, id, record, parent) => run(S, id, ops, S.N, id * C.words * 4 + C.kind, parent);
	};
	const ts = (tag: number): Builder => (generated ? generate(C, G, config, G.ts[tag], true, (config & ERASE) === 0 ? G.ts_adds[tag] : NO_OPS) : (S, id, record, parent) => run(S, id, G.ts[tag], S.TS!, record * 4, parent));
	const set: Builders = (B = {
		js: new Array<Builder>(C.layout.kinds.length).fill((S, id, record, parent) => (set.js[S.N[id * C.words + (C.kind >> 2)]] = js(S.N[id * C.words + (C.kind >> 2)]))(S, id, record, parent)),
		ts: new Array<Builder>(G.ts.length).fill((S, id, record, parent) => (set.ts[S.TS![record]] = ts(S.TS![record]))(S, id, record, parent)),
		hosts: [],
	});
	set.js[C.extension] = (S, id, _record, parent) => { const record = ts_record(C, S.N, id); return set.ts[S.TS![record]](S, id, record, parent); };
	set.js[C.host] = generated
		? (S, id, _record, parent) => {
				const index = S.N[id * C.words + C.payload], H = C.R.host;
				const build = S.H[S.hosts[index * H.words + H.shape]];
				return build === undefined ? host_by_shape(S, id, index, config, parent) : build(S, id, index, S.hosts[index * H.words + H.from], parent);
			}
		: (S, id, _record, parent) => host(S, id, S.N[id * C.words + C.payload], parent);
	G.sets.set(config, set);
	return (G.builders = set);
}

function comments(S: State, parent: Decoded | undefined): Decoded[] {
	const out = new Array<Decoded>(S.comment_count);
	for (let i = 0; i < S.comment_count; i++) out[i] = comment(S, i, parent);
	return out;
}

// the names the engine has numbered since the last answer. Once the tree moved, the names known
// must still head the table, or an engine that started over numbered them anew, and the shapes
// found by those numbers with them
function names(G: Language, tree: Tree, count: number, moved: boolean): string[] {
	if (!moved && count === G.names.length) return G.names;
	const text = tree[G.at.names] as Uint8Array, starts = tree[G.at.name_starts] as Uint32Array;
	const name = (i: number) => utf8.decode(text.subarray(starts[i], starts[i + 1]));
	if (moved && (count < G.names.length || G.names.some((known, i) => known !== name(i)))) {
		G.names = [];
		G.host_ops.clear();
		for (const set of G.sets.values()) set.hosts.length = 0;
	}
	for (let i = G.names.length; i < count; i++) G.names.push(name(i));
	return G.names;
}

const filled = (tree: Tree, words: Uint32Array, lens: number, at: number) => (words[lens + at] === 0 ? null : (tree[at] as Uint32Array).subarray(0, words[lens + at]));

// what `words[lens]` says of an answer, what the caller could not know: the parse failed, the tree is TypeScript's (a document's language is the grammar's to tell); each view's length follows it, then where the tree's buffers sit
const FAILED = 1, TYPESCRIPT = 2;
// the options the answer was asked with, as `flags` packed them
const slot = (key: keyof Options, index = 3) => index << (2 * SLOT[key]);
const TS_OPTION = slot('typescript'), ERASED = slot('typescript', 2), COMMENTS = slot('comments'), LOCATED = slot('locations'), JS_LOCATED = slot('locations', 2), RECOVERED = slot('errorRecovery');

function table(S: State, tree: Tree, words: Uint32Array, lens: number, rows: Rows, at: number): Decoded[] {
	const build = S.link ? rows.linked : rows.plain;
	const size = rows.words;
	const view = tree[at] as Uint32Array;
	const out = new Array<Decoded>(words[lens + at] / size);
	for (let i = 0; i < out.length; i++) out[i] = build(S, view, i * size, out);
	return out;
}

const NO_ARGS = Object.freeze([]);

function args_of(E: Records['error'], view: Uint32Array, at: number): number[] {
	const { first, second } = E;
	if (view[at + first.tag] === first.missing) return [];
	return view[at + second.tag] === second.missing ? [view[at + first.value]] : [view[at + first.value], view[at + second.value]];
}

function errors(S: State, view: Uint32Array, count: number): Decoded[] {
	const E = S.C.R.error;
	const out = new Array<Decoded>(count);
	for (let i = 0, at = 0; i < count; i++, at += E.words) {
		const ids = args_of(E, view, at);
		const args = ids.length === 0 ? NO_ARGS : ids.map((id) => S.strings[id]);
		out[i] = { code: CODES[view[at + E.code]], args, pos: view[at + E.pos], end: view[at + E.end], line: view[at + E.line], column: view[at + E.column] };
	}
	return out;
}

/**
 * The answer of the last parse, asked with the options `flags` packed, `listed` for the parameter
 * list entry: `words` are `end`, the roots by number, a word of what the answer is and each
 * view's length; the tree itself is read in place. `link` replaces the scope and binding numbers
 * with the objects they index.
 */
export function decode(words: Uint32Array, source: string, engine: Views, flags: number, listed: boolean, link = true): Decoded {
	const C = compile(engine);
	const lens = 2 + words[1];
	const what = words[lens];
	const typescript = (what & TYPESCRIPT) !== 0;
	const G = typescript ? (C.ts ??= language(C.layout, C.layout.views.ts!, true)) : C.js;
	const at = G.at;
	const lo = words[lens + at.sits], hi = words[lens + at.sits + 1];
	const moved = lo !== G.sat[0] || hi !== G.sat[1];
	G.sat[0] = lo;
	G.sat[1] = hi;
	const tree = engine.tree(typescript, moved);
	const erase = (flags & TS_OPTION) === ERASED, lines = (flags & LOCATED) !== 0, failed = (what & FAILED) !== 0;
	const host_lines = lines && (flags & LOCATED) !== JS_LOCATED;
	const spans = words[lens + at.spans] !== 0;
	const scoped = !failed && words[lens + at.scopes] !== 0;
	const rows = tree[at.errors] as Uint32Array;
	const config = (link ? LINK : 0) | (lines ? LINES : 0) | (host_lines ? HOST_LINES : 0) | (scoped ? FACTS : 0) | (erase ? ERASE : 0);
	const B = G.last === config ? G.builders! : builders(C, G, config, typescript);
	// the state stays with the tree's views, one answer at a time: only what an answer changes is set
	let S = G.state;
	if (S === undefined || moved || G.tree !== tree) {
		G.tree = tree;
		S = G.state = {
			source,
			scopes: NO_ROWS,
			bindings: NO_ROWS,
			references: NO_ROWS,
			roots: NO_ROWS,
			link,
			erase,
			lines,
			host_lines,
			C,
			G,
			J: B.js,
			H: B.hosts,
			names: G.names,
			N: tree[at.nodes] as Uint32Array,
			L: tree[at.lists] as Uint32Array,
			numbers: tree[at.numbers] as Float64Array,
			strings: NO_STRINGS,
			P: NO_WORDS,
			ps: 0,
			po: 0,
			locs: tree[at.locs] as Uint32Array,
			rare: tree[at.rare] as Uint32Array,
			late: tree[at.late] as Uint32Array,
			parenthesized: null,
			erased: null,
			comments: tree[at.comments] as Uint32Array,
			comment_count: 0,
			attached_slots: null,
			attached: tree[at.attached] as Uint32Array,
			hosts: tree[at.hosts] as Uint32Array,
			host_keys: tree[at.host_keys] as Uint32Array,
			host_vals: tree[at.host_vals] as Uint32Array,
			host_strings: tree[at.host_strings] as Uint32Array,
			of_node: null,
			of_identifier: tree[at.of_identifier] as Uint32Array,
			root_of: tree[at.root_of] as Uint32Array,
			declared_by: tree[at.declared_by] as Uint32Array,
			declared_by_at: tree[at.declared_by_at] as Uint32Array,
			writes_of: tree[at.writes_of] as Uint32Array,
			writes_of_at: tree[at.writes_of_at] as Uint32Array,
			TS: typescript ? (tree[at.ts] as Uint32Array) : null,
			extras_slots: null,
			extras: typescript ? (tree[at.extras] as Uint32Array) : NO_WORDS,
			name_only: false,
			adopted: [],
			kept: [],
			pending: null,
		};
	}
	S.source = source;
	S.link = link;
	S.erase = erase;
	S.lines = lines;
	S.host_lines = host_lines;
	S.J = B.js;
	S.H = B.hosts;
	S.names = names(G, tree, words[lens + at.name_starts] - 1, moved);
	S.strings = failed ? strings(C, tree, words, lens, at, args_of(C.R.error, rows, 0)) : strings(C, tree, words, lens, at);
	S.P = spans ? (tree[at.spans] as Uint32Array) : S.N;
	S.ps = spans ? C.R.span.words : C.words;
	S.po = spans ? C.R.span.start : C.layout.node.start >> 2;
	S.parenthesized = filled(tree, words, lens, at.parenthesized);
	S.erased = erase ? filled(tree, words, lens, at.erased) : null;
	S.comment_count = words[lens + at.comments] / C.R.comment.words;
	S.attached_slots = filled(tree, words, lens, at.attached_slots);
	S.of_node = scoped ? (tree[at.of_node] as Uint32Array) : null;
	S.extras_slots = typescript ? filled(tree, words, lens, at.extras_slots) : null;
	S.name_only = false;
	S.pending = null;
	S.adopted.length = 0;
	S.kept.length = 0;
	if (scoped) {
		const rows = C.rows ?? tables(C);
		S.scopes = table(S, tree, words, lens, rows.scopes, at.scopes);
		S.bindings = table(S, tree, words, lens, rows.bindings, at.bindings);
		S.references = table(S, tree, words, lens, rows.references, at.references);
		S.roots = table(S, tree, words, lens, rows.roots, at.roots);
	}
	let node: Decoded | Decoded[] | null = null;
	if (!failed) {
		if (listed) {
			node = [];
			for (let i = 2; i < lens; i++) if (S.erased === null || !bit(S.erased, words[i])) node.push(build(S, words[i], undefined));
		} else node = build(S, words[2], undefined);
	}
	const answer: Decoded = { node, end: words[0] };
	if (!failed && (flags & COMMENTS) !== 0) answer.comments = comments(S, undefined);
	if (failed || (flags & RECOVERED) !== 0) answer.errors = errors(S, rows, words[lens + at.errors] / C.R.error.words);
	if (!failed && erase) answer.typescript = kept(S);
	if (!failed && scoped) {
		answer.scopes = S.scopes;
		answer.bindings = S.bindings;
		answer.references = S.references;
		if (words[lens + at.hosts] !== 0) answer.roots = S.roots;
	}
	// the state outlives the answer: it lets go of what the answer holds
	S.source = '';
	S.strings = NO_STRINGS;
	S.scopes = S.bindings = S.references = S.roots = NO_ROWS;
	return answer;
}

/** What erasure left in place, in source order. */
function kept(S: State): Decoded[] {
	const out: Decoded[] = [];
	for (let i = 0; i < S.kept.length; i += 2) {
		const id = S.kept[i + 1] as number;
		const at = id * S.ps + S.po;
		const n: Decoded = { type: S.kept[i], start: S.P[at], end: S.P[at + 1] };
		if (S.lines) n.loc = loc(S.C.R.loc, S.locs, id * S.C.R.loc.words);
		out.push(n);
	}
	// a node comes before what it contains
	return out.sort((a, b) => a.start - b.start || b.end - a.end);
}
