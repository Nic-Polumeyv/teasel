// Builds ESTree objects from the parser's own tree, read in place: the layout says where each
// kind's fields sit, the recipes how the kind is spelled.

// symbol keys: ten times cheaper than a WeakMap entry, and skipped by JSON, Object.keys and for-in
export const SCOPE = Symbol('scope');
export const REFERENCE = Symbol('reference');
export const PARENT = Symbol('parent');

/** A decoded object: the tree decides its shape, `index.ts` describes it. */
export type Decoded = Record<string | symbol, any>;

type View = Uint32Array | Float64Array | Uint8Array;
/** Whether the tree is the TypeScript one, then each view of the layout's `views`, as long as its buffer's room; `undefined` for a table no parse filled yet. */
export type Tree = readonly (View | number | undefined)[];

interface Field {
	name: string;
	at: number;
	ty: string;
	names?: string[];
	fields?: Field[];
}
interface Kind {
	name: string;
	fields: Field[];
}
interface Tagged {
	tag: number;
	missing: number;
}
type RawOp = [string, ...any[]];
type RawRecipes = [string, RawOp[]][];
interface Layout {
	node: { size: number; start: number; end: number; kind: number };
	kinds: Kind[];
	ts?: { size: number; kinds: Kind[] };
	extras?: { size: number; fields: Field[] };
	/** Each table's record: its size and its fields. */
	rows: Record<string, { size: number; fields: Field[] }>;
	none: { enum: number; bool: number; list: Tagged; str: Tagged; int: Tagged };
	views: { js: string[]; ts?: string[] };
	recipes: { js: RawRecipes; rows: RawRecipes; ts?: RawRecipes; adds?: RawRecipes; extras?: RawRecipes };
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
	/** What `const`, `constbool`, `enumor` and `boolnames` spell: the value, or the names for true and false. */
	value: any;
	other: string;
	inner: Op[];
}

interface Language {
	/** Each view's place in the tree, by name. */
	at: At;
	recipes: Op[][];
	ts: Op[][];
	adds: Op[][];
	extras: Op[];
	erased: Op[];
	/** The builders of each way to build: linked, with lines, with facts, erasing; the last asked for kept near. */
	sets: Map<number, Builders>;
	last: number;
	builders: Builders | undefined;
	/** The names of the hosts' types and keys, by the number the engine gave each; they only grow. */
	names: string[];
	/** The state of the last answer read from `tree`, kept for the next. */
	state: State | undefined;
	tree: Tree | undefined;
}

type Builder = (S: State, id: number, record: number) => Decoded;
/** A table's row from its record, `b` words into the view. */
type Row = (S: State, view: Uint32Array, b: number) => Decoded;
/** A host's node from its fields, `from` on in the hosts' keys and values. */
type HostBuilder = (S: State, id: number, index: number, from: number) => Decoded;
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
	/** Each table's row builders, plain and linked, and its record's words. */
	rows: { scopes: Rows; bindings: Rows; references: Rows; roots: Rows };
	js: Language;
	ts: Language | undefined;
}

function resolve(raw: RawOp[], fields: Field[]): Op[] {
	const find = (path: string): Field => {
		const [head, rest] = path.split('.');
		const field = fields.find((f) => f.name === head)!;
		if (rest === undefined) return field;
		const inner = field.fields!.find((f) => f.name === rest)!;
		return { ...inner, at: field.at + inner.at };
	};
	return raw.map(([op, ...rest]) => {
		const out: Op = { op, key: '', at: 0, ty: '', names: [], at2: 0, value: null, other: '', inner: [] };
		let field: string | undefined;
		if (op === 'typeof' || op === 'through') [field] = rest;
		else if (op === 'object') [out.key, out.inner] = [rest[0], resolve(rest[1], fields)];
		else if (op === 'const' || op === 'constbool') [out.key, out.value] = rest;
		else if (op === 'boolnames') [out.key, field, out.value, out.other] = rest;
		else if (op === 'enumor') [out.key, field, out.other] = rest;
		else if (op === 'othername') {
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

/** Each view's place in a tree; zero for a view the tree does not have. */
interface At {
	nodes: number;
	lists: number;
	numbers: number;
	text: number;
	starts: number;
	units: number;
	spans: number;
	locs: number;
	parenthesized: number;
	erased: number;
	comments: number;
	attached_slots: number;
	attached: number;
	errors: number;
	hosts: number;
	host_keys: number;
	host_vals: number;
	host_strings: number;
	scopes: number;
	bindings: number;
	references: number;
	roots: number;
	of_node: number;
	of_identifier: number;
	root_of: number;
	declared_by: number;
	declared_by_at: number;
	writes_of: number;
	writes_of_at: number;
	ts: number;
	extras_slots: number;
	extras: number;
	names: number;
	name_starts: number;
	rare: number;
	late: number;
}

function language(layout: Layout, views: string[], typescript: boolean): Language {
	const by = (recipes: RawRecipes, kinds: Kind[]) => kinds.map((kind) => resolve(recipes.find(([name]) => name === kind.name)?.[1] ?? [], kind.fields));
	// a literal: an object filled by computed keys turns into a dictionary, a hash lookup per read
	const of = (name: string) => 1 + views.indexOf(name);
	const at: At = { nodes: of('nodes'), lists: of('lists'), numbers: of('numbers'), text: of('text'), starts: of('starts'), units: of('units'), spans: of('spans'), locs: of('locs'), parenthesized: of('parenthesized'), erased: of('erased'), comments: of('comments'), attached_slots: of('attached_slots'), attached: of('attached'), errors: of('errors'), hosts: of('hosts'), host_keys: of('host_keys'), host_vals: of('host_vals'), host_strings: of('host_strings'), scopes: of('scopes'), bindings: of('bindings'), references: of('references'), roots: of('roots'), of_node: of('of_node'), of_identifier: of('of_identifier'), root_of: of('root_of'), declared_by: of('declared_by'), declared_by_at: of('declared_by_at'), writes_of: of('writes_of'), writes_of_at: of('writes_of_at'), ts: of('ts'), extras_slots: of('extras_slots'), extras: of('extras'), names: of('names'), name_starts: of('name_starts'), rare: of('rare'), late: of('late') };
	const out: Language = { at, recipes: by(layout.recipes.js, layout.kinds), ts: [], adds: [], extras: [], erased: [], sets: new Map(), last: -1, builders: undefined, names: [], state: undefined, tree: undefined };
	if (typescript) {
		const extras = layout.extras!.fields;
		out.ts = by(layout.recipes.ts!, layout.ts!.kinds);
		out.adds = layout.kinds.map((kind) => resolve(layout.recipes.adds!.find(([name]) => name === kind.name)?.[1] ?? [], extras));
		out.extras = resolve(layout.recipes.extras!.find(([name]) => name === 'extras')![1], extras);
		out.erased = resolve(layout.recipes.extras!.find(([name]) => name === 'erased')![1], extras);
	}
	return out;
}

const compiled = new WeakMap<object, Compiled>();

function compile(engine: Views): Compiled {
	const known = compiled.get(engine);
	if (known !== undefined) return known;
	const layout: Layout = JSON.parse(engine.layout());
	const tag = (name: string) => layout.kinds.findIndex((kind) => kind.name === name);
	const blank = new Uint32Array((layout.extras?.size ?? 0) >> 2);
	for (const field of layout.extras?.fields ?? []) {
		const none = field.ty === '?enum' ? layout.none.enum : field.ty === '?bool' ? layout.none.bool : 0;
		blank[field.at >> 2] |= none << ((field.at & 3) << 3);
	}
	const made: Compiled = {
		layout,
		words: layout.node.size >> 2,
		kind: layout.node.kind,
		extension: tag('Extension'),
		host: tag('Host'),
		identifier: tag('Identifier'),
		name: layout.kinds[tag('Identifier')].fields[0].at,
		blank,
		rows: undefined as unknown as Compiled['rows'],
		js: language(layout, layout.views.js, false),
		ts: layout.views.ts === undefined ? undefined : language(layout, layout.views.ts, true),
	};
	const rows = (name: string): Rows => {
		const ops = resolve(layout.recipes.rows.find(([table]) => table === name)![1], layout.rows[name].fields);
		return { plain: row(made, ops, undefined), linked: row(made, ops, LINKED[name]), words: layout.rows[name].size >> 2 };
	};
	made.rows = { scopes: rows('scopes'), bindings: rows('bindings'), references: rows('references'), roots: rows('roots') };
	compiled.set(engine, made);
	return made;
}

interface State {
	source: string;
	scopes: Decoded[];
	bindings: Decoded[];
	references: Decoded[];
	roots: Decoded[];
	link: boolean;
	erase: boolean;
	/** Lines and columns are on: every node and comment has a `loc`. */
	lines: boolean;
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
const NO_STRINGS: string[] = [];

// the tree's interned strings, by their number
function strings(tree: Tree, words: Uint32Array, lens: number, at: At): string[] {
	const count = words[lens + at.starts] - 1;
	const bytes = tree[at.text] as Uint8Array, length = words[lens + at.text];
	const ascii = words[lens + at.units] === 0;
	const text = node_slices === undefined ? utf8.decode(bytes.subarray(0, length)) : (ascii ? node_slices.latin1Slice : node_slices.utf8Slice).call(bytes, 0, length);
	const ends = ascii ? (tree[at.starts] as Uint32Array) : (tree[at.units] as Uint32Array);
	const out = new Array<string>(count);
	for (let i = 0; i < count; i++) out[i] = text.slice(ends[i], ends[i + 1]);
	return out;
}

const bit = (set: Uint32Array, id: number) => ((set[id >>> 5] >>> (id & 31)) & 1) === 1;
const byte = (view: Uint32Array, at: number) => (view[at >> 2] >>> ((at & 3) << 3)) & 255;

function loc(words: Uint32Array, at: number) {
	return { start: { line: words[at], column: words[at + 1] }, end: { line: words[at + 2], column: words[at + 3] } };
}

function comment(S: State, index: number): Decoded {
	const c = S.comments;
	const at = index * 9;
	const type = c[at] === 1 ? 'Block' : 'Line';
	const n: Decoded = S.link ? { type, value: S.source.slice(c[at + 1], c[at + 2]), start: c[at + 3], end: c[at + 4], [PARENT]: undefined } : { type, value: S.source.slice(c[at + 1], c[at + 2]), start: c[at + 3], end: c[at + 4] };
	if (S.lines) n.loc = loc(c, at + 5);
	return n;
}

function run_of_comments(S: State, n: Decoded, key: string, start: number, len: number) {
	if (len === 0) return;
	const list = [];
	for (let i = start; i < start + len; i++) {
		const c = comment(S, i);
		if (S.link) c[PARENT] = n;
		list.push(c);
	}
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

function begin(S: State, type: string, id: number): Decoded {
	const at = id * S.ps + S.po;
	const n: Decoded = !S.link ? { type, start: S.P[at], end: S.P[at + 1] } : type === 'Identifier' ? { type, start: S.P[at], end: S.P[at + 1], [PARENT]: undefined, [SCOPE]: undefined, [REFERENCE]: undefined } : { type, start: S.P[at], end: S.P[at + 1], [PARENT]: undefined, [SCOPE]: undefined };
	if (S.lines) n.loc = loc(S.locs, id * 4);
	if (S.of_node !== null && !S.name_only) facts(S, n, id);
	// the extras' children begin inside this one: what it declares waits past them
	const pending = S.pending;
	S.pending = null;
	if (S.parenthesized !== null && bit(S.parenthesized, id)) n.parenthesized = true;
	if (S.TS !== null) {
		// a node without extras has only what TypeScript adds to its kind whatever they say
		const slot = S.extras_slots === null ? 0 : S.extras_slots[id];
		if (slot > 0) {
			const base = (slot - 1) * S.C.layout.extras!.size;
			if (S.erase) apply(S, id, n, S.G.erased, S.extras, base);
			else {
				apply(S, id, n, S.G.adds[S.N[id * S.C.words + (S.C.kind >> 2)]], S.extras, base);
				apply(S, id, n, S.G.extras, S.extras, base);
			}
		} else if (!S.erase) {
			const adds = S.G.adds[S.N[id * S.C.words + (S.C.kind >> 2)]];
			if (adds.length !== 0) apply(S, id, n, adds, S.C.blank, 0);
		}
	}
	if (S.attached_slots !== null) {
		const slot = S.attached_slots[id];
		if (slot) {
			const a = S.attached;
			const at = (slot - 1) * 6;
			run_of_comments(S, n, 'leadingComments', a[at], a[at + 1]);
			run_of_comments(S, n, 'trailingComments', a[at + 2], a[at + 3]);
			run_of_comments(S, n, 'innerComments', a[at + 4], a[at + 5]);
		}
	}
	S.pending = pending;
	return n;
}

function child(S: State, n: Decoded, key: string, word: number) {
	const c = word === 0 ? null : build(S, word - 1);
	n[key] = c;
	// a child with a type is a node; a literal's regex or a template element's value is not
	if (S.link && c !== null && n.type !== undefined && c.type !== undefined) c[PARENT] = n;
}

function list(S: State, n: Decoded, key: string, items: (Decoded | null)[]) {
	if (S.link && n.type !== undefined) link_items(items, n);
	n[key] = items;
}

// an optional's tag word sits among its value's words, which follow their order around it
function tagged(view: Uint32Array, at: number, { tag, missing }: Tagged): number {
	return view[(at >> 2) + tag] === missing ? -1 : (at >> 2) + (tag === 0 ? 1 : 0);
}

function apply(S: State, id: number, n: Decoded, ops: Op[], view: Uint32Array, base: number) {
	const none = S.C.layout.none;
	for (let i = 0; i < ops.length; i++) {
		const op = ops[i];
		const at = base + op.at;
		switch (op.op) {
			case 'node':
				child(S, n, op.key, view[at >> 2]);
				break;
			case 'opt':
				if (op.ty === '?str' || op.ty === '?u32') {
					const value = tagged(view, at, op.ty === '?str' ? none.str : none.int);
					n[op.key] = value === -1 ? null : op.ty === '?str' ? S.strings[view[value]] : view[value];
				} else child(S, n, op.key, view[at >> 2]);
				break;
			case 'int':
				n[op.key] = view[at >> 2];
				break;
			case 'pair':
				n[op.key] = [view[at >> 2], view[(at >> 2) + 1]];
				break;
			case 'optkey':
				if (view[at >> 2] !== 0) child(S, n, op.key, view[at >> 2]);
				break;
			case 'list':
				list(S, n, op.key, items(S, view[at >> 2], view[(at >> 2) + 1]));
				break;
			case 'optlistkey': {
				const value = tagged(view, at, none.list);
				if (value !== -1) list(S, n, op.key, items(S, view[value], view[value + 1]));
				break;
			}
			case 'params':
				list(S, n, op.key, params(S, view[at >> 2], view[(at >> 2) + 1]));
				break;
			case 'bool':
				n[op.key] = byte(view, at) === 1;
				break;
			case 'boolif':
				if (byte(view, at) === 1) n[op.key] = true;
				break;
			case 'optboolkey':
				if (byte(view, at) !== none.bool) n[op.key] = byte(view, at) === 1;
				break;
			case 'str':
				n[op.key] = S.strings[view[at >> 2]];
				break;
			case 'optstrkey': {
				const value = tagged(view, at, none.str);
				if (value !== -1) n[op.key] = S.strings[view[value]];
				break;
			}
			case 'enum':
				n[op.key] = op.names[byte(view, at)];
				break;
			case 'optenumkey':
				if (byte(view, at) < op.names.length) n[op.key] = op.names[byte(view, at)];
				break;
			case 'enumor':
				n[op.key] = byte(view, at) < op.names.length ? op.names[byte(view, at)] : op.other;
				break;
			case 'modifier':
				if (byte(view, at) < op.names.length) n[op.key] = op.names[byte(view, at)] === 'true' ? true : op.names[byte(view, at)];
				break;
			case 'boolnames':
				n[op.key] = byte(view, at) === 1 ? op.value : op.other;
				break;
			case 'float': {
				const value = S.numbers[view[at >> 2]];
				n[op.key] = Number.isFinite(value) ? value : null;
				break;
			}
			case 'raw':
				n.raw = S.source.slice(n.start, n.end);
				break;
			case 'bigint':
				n.bigint = BigInt(S.source.slice(n.start, n.end - 1).replaceAll('_', '')).toString();
				break;
			case 'const':
			case 'constbool':
				n[op.key] = op.value;
				break;
			case 'null':
				n[op.key] = null;
				break;
			case 'emptylist':
				n[op.key] = [];
				break;
			case 'object': {
				const inner: Decoded = {};
				apply(S, id, inner, op.inner, view, base);
				n[op.key] = inner;
				break;
			}
			case 'othername': {
				const was = S.name_only;
				S.name_only = view[at >> 2] === view[(base + op.at2) >> 2];
				child(S, n, op.key, view[at >> 2]);
				S.name_only = was;
				break;
			}
			case 'keepif':
				if (S.erase && byte(view, at) === 1) S.kept.push(op.key, id);
				break;
		}
	}
}

function settle(S: State, n: Decoded, id: number, pending: number[] | null) {
	if (pending === null) return;
	for (let i = 0; i < pending.length; i++) late(S, n, pending[i]);
	late(S, n, id);
}

function run(S: State, id: number, ops: Op[], view: Uint32Array, base: number): Decoded {
	let at = 0;
	for (; ; at++) {
		const op = ops[at];
		if (op.op === 'keep') {
			if (S.erase) S.kept.push(op.key, id);
		} else if (op.op === 'through') {
			if (S.erase) {
				S.adopted.push(id);
				return build(S, view[(base + op.at) >> 2] - 1);
			}
		} else break;
	}
	const head = ops[at];
	const n = begin(S, head.op === 'type' ? head.key : head.names[byte(view, base + head.at)], id);
	const pending = S.pending;
	S.pending = null;
	apply(S, id, n, at === 0 ? ops : ops.slice(at), view, base);
	settle(S, n, id, pending);
	return n;
}

function host(S: State, id: number, index: number): Decoded {
	const ty = S.hosts[index * 5], from = S.hosts[index * 5 + 1], len = S.hosts[index * 5 + 2], span = S.hosts[index * 5 + 3];
	let n: Decoded;
	if (ty === 0xffffffff) {
		// an object of the host's without a type, positions and all
		const at = id * S.ps + S.po;
		n = { start: S.P[at], end: S.P[at + 1] };
		if (S.lines) n.loc = loc(S.locs, id * 4);
	} else if (span === 1) n = begin(S, S.names[ty], id);
	else {
		n = S.link ? { type: S.names[ty], [PARENT]: undefined, [SCOPE]: undefined } : { type: S.names[ty] };
		if (S.of_node !== null && !S.name_only) facts(S, n, id);
	}
	const pending = S.pending;
	S.pending = null;
	for (let i = from; i < from + len; i++) {
		const key = S.names[S.host_keys[i]];
		const a = S.host_vals[i * 3 + 1], b = S.host_vals[i * 3 + 2];
		switch (S.host_vals[i * 3]) {
			case 0:
				child(S, n, key, a + 1);
				break;
			case 1:
				list(S, n, key, items(S, a, b));
				break;
			case 2:
				n[key] = S.strings[a];
				break;
			case 3:
				n[key] = S.source.slice(a, b);
				break;
			case 4:
				n[key] = strs(S, a, b);
				break;
			case 5:
				n[key] = a === 1;
				break;
			case 6:
				n[key] = a;
				break;
			case 7:
				n[key] = null;
				break;
			case 8: {
				const all = comments(S);
				if (S.link && n.type !== undefined) for (const c of all) c[PARENT] = n;
				n[key] = all;
				break;
			}
		}
	}
	settle(S, n, id, pending);
	return n;
}

function build(S: State, id: number): Decoded {
	return S.J[S.N[id * S.C.words + (S.C.kind >> 2)]](S, id, 0);
}

function items(S: State, start: number, len: number): (Decoded | null)[] {
	const out: (Decoded | null)[] = [];
	for (let i = start; i < start + len; i++) {
		const word = S.L[i];
		if (word === 0) out.push(null);
		else if (S.erased === null || !bit(S.erased, word - 1)) out.push(S.J[S.N[(word - 1) * S.C.words + (S.C.kind >> 2)]](S, word - 1, 0));
	}
	return out;
}

// erasing drops TypeScript's `this` parameter
function params(S: State, start: number, len: number) {
	if (S.erase && len > 0 && S.L[start] !== 0) {
		const first = (S.L[start] - 1) * S.C.words * 4 + S.C.kind;
		if (S.N[first >> 2] === S.C.identifier && S.strings[S.N[(first + S.C.name) >> 2]] === 'this') return items(S, start + 1, len - 1);
	}
	return items(S, start, len);
}

function link_items(list: (Decoded | null)[], n: Decoded) {
	for (let i = 0; i < list.length; i++) {
		const c = list[i];
		if (c !== null) c[PARENT] = n;
	}
}

const finite = (value: number) => (Number.isFinite(value) ? value : null);
const bigint = (digits: string) => BigInt(digits.replaceAll('_', '')).toString();

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

const LINK = 1, LINES = 2, FACTS = 4, ERASE = 8;
const CONDITIONAL = new Set(['optkey', 'optlistkey', 'boolif', 'optboolkey', 'optstrkey', 'optenumkey', 'modifier', 'keepif']);

// One object literal per kind, its parent and facts as symbol slots of the literal: V8 allocates
// it in one hidden class. Keys a kind may leave out are set after, in the recipe's order, and a
// node with anything the literal has no room for goes to `run`.
// What turns a recipe's operations into source over a record: `word` and `byte` spell a read at a
// byte offset of it. `constants` go to the generated function as `K`, `links` are what to run
// once the object, `n`, exists.
function emitter(C: Compiled, word: (at: number) => string, byte: (at: number) => string, link: boolean, facts: boolean, erase: boolean) {
	const none = C.layout.none;
	const constants: unknown[] = [];
	const constant = (value: unknown) => `K[${constants.push(value) - 1}]`;
	const tagged = ({ tag }: Tagged, at: number) => [word(at + tag * 4), at + (tag === 0 ? 4 : 0)] as const;
	const links: string[] = [];
	// a child's builder is looked up where the child is read: each site sees its own few kinds
	const built = (id: string) => `J[N[${id} * ${C.words} + ${C.kind >> 2}]](S, ${id}, 0)`;
	const child = (local: string, at: number) => `const w${local} = ${word(at)}; const ${local} = w${local} === 0 ? null : ${built(`(w${local} - 1)`)};`;
	let count = 0;
	// the value of an operation every node of the kind has, the statements it needs before it in `lead`
	const value = (op: Op, into: string[]): string => {
		const local = `v${count++}`;
		switch (op.op) {
			case 'node':
				into.push(child(local, op.at));
				if (link) links.push(`if (${local} !== null && ${local}.type !== undefined) ${local}[PARENT] = n;`);
				return local;
			case 'opt':
				if (op.ty === '?str') {
					const [tag, from] = tagged(none.str, op.at);
					return `(${tag} === ${none.str.missing} ? null : S.strings[${word(from)}])`;
				}
				if (op.ty === '?u32') {
					const [tag, from] = tagged(none.int, op.at);
					return `(${tag} === ${none.int.missing} ? null : ${word(from)})`;
				}
				into.push(child(local, op.at));
				if (link) links.push(`if (${local} !== null && ${local}.type !== undefined) ${local}[PARENT] = n;`);
				return local;
			case 'int':
				return word(op.at);
			case 'pair':
				return `[${word(op.at)}, ${word(op.at + 4)}]`;
			case 'list':
			case 'params':
				into.push(`const ${local} = ${op.op === 'params' && erase ? 'params' : 'items'}(S, ${word(op.at)}, ${word(op.at + 4)});`);
				if (link) links.push(`link_items(${local}, n);`);
				return local;
			case 'bool':
				return `${byte(op.at)} === 1`;
			case 'str':
				return `S.strings[${word(op.at)}]`;
			case 'enum':
				return `${constant(op.names)}[${byte(op.at)}]`;
			case 'enumor':
				return `(${constant(op.names)}[${byte(op.at)}] ?? ${JSON.stringify(op.other)})`;
			case 'boolnames':
				return `(${byte(op.at)} === 1 ? ${JSON.stringify(op.value)} : ${JSON.stringify(op.other)})`;
			case 'float':
				return `finite(S.numbers[${word(op.at)}])`;
			case 'raw':
				return 'S.source.slice(S.P[p], S.P[p + 1])';
			case 'bigint':
				return 'bigint(S.source.slice(S.P[p], S.P[p + 1] - 1))';
			case 'const':
			case 'constbool':
				return JSON.stringify(op.value);
			case 'null':
				return 'null';
			case 'emptylist':
				return '[]';
			case 'object':
				return `{ ${op.inner.map((inner) => `${JSON.stringify(inner.key)}: ${value(inner, into)}`).join(', ')} }`;
			case 'othername':
				if (facts) into.push(`const was${local} = S.name_only; S.name_only = ${word(op.at)} === ${word(op.at2)};`);
				into.push(child(local, op.at));
				if (facts) into.push(`S.name_only = was${local};`);
				if (link) links.push(`if (${local}.type !== undefined) ${local}[PARENT] = n;`);
				return local;
		}
		throw new Error(`no value for ${op.op}`);
	};
	return { none, constants, constant, tagged, links, built, value };
}

function generate(C: Compiled, G: Language, config: number, ops: Op[], ts: boolean, adds: boolean): Builder {
	const slow: Builder = ts ? (S, id, record) => run(S, id, ops, S.TS!, record * 4) : (S, id) => run(S, id, ops, S.N, id * C.words * 4 + C.kind);
	const link = (config & LINK) !== 0, facts = (config & FACTS) !== 0, erase = (config & ERASE) !== 0;
	// facts as keys, in the writer's order: only `run` spells them
	if ((facts && !link) || adds || ops.length === 0) return slow;
	const word = (at: number) => (ts ? `T[t + ${at >> 2}]` : `N[b + ${(C.kind + at) >> 2}]`);
	const byte = (at: number) => `(${word(at)} >>> ${(((ts ? 0 : C.kind) + at) & 3) << 3} & 255)`;
	const { none, constants, constant, tagged, links, built, value } = emitter(C, word, byte, link, facts, erase);
	const lead: string[] = [];
	let at = 0;
	for (; ops[at].op === 'keep' || ops[at].op === 'through'; at++) {
		if (!erase) continue;
		if (ops[at].op === 'keep') lead.push(`S.kept.push(${JSON.stringify(ops[at].key)}, id);`);
		else lead.push(`S.adopted.push(id); return ${built(`(${word(ops[at].at)} - 1)`)};`);
	}
	const head = ops[at++];
	const type = head.op === 'type' ? JSON.stringify(head.key) : `${constant(head.names)}[${byte(head.at)}]`;
	const identifier = head.op === 'type' && head.key === 'Identifier';
	const key = (op: Op) => JSON.stringify(op.op === 'raw' || op.op === 'bigint' ? op.op : op.key);
	const props = [`type: ${type}`, 'start: S.P[p]', 'end: S.P[p + 1]'];
	if ((config & LINES) !== 0) props.push('loc: { start: { line: S.locs[id * 4], column: S.locs[id * 4 + 1] }, end: { line: S.locs[id * 4 + 2], column: S.locs[id * 4 + 3] } }');
	if (ops.slice(at).some((op) => op.op === 'object' && op.inner.some((inner) => CONDITIONAL.has(inner.op)))) return slow;
	for (; at < ops.length && !CONDITIONAL.has(ops[at].op); at++) props.push(`${key(ops[at])}: ${value(ops[at], lead)}`);
	// a key some nodes of the kind leave out, and every key after it
	const tail: string[] = [];
	for (; at < ops.length; at++) {
		const op = ops[at];
		if (!CONDITIONAL.has(op.op)) {
			const before = links.length;
			tail.push(`n[${key(op)}] = ${value(op, tail)};`, ...links.splice(before));
			continue;
		}
		const names = constant(op.op === 'modifier' ? op.names.map((name) => (name === 'true' ? true : name)) : op.names);
		switch (op.op) {
			case 'optkey':
				tail.push(`if (${word(op.at)} !== 0) { const c = ${built(`(${word(op.at)} - 1)`)}; n[${key(op)}] = c; ${link ? 'if (c.type !== undefined) c[PARENT] = n;' : ''} }`);
				break;
			case 'optlistkey': {
				const [tag, from] = tagged(none.list, op.at);
				tail.push(`if (${tag} !== ${none.list.missing}) { const c = items(S, ${word(from)}, ${word(from + 4)}); n[${key(op)}] = c; ${link ? 'link_items(c, n);' : ''} }`);
				break;
			}
			case 'boolif':
				tail.push(`if (${byte(op.at)} === 1) n[${key(op)}] = true;`);
				break;
			case 'optboolkey':
				tail.push(`if (${byte(op.at)} !== ${none.bool}) n[${key(op)}] = ${byte(op.at)} === 1;`);
				break;
			case 'optstrkey': {
				const [tag, from] = tagged(none.str, op.at);
				tail.push(`if (${tag} !== ${none.str.missing}) n[${key(op)}] = S.strings[${word(from)}];`);
				break;
			}
			case 'optenumkey':
			case 'modifier':
				tail.push(`if (${byte(op.at)} < ${op.names.length}) n[${key(op)}] = ${names}[${byte(op.at)}];`);
				break;
			case 'keepif':
				if (erase) tail.push(`if (${byte(op.at)} === 1) S.kept.push(${key(op)}, id);`);
				break;
		}
	}
	// what the literal has no room for, the engine marks: parentheses, comments, a TypeScript extra, a binding on what is not an identifier
	const rare = ['(S.rare[id >>> 5] >>> (id & 31) & 1) === 1'];
	const before: string[] = [];
	const after: string[] = [];
	if (link) {
		props.push('[PARENT]: undefined');
		if (!facts) props.push('[SCOPE]: undefined');
		else {
			rare.push('S.name_only', 'S.adopted.length !== 0');
			before.push('const sc = S.of_node[id]; const s = sc === 0 ? undefined : S.scopes[sc - 1];');
			props.push('[SCOPE]: s');
			after.push('if (s !== undefined) s.node = n;');
		}
		if (identifier && !facts) props.push('[REFERENCE]: undefined');
		else if (identifier) {
			before.push('const ro = S.of_identifier[id] - 1; const r = ro === -1 ? undefined : (ro & 1) === 0 ? S.bindings[ro >>> 1] : S.references[ro >>> 1];');
			props.push('[REFERENCE]: r');
			after.push('if (r !== undefined && ((ro & 1) === 1 || r.node === null)) r.node = n;');
		}
		if (facts) after.push('if ((S.late[id >>> 5] >>> (id & 31) & 1) === 1) late(S, n, id);');
	}
	const body = `const N = S.N, J = S.J, b = id * ${C.words}${ts ? ', T = S.TS' : ''}; ${lead.length !== 0 && lead[lead.length - 1].includes('return J[') ? lead.join(' ') : `if (${rare.join(' || ')}) return slow(S, id, t); ${before.join(' ')} const p = id * S.ps + S.po; ${lead.join(' ')} const n = { ${props.join(', ')} }; ${tail.join(' ')} ${links.join(' ')} ${after.join(' ')} return n;`}`;
	return new Function('K', 'slow', 'items', 'params', 'link_items', 'finite', 'bigint', 'late', 'PARENT', 'SCOPE', 'REFERENCE', `return (S, id, t) => { ${body} };`)(constants, slow, items, params, link_items, finite, bigint, late, PARENT, SCOPE, REFERENCE);
}

function strs(S: State, start: number, len: number): string[] {
	const out = new Array<string>(len);
	for (let i = 0; i < len; i++) out[i] = S.strings[S.host_strings[start + i]];
	return out;
}

// The engine numbers the shapes of a host's nodes, a type with its fields' keys and kinds of value:
// one literal each, as the kinds have.
function host_by_shape(S: State, id: number, index: number, config: number): Decoded {
	const from = S.hosts[index * 5 + 1], shape = S.hosts[index * 5 + 4];
	let build = S.H[shape];
	if (build === undefined) {
		const ty = S.hosts[index * 5], len = S.hosts[index * 5 + 2];
		const keys: string[] = [], tags: number[] = [];
		for (let i = from; i < from + len; i++) {
			keys.push(S.names[S.host_keys[i]]);
			tags.push(S.host_vals[i * 3]);
		}
		build = S.H[shape] = generate_host(S.C, config, ty === 0xffffffff ? null : S.names[ty], S.hosts[index * 5 + 3] === 1, keys, tags);
	}
	return build(S, id, index, from);
}

function generate_host(C: Compiled, config: number, type: string | null, span: boolean, keys: string[], tags: number[]): HostBuilder {
	const link = (config & LINK) !== 0 && type !== null, facts = (config & FACTS) !== 0 && type !== null;
	if (facts && (config & LINK) === 0) return host;
	const lead: string[] = [], links: string[] = [];
	const props = type === null ? [] : [`type: ${JSON.stringify(type)}`];
	if (type === null || span) {
		props.push('start: S.P[p]', 'end: S.P[p + 1]');
		if ((config & LINES) !== 0) props.push('loc: { start: { line: S.locs[id * 4], column: S.locs[id * 4 + 1] }, end: { line: S.locs[id * 4 + 2], column: S.locs[id * 4 + 3] } }');
	}
	keys.forEach((key, i) => {
		const a = `HV[(from + ${i}) * 3 + 1]`, b = `HV[(from + ${i}) * 3 + 2]`, v = `v${i}`;
		let value = v;
		if (tags[i] === 0) {
			lead.push(`const ${v} = J[N[${a} * ${C.words} + ${C.kind >> 2}]](S, ${a}, 0);`);
			if (link) links.push(`if (${v}.type !== undefined) ${v}[PARENT] = n;`);
		} else if (tags[i] === 1 || tags[i] === 8) {
			lead.push(`const ${v} = ${tags[i] === 1 ? `items(S, ${a}, ${b})` : 'comments(S)'};`);
			if (link) links.push(`link_items(${v}, n);`);
		} else value = [, , `S.strings[${a}]`, `S.source.slice(${a}, ${b})`, `strs(S, ${a}, ${b})`, `${a} === 1`, a, 'null'][tags[i]]!;
		props.push(`${JSON.stringify(key)}: ${value}`);
	});
	// what the literal has no room for, as for the kinds; a node without a span has only its facts
	const rare = span ? ['(S.rare[id >>> 5] >>> (id & 31) & 1) === 1'] : [];
	const before: string[] = [], after: string[] = [];
	if (link) {
		props.push('[PARENT]: undefined');
		if (!facts) props.push('[SCOPE]: undefined');
		else {
			rare.push('S.name_only', 'S.adopted.length !== 0', '(S.rare[id >>> 5] >>> (id & 31) & 1) === 1');
			before.push('const sc = S.of_node[id]; const s = sc === 0 ? undefined : S.scopes[sc - 1];');
			props.push('[SCOPE]: s');
			after.push('if (s !== undefined) s.node = n;', 'if ((S.late[id >>> 5] >>> (id & 31) & 1) === 1) late(S, n, id);');
		}
	}
	const body = `const N = S.N, J = S.J, HV = S.host_vals; ${rare.length === 0 ? '' : `if (${rare.join(' || ')}) return host(S, id, index);`} ${before.join(' ')} const p = id * S.ps + S.po; ${lead.join(' ')} const n = { ${props.join(', ')} }; ${links.join(' ')} ${after.join(' ')} return n;`;
	return new Function('host', 'items', 'comments', 'strs', 'link_items', 'late', 'PARENT', 'SCOPE', `return (S, id, index, from) => { ${body} };`)(host, items, comments, strs, link_items, late, PARENT, SCOPE);
}

const generated = (() => {
	try {
		new Function('');
		return true;
	} catch {
		return false;
	}
})();

// what a table's row points at once the tree is built, in its literal from the start so nothing is
// added later; a binding is the reference its declaring identifier makes, `binding` itself
const LINKED: Record<string, Record<string, unknown>> = {
	scopes: { node: null },
	bindings: { node: null, declaration: null, binding: null, declares: true, read: false, mutate: false, writeExpr: null },
	references: { node: null, writeExpr: null },
	roots: { node: null },
};

function row(C: Compiled, ops: Op[], linked: Record<string, unknown> | undefined): Row {
	if (!generated) {
		return (S, view, b) => {
			const n: Decoded = {};
			apply(S, 0, n, ops, view, b * 4);
			return Object.assign(n, linked);
		};
	}
	const E = emitter(C, (at) => `V[b + ${at >> 2}]`, (at) => `(V[b + ${at >> 2}] >>> ${(at & 3) << 3} & 255)`, false, false, false);
	const lead: string[] = [];
	const props = ops.map((op) => `${JSON.stringify(op.key)}: ${E.value(op, lead)}`);
	for (const key in linked) props.push(`${JSON.stringify(key)}: ${JSON.stringify(linked[key])}`);
	return new Function('K', `return (S, V, b) => { ${lead.join(' ')} return { ${props.join(', ')} }; };`)(E.constants);
}

// each builder is made the first time its kind is met
function builders(C: Compiled, G: Language, config: number, typescript: boolean): Builders {
	let B = G.sets.get(config);
	G.last = config;
	if (B !== undefined) return (G.builders = B);
	const set: Builders = (B = { js: [], ts: [], hosts: [] });
	const lazy = (list: Builder[], tag: number, make: () => Builder): Builder => (S, id, record) => (list[tag] = make())(S, id, record);
	C.layout.kinds.forEach((_, tag) => {
		const ops = G.recipes[tag];
		if (tag === C.extension) set.js.push((S, id) => { const record = S.N[id * C.words + (C.kind >> 2) + 1] * (C.layout.ts!.size >> 2); return set.ts[S.TS![record]](S, id, record); });
		else if (tag === C.host && generated) {
			set.js.push((S, id) => {
				const index = S.N[id * C.words + (C.kind >> 2) + 1];
				const build = S.H[S.hosts[index * 5 + 4]];
				return build === undefined ? host_by_shape(S, id, index, config) : build(S, id, index, S.hosts[index * 5 + 1]);
			});
		} else if (tag === C.host) set.js.push((S, id) => host(S, id, S.N[id * C.words + (C.kind >> 2) + 1]));
		else set.js.push(lazy(set.js, tag, () => (generated ? generate(C, G, config, ops, false, typescript && (config & ERASE) === 0 && G.adds[tag].length !== 0) : (S, id) => run(S, id, ops, S.N, id * C.words * 4 + C.kind))));
	});
	G.ts.forEach((ops, tag) => set.ts.push(lazy(set.ts, tag, () => (generated ? generate(C, G, config, ops, true, false) : (S, id, record) => run(S, id, ops, S.TS!, record * 4)))));
	G.sets.set(config, set);
	return (G.builders = set);
}

function comments(S: State): Decoded[] {
	const out = new Array<Decoded>(S.comment_count);
	for (let i = 0; i < S.comment_count; i++) out[i] = comment(S, i);
	return out;
}

// the names the engine has numbered since the last answer; a table that shrank started over, and the shapes found by its numbers with it
function names(G: Language, tree: Tree, count: number): string[] {
	if (count === G.names.length || count < 0) return G.names;
	if (count < G.names.length) {
		G.names = [];
		for (const set of G.sets.values()) set.hosts.length = 0;
	}
	const text = tree[G.at.names] as Uint8Array, starts = tree[G.at.name_starts] as Uint32Array;
	for (let i = G.names.length; i < count; i++) G.names.push(utf8.decode(text.subarray(starts[i], starts[i + 1])));
	return G.names;
}

const filled = (tree: Tree, words: Uint32Array, lens: number, at: number) => (words[lens + at] === 0 ? null : (tree[at] as Uint32Array).subarray(0, words[lens + at]));

// what `words[lens]` says of an answer; each view's length follows it
const MOVED = 1, TYPESCRIPT = 2, COMMENTS = 4, ERASED = 8, LINED = 16, LISTED = 32, RECOVERED = 64;

/** What parses, as the reader sees it. */
export interface Views {
	/** The tree's memory layout, the names of its views and the recipes, as JSON. */
	readonly layout: () => string;
	/** The views of the last parse's tree, JavaScript's or TypeScript's; `moved` when a buffer of it has since the last answer. The same array as long as no view in it changed, `moved` aside. */
	readonly tree: (typescript: boolean, moved: boolean) => Tree;
}

/** A source the engine prepared: it parses at an entry and offset, cut at `end`, the stop tokens as one string; the answer is its words, or an error as JSON. */
export interface Prepared {
	readonly parse: (entry: number, offset: number, end: number | undefined, stop: string) => Uint32Array | string;
	readonly free: () => void;
}

/** What parses: the addon or the WebAssembly module. */
export interface Engine extends Views {
	readonly create: (source: string, flags: number, host: string) => Prepared;
}

function table(S: State, tree: Tree, words: Uint32Array, lens: number, rows: Rows, at: number): Decoded[] {
	const build = S.link ? rows.linked : rows.plain;
	const size = rows.words;
	const view = tree[at] as Uint32Array;
	const out = new Array<Decoded>(words[lens + at] / size);
	for (let i = 0; i < out.length; i++) out[i] = build(S, view, i * size);
	return out;
}

// every row arrives with its links in place as nulls, so nothing here adds a property
function link_tables(S: State) {
	const { scopes, bindings, references } = S;
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
	for (const root of S.roots) {
		root.scope = scopes[root.scope];
		root.scopes = scopes.slice(root.scopes[0], root.scopes[1]);
		root.bindings = bindings.slice(root.bindings[0], root.bindings[1]);
		root.references = references.slice(root.references[0], root.references[1]);
	}
}

function errors(S: State, view: Uint32Array, count: number): Decoded[] {
	const out = new Array<Decoded>(count);
	for (let i = 0; i < count; i++) out[i] = { code: S.strings[view[i * 6]], message: S.strings[view[i * 6 + 1]], pos: view[i * 6 + 2], end: view[i * 6 + 3], loc: { line: view[i * 6 + 4], column: view[i * 6 + 5] } };
	return out;
}

/**
 * The answer of the last parse: `words` are `end`, the roots by number, a word of what the answer
 * is and each view's length; the tree itself is read in place. `link` replaces the scope and
 * binding numbers with the objects they index.
 */
export function decode(words: Uint32Array, source: string, engine: Views, link = true): Decoded {
	const C = compile(engine);
	const lens = 2 + words[1];
	const what = words[lens];
	const typescript = (what & TYPESCRIPT) !== 0;
	const tree = engine.tree(typescript, (what & MOVED) !== 0);
	const erase = (what & ERASED) !== 0, lines = (what & LINED) !== 0, listed = (what & LISTED) !== 0;
	const G = typescript ? C.ts! : C.js;
	const at = G.at;
	const spans = words[lens + at.spans] !== 0;
	const scoped = words[lens + at.scopes] !== 0;
	const config = (link ? LINK : 0) | (lines ? LINES : 0) | (scoped ? FACTS : 0) | (erase ? ERASE : 0);
	const B = G.last === config ? G.builders! : builders(C, G, config, typescript);
	// the state stays with the tree's views, one answer at a time: only what an answer changes is set
	let S = G.state;
	if (S === undefined || (what & MOVED) !== 0 || G.tree !== tree) {
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
	S.J = B.js;
	S.H = B.hosts;
	S.names = names(G, tree, words[lens + at.name_starts] - 1);
	S.strings = strings(tree, words, lens, at);
	S.P = spans ? (tree[at.spans] as Uint32Array) : S.N;
	S.ps = spans ? 2 : C.words;
	S.po = spans ? 0 : C.layout.node.start >> 2;
	S.parenthesized = filled(tree, words, lens, at.parenthesized);
	S.erased = erase ? filled(tree, words, lens, at.erased) : null;
	S.comment_count = words[lens + at.comments] / 9;
	S.attached_slots = filled(tree, words, lens, at.attached_slots);
	S.of_node = scoped ? (tree[at.of_node] as Uint32Array) : null;
	S.extras_slots = typescript ? filled(tree, words, lens, at.extras_slots) : null;
	S.name_only = false;
	S.pending = null;
	S.adopted.length = 0;
	S.kept.length = 0;
	if (scoped) {
		S.scopes = table(S, tree, words, lens, C.rows.scopes, at.scopes);
		S.bindings = table(S, tree, words, lens, C.rows.bindings, at.bindings);
		S.references = table(S, tree, words, lens, C.rows.references, at.references);
		S.roots = table(S, tree, words, lens, C.rows.roots, at.roots);
		if (link) link_tables(S);
	}
	let node: Decoded | Decoded[];
	if (listed) {
		node = [];
		for (let i = 2; i < lens; i++) if (S.erased === null || !bit(S.erased, words[i])) node.push(build(S, words[i]));
	} else node = build(S, words[2]);
	const answer: Decoded = { node, end: words[0] };
	if ((what & COMMENTS) !== 0) answer.comments = comments(S);
	if ((what & RECOVERED) !== 0) answer.errors = errors(S, tree[at.errors] as Uint32Array, words[lens + at.errors] / 6);
	if (erase) answer.typescript = kept(S);
	if (scoped) {
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
		if (S.lines) n.loc = loc(S.locs, id * 4);
		out.push(n);
	}
	return out.sort((a, b) => a.start - b.start);
}
