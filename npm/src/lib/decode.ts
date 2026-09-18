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
/** How the compiler spells a missing optional of each type. */
interface Missing {
	enum: number;
	bool: number;
	list: Tagged;
	str: Tagged;
	int: Tagged;
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
	none: Missing;
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
	layout: Layout;
	/** Each view's place in the tree, by name. */
	at: At;
	/** By a kind's tag, resolved the first time the kind is met. */
	recipes: (Op[] | undefined)[];
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
	/** The hosts' shapes as operations, by the number the engine gave each shape. */
	host_ops: Map<number, Op[]>;
	/** The state of the last answer read from `tree`, kept for the next. */
	state: State | undefined;
	tree: Tree | undefined;
	/** Where the tree's buffers sat when the state was taken, as the answer folds it. */
	sat: [number, number];
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
	/** Each table's row builders, plain and linked, and its record's words; made on the first answer with scopes. */
	rows: { scopes: Rows; bindings: Rows; references: Rows; roots: Rows } | undefined;
	js: Language;
	/** Made on the first TypeScript answer. */
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
	/** Past the last view's length in the answer's words: where the tree's buffers sit, folded into two. */
	sits: number;
}

function language(layout: Layout, views: string[], typescript: boolean): Language {
	const by = (recipes: RawRecipes, kinds: Kind[]) => kinds.map((kind) => resolve(recipes.find(([name]) => name === kind.name)?.[1] ?? [], kind.fields));
	// a literal: an object filled by computed keys turns into a dictionary, a hash lookup per read
	const of = (name: string) => 1 + views.indexOf(name);
	const at: At = { nodes: of('nodes'), lists: of('lists'), numbers: of('numbers'), text: of('text'), starts: of('starts'), units: of('units'), spans: of('spans'), locs: of('locs'), parenthesized: of('parenthesized'), erased: of('erased'), comments: of('comments'), attached_slots: of('attached_slots'), attached: of('attached'), errors: of('errors'), hosts: of('hosts'), host_keys: of('host_keys'), host_vals: of('host_vals'), host_strings: of('host_strings'), scopes: of('scopes'), bindings: of('bindings'), references: of('references'), roots: of('roots'), of_node: of('of_node'), of_identifier: of('of_identifier'), root_of: of('root_of'), declared_by: of('declared_by'), declared_by_at: of('declared_by_at'), writes_of: of('writes_of'), writes_of_at: of('writes_of_at'), ts: of('ts'), extras_slots: of('extras_slots'), extras: of('extras'), names: of('names'), name_starts: of('name_starts'), rare: of('rare'), late: of('late'), sits: 1 + views.length };
	const out: Language = { layout, at, recipes: new Array<Op[] | undefined>(layout.kinds.length), ts: [], adds: [], extras: [], erased: [], sets: new Map(), last: -1, builders: undefined, names: [], host_ops: new Map(), state: undefined, tree: undefined, sat: [-1, -1] };
	if (typescript) {
		const extras = layout.extras!.fields;
		out.ts = by(layout.recipes.ts!, layout.ts!.kinds);
		out.adds = layout.kinds.map((kind) => resolve(layout.recipes.adds!.find(([name]) => name === kind.name)?.[1] ?? [], extras));
		out.extras = resolve(layout.recipes.extras!.find(([name]) => name === 'extras')![1], extras);
		out.erased = resolve(layout.recipes.extras!.find(([name]) => name === 'erased')![1], extras);
	}
	return out;
}

const recipe = (G: Language, tag: number): Op[] => (G.recipes[tag] ??= resolve(G.layout.recipes.js.find(([name]) => name === G.layout.kinds[tag].name)?.[1] ?? [], G.layout.kinds[tag].fields));

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
		rows: undefined,
		js: language(layout, layout.views.js, false),
		ts: undefined,
	};
	compiled.set(engine, made);
	return made;
}

function tables(C: Compiled): NonNullable<Compiled['rows']> {
	const { layout } = C;
	const rows = (name: string): Rows => {
		const ops = resolve(layout.recipes.rows.find(([table]) => table === name)![1], layout.rows[name].fields);
		return { plain: row(C, ops, undefined), linked: row(C, ops, LINKED[name]), words: layout.rows[name].size >> 2 };
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

// ── What an operation reads and writes, spelled once. A `Spelling` is what spelling it takes,
// `E` a value and `St` a statement: the generator spells source, the interpreter closures.
interface Spelling<E, St> {
	/** The record's word, and byte, at a byte offset of it. */
	word(at: number): E;
	byte(at: number): E;
	lit(value: string | number | boolean | null): E;
	empty(): E;
	dec(value: E): E;
	eq(a: E, b: E): E;
	ne(a: E, b: E): E;
	lt(a: E, b: E): E;
	/** `a ?? b` */
	or(a: E, b: E): E;
	pick(test: E, yes: E, no: E): E;
	strings(index: E): E;
	numbers(index: E): E;
	/** A constant table, and an element of one. */
	table(list: unknown[]): E;
	at(table: E, index: E): E;
	/** The node with an id. */
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
	/** A value computed once, before the keys are set, and read where it is used. */
	hold(value: E): E;
	/** `hold`, the node built while `name_only` says whether it names what another declares. */
	othername(same: E, id: E): E;
	set(key: string, value: E): St;
	/** The parent link of a node value, `known` when it is never null. */
	link(value: E, known: boolean): St;
	link_items(value: E): St;
	when(test: E, body: () => St[]): St;
	/** Under erasure: the key is what erasure keeps of the node. */
	kept(key: string): St;
}

// `raw` and `bigint` are keys of their own name
const key_of = (op: Op) => (op.op === 'raw' || op.op === 'bigint' ? op.op : op.key);

// an optional's tag word sits among its value's words, which follow their order around it
const tagged = ({ tag }: Tagged, at: number) => [at + tag * 4, at + (tag === 0 ? 4 : 0)] as const;

const child = <E, St>(B: Spelling<E, St>, word: E): E => {
	const w = B.hold(word);
	return B.hold(B.pick(B.eq(w, B.lit(0)), B.lit(null), B.built(B.dec(w))));
};

/** An operation every node of the kind has: its value, and what runs once the node holds it. */
function value<E, St>(B: Spelling<E, St>, op: Op, none: Missing, erase: boolean, after: St[]): E {
	switch (op.op) {
		case 'node': {
			const v = child(B, B.word(op.at));
			after.push(B.link(v, false));
			return v;
		}
		case 'opt': {
			if (op.ty === '?str' || op.ty === '?u32') {
				const which = op.ty === '?str' ? none.str : none.int;
				const [tag, from] = tagged(which, op.at);
				return B.pick(B.eq(B.word(tag), B.lit(which.missing)), B.lit(null), op.ty === '?str' ? B.strings(B.word(from)) : B.word(from));
			}
			const v = child(B, B.word(op.at));
			after.push(B.link(v, false));
			return v;
		}
		case 'int':
			return B.word(op.at);
		case 'pair':
			return B.pair(B.word(op.at), B.word(op.at + 4));
		case 'list':
		case 'params': {
			const v = B.hold(B.items(B.word(op.at), B.word(op.at + 4), op.op === 'params' && erase));
			after.push(B.link_items(v));
			return v;
		}
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
		case 'const':
		case 'constbool':
			return B.lit(op.value);
		case 'null':
			return B.lit(null);
		case 'emptylist':
			return B.empty();
		case 'object':
			return B.object(op.inner.map((inner) => [inner.key, value(B, inner, none, erase, after)]));
		case 'host':
			return host_value(B, op.value, op.at, after);
		case 'othername': {
			const w = B.hold(B.word(op.at));
			const v = B.othername(B.eq(w, B.word(op.at2)), B.dec(w));
			after.push(B.link(v, false));
			return v;
		}
	}
	throw new Error(`no value for ${op.op}`);
}

/** An operation only some nodes of the kind have: what sets its key when they do. */
function conditional<E, St>(B: Spelling<E, St>, op: Op, none: Missing, erase: boolean): St | undefined {
	const key = op.key;
	switch (op.op) {
		case 'optkey':
			return B.when(B.ne(B.word(op.at), B.lit(0)), () => {
				const c = B.hold(B.built(B.dec(B.word(op.at))));
				return [B.set(key, c), B.link(c, true)];
			});
		case 'optlistkey': {
			const [tag, from] = tagged(none.list, op.at);
			return B.when(B.ne(B.word(tag), B.lit(none.list.missing)), () => {
				const c = B.hold(B.items(B.word(from), B.word(from + 4), false));
				return [B.set(key, c), B.link_items(c)];
			});
		}
		case 'boolif':
			return B.when(B.eq(B.byte(op.at), B.lit(1)), () => [B.set(key, B.lit(true))]);
		case 'optboolkey':
			return B.when(B.ne(B.byte(op.at), B.lit(none.bool)), () => [B.set(key, B.eq(B.byte(op.at), B.lit(1)))]);
		case 'optstrkey': {
			const [tag, from] = tagged(none.str, op.at);
			return B.when(B.ne(B.word(tag), B.lit(none.str.missing)), () => [B.set(key, B.strings(B.word(from)))]);
		}
		case 'optenumkey':
		case 'modifier': {
			const names = op.op === 'modifier' ? op.names.map((name) => (name === 'true' ? true : name)) : op.names;
			return B.when(B.lt(B.byte(op.at), B.lit(names.length)), () => [B.set(key, B.at(B.table(names), B.byte(op.at)))]);
		}
		case 'keepif':
			return erase ? B.when(B.eq(B.byte(op.at), B.lit(1)), () => [B.kept(key)]) : undefined;
	}
	return undefined;
}

// the engine tags a host node's field by the kind of value: a node, a list, a string, a slice of
// the source, strings, a boolean, a number, null, every comment
const HOST_NODE = 0, HOST_LIST = 1, HOST_COMMENTS = 8;

/** A host node's field from the three words at `at`: its tag, then two of value; what runs once the node holds it goes to `after`. */
function host_value<E, St>(B: Spelling<E, St>, tag: number, at: number, after: St[]): E {
	const a = B.word(at + 4), b = B.word(at + 8);
	switch (tag) {
		case HOST_NODE: {
			const v = B.hold(B.built(a));
			after.push(B.link(v, true));
			return v;
		}
		case HOST_LIST:
		case HOST_COMMENTS: {
			const v = B.hold(tag === HOST_LIST ? B.items(a, b, false) : B.comments());
			after.push(B.link_items(v));
			return v;
		}
		case 2:
			return B.strings(a);
		case 3:
			return B.text(a, b);
		case 4:
			return B.strs(a, b);
		case 5:
			return B.eq(a, B.lit(1));
		case 6:
			return a;
		case 7:
			return B.lit(null);
	}
	throw new Error(`no host value ${tag}`);
}

// ── The closure spelling: what the interpreter runs where code generation is forbidden. A held
// value lives in `L`, the locals of one node's build.
type Value = (S: State, V: Uint32Array, b: number, n: Decoded, L: unknown[]) => any;
type Stmt = (S: State, V: Uint32Array, b: number, n: Decoded, L: unknown[], id: number) => void;

/** A kind's operations as closures: the leads, then the keys in order, then what follows. */
interface Program {
	slots: number;
	lead: Stmt[];
	body: Stmt[];
	after: Stmt[];
}

class Closures implements Spelling<Value, Stmt> {
	slots = 0;
	lead: Stmt[] = [];
	word = (at: number): Value => (S, V, b) => V[(b + at) >> 2];
	byte = (at: number): Value => (S, V, b) => byte(V, b + at);
	lit = (value: unknown): Value => () => value;
	empty = (): Value => () => [];
	dec = (value: Value): Value => (S, V, b, n, L) => value(S, V, b, n, L) - 1;
	eq = (a: Value, c: Value): Value => (S, V, b, n, L) => a(S, V, b, n, L) === c(S, V, b, n, L);
	ne = (a: Value, c: Value): Value => (S, V, b, n, L) => a(S, V, b, n, L) !== c(S, V, b, n, L);
	lt = (a: Value, c: Value): Value => (S, V, b, n, L) => a(S, V, b, n, L) < c(S, V, b, n, L);
	or = (a: Value, c: Value): Value => (S, V, b, n, L) => a(S, V, b, n, L) ?? c(S, V, b, n, L);
	pick = (test: Value, yes: Value, no: Value): Value => (S, V, b, n, L) => (test(S, V, b, n, L) ? yes(S, V, b, n, L) : no(S, V, b, n, L));
	strings = (index: Value): Value => (S, V, b, n, L) => S.strings[index(S, V, b, n, L)];
	numbers = (index: Value): Value => (S, V, b, n, L) => S.numbers[index(S, V, b, n, L)];
	table = (list: unknown[]): Value => () => list;
	at = (table: Value, index: Value): Value => (S, V, b, n, L) => (table(S, V, b, n, L) as unknown[])[index(S, V, b, n, L)];
	built = (id: Value): Value => (S, V, b, n, L) => build(S, id(S, V, b, n, L));
	items = (start: Value, len: Value, params_: boolean): Value => (S, V, b, n, L) => (params_ ? params : items)(S, start(S, V, b, n, L), len(S, V, b, n, L));
	pair = (a: Value, c: Value): Value => (S, V, b, n, L) => [a(S, V, b, n, L), c(S, V, b, n, L)];
	object = (fields: [string, Value][]): Value => (S, V, b, n, L) => {
		const o: Decoded = {};
		for (const [key, field] of fields) o[key] = field(S, V, b, n, L);
		return o;
	};
	slice = (back: number): Value => (S, V, b, n) => S.source.slice(n.start, n.end - back);
	text = (start: Value, end: Value): Value => (S, V, b, n, L) => S.source.slice(start(S, V, b, n, L), end(S, V, b, n, L));
	strs = (start: Value, len: Value): Value => (S, V, b, n, L) => strs(S, start(S, V, b, n, L), len(S, V, b, n, L));
	comments = (): Value => (S) => comments(S);
	finite = (value: Value): Value => (S, V, b, n, L) => finite(value(S, V, b, n, L));
	bigint = (text: Value): Value => (S, V, b, n, L) => bigint(text(S, V, b, n, L));
	hold(value: Value): Value {
		const slot = this.slots++;
		this.lead.push((S, V, b, n, L) => {
			L[slot] = value(S, V, b, n, L);
		});
		return (S, V, b, n, L) => L[slot];
	}
	othername(same: Value, id: Value): Value {
		const slot = this.slots++;
		this.lead.push((S, V, b, n, L) => {
			const was = S.name_only;
			S.name_only = same(S, V, b, n, L);
			L[slot] = build(S, id(S, V, b, n, L));
			S.name_only = was;
		});
		return (S, V, b, n, L) => L[slot];
	}
	set = (key: string, value: Value): Stmt => (S, V, b, n, L) => {
		n[key] = value(S, V, b, n, L);
	};
	// a child with a type is a node; a literal's regex or a template element's value is not
	link = (value: Value, known: boolean): Stmt => (S, V, b, n, L) => {
		const c = value(S, V, b, n, L);
		if (S.link && (known || c !== null) && n.type !== undefined && c.type !== undefined) c[PARENT] = n;
	};
	link_items = (value: Value): Stmt => (S, V, b, n, L) => {
		if (S.link && n.type !== undefined) link_items(value(S, V, b, n, L), n);
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

// the operations past the head: a kind's own keys, or a table row's, or an extras record's
const programs = new WeakMap<Op[], Program>();
function program(ops: Op[], none: Missing): Program {
	let made = programs.get(ops);
	if (made !== undefined) return made;
	const B = new Closures();
	const body: Stmt[] = [], after: Stmt[] = [];
	for (const op of ops) {
		if (op.op === 'type' || op.op === 'typeof' || op.op === 'keep' || op.op === 'through') continue;
		if (CONDITIONAL.has(op.op)) {
			const set = conditional(B, op, none, true);
			if (set !== undefined) body.push(set);
		} else body.push(B.set(key_of(op), value(B, op, none, true, after)));
	}
	programs.set(ops, (made = { slots: B.slots, lead: B.lead, body, after }));
	return made;
}

const NO_LOCALS: unknown[] = [];

function apply(S: State, id: number, n: Decoded, ops: Op[], view: Uint32Array, base: number) {
	const { slots, lead, body, after } = program(ops, S.C.layout.none);
	const L = slots === 0 ? NO_LOCALS : new Array<unknown>(slots);
	for (let i = 0; i < lead.length; i++) lead[i](S, view, base, n, L, id);
	for (let i = 0; i < body.length; i++) body[i](S, view, base, n, L, id);
	for (let i = 0; i < after.length; i++) after[i](S, view, base, n, L, id);
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
	apply(S, id, n, ops, view, base);
	settle(S, n, id, pending);
	return n;
}

// a host node's fields as operations, one list per shape the engine numbers
function host_ops(S: State, index: number): Op[] {
	const shape = S.hosts[index * 5 + 4];
	let ops = S.G.host_ops.get(shape);
	if (ops === undefined) {
		const from = S.hosts[index * 5 + 1], len = S.hosts[index * 5 + 2];
		ops = [];
		for (let i = 0; i < len; i++) ops.push({ op: 'host', key: S.names[S.host_keys[from + i]], at: i * 12, ty: '', names: [], at2: 0, value: S.host_vals[(from + i) * 3], other: '', inner: [] });
		S.G.host_ops.set(shape, ops);
	}
	return ops;
}

function host(S: State, id: number, index: number): Decoded {
	const ty = S.hosts[index * 5], from = S.hosts[index * 5 + 1], span = S.hosts[index * 5 + 3];
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
	apply(S, id, n, host_ops(S, index), S.host_vals, from * 12);
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

// ── The source spelling: what a generated builder is made of. `word` and `byte` spell a read of
// the record at hand: a node's, a TypeScript record's, a table row's, a host node's fields.
class Source implements Spelling<string, string> {
	constants: unknown[] = [];
	lead: string[] = [];
	count = 0;
	readonly word: (at: number) => string;
	readonly byte: (at: number) => string;
	readonly links: boolean;
	readonly facts: boolean;
	readonly words: number;
	readonly kind: number;
	constructor(word: (at: number) => string, byte: (at: number) => string, links: boolean, facts: boolean, words: number, kind: number) {
		this.word = word;
		this.byte = byte;
		this.links = links;
		this.facts = facts;
		this.words = words;
		this.kind = kind;
	}
	lit = (value: unknown) => JSON.stringify(value);
	empty = () => '[]';
	dec = (value: string) => `(${value} - 1)`;
	eq = (a: string, b: string) => `${a} === ${b}`;
	ne = (a: string, b: string) => `${a} !== ${b}`;
	lt = (a: string, b: string) => `${a} < ${b}`;
	or = (a: string, b: string) => `(${a} ?? ${b})`;
	pick = (test: string, yes: string, no: string) => `(${test} ? ${yes} : ${no})`;
	strings = (index: string) => `S.strings[${index}]`;
	numbers = (index: string) => `S.numbers[${index}]`;
	table = (list: unknown[]) => `K[${this.constants.push(list) - 1}]`;
	at = (table: string, index: string) => `${table}[${index}]`;
	// a child's builder is looked up where the child is read: each site sees its own few kinds
	built = (id: string) => `J[N[${id} * ${this.words} + ${this.kind >> 2}]](S, ${id}, 0)`;
	items = (start: string, len: string, params: boolean) => `${params ? 'params' : 'items'}(S, ${start}, ${len})`;
	pair = (a: string, b: string) => `[${a}, ${b}]`;
	object = (fields: [string, string][]) => `{ ${fields.map(([key, field]) => `${JSON.stringify(key)}: ${field}`).join(', ')} }`;
	slice = (back: number) => `S.source.slice(S.P[p], S.P[p + 1]${back === 0 ? '' : ` - ${back}`})`;
	text = (start: string, end: string) => `S.source.slice(${start}, ${end})`;
	strs = (start: string, len: string) => `strs(S, ${start}, ${len})`;
	comments = () => 'comments(S)';
	finite = (value: string) => `finite(${value})`;
	bigint = (text: string) => `bigint(${text})`;
	hold(value: string): string {
		const local = `v${this.count++}`;
		this.lead.push(`const ${local} = ${value};`);
		return local;
	}
	othername(same: string, id: string): string {
		const local = `v${this.count++}`;
		if (this.facts) this.lead.push(`const was${local} = S.name_only; S.name_only = ${same};`);
		this.lead.push(`const ${local} = ${this.built(id)};`);
		if (this.facts) this.lead.push(`S.name_only = was${local};`);
		return local;
	}
	set = (key: string, value: string) => `n[${JSON.stringify(key)}] = ${value};`;
	link = (value: string, known: boolean) => (this.links ? `if (${known ? '' : `${value} !== null && `}${value}.type !== undefined) ${value}[PARENT] = n;` : '');
	link_items = (value: string) => (this.links ? `link_items(${value}, n);` : '');
	when(test: string, body: () => string[]): string {
		const outer = this.lead;
		const lead: string[] = (this.lead = []);
		const inner = body();
		this.lead = outer;
		const all = [...lead, ...inner].filter((statement) => statement !== '');
		return all.length === 1 ? `if (${test}) ${all[0]}` : `if (${test}) { ${all.join(' ')} }`;
	}
	kept = (key: string) => `S.kept.push(${JSON.stringify(key)}, id);`;
}

const LOC = 'loc: { start: { line: S.locs[id * 4], column: S.locs[id * 4 + 1] }, end: { line: S.locs[id * 4 + 2], column: S.locs[id * 4 + 3] } }';
const RARE = '(S.rare[id >>> 5] >>> (id & 31) & 1) === 1';
const LATE = 'if ((S.late[id >>> 5] >>> (id & 31) & 1) === 1) late(S, n, id);';

// One object literal per kind, its parent and facts as symbol slots of the literal: V8 allocates
// it in one hidden class. Keys a kind may leave out are set after, in the recipe's order, and a
// node with anything the literal has no room for goes to `run`.
function generate(C: Compiled, G: Language, config: number, ops: Op[], ts: boolean, adds: boolean): Builder {
	const slow: Builder = ts ? (S, id, record) => run(S, id, ops, S.TS!, record * 4) : (S, id) => run(S, id, ops, S.N, id * C.words * 4 + C.kind);
	const link = (config & LINK) !== 0, facts = (config & FACTS) !== 0, erase = (config & ERASE) !== 0;
	// facts as keys, in the writer's order: only `run` spells them
	if ((facts && !link) || adds || ops.length === 0) return slow;
	const word = (at: number) => (ts ? `T[t + ${at >> 2}]` : `N[b + ${(C.kind + at) >> 2}]`);
	const byte = (at: number) => `(${word(at)} >>> ${(((ts ? 0 : C.kind) + at) & 3) << 3} & 255)`;
	const B = new Source(word, byte, link, facts, C.words, C.kind);
	const none = C.layout.none;
	let at = 0;
	for (; ops[at].op === 'keep' || ops[at].op === 'through'; at++) {
		if (!erase) continue;
		if (ops[at].op === 'keep') B.lead.push(B.kept(ops[at].key));
		else B.lead.push(`S.adopted.push(id); return ${B.built(B.dec(word(ops[at].at)))};`);
	}
	const head = ops[at++];
	const type = head.op === 'type' ? JSON.stringify(head.key) : B.at(B.table(head.names), byte(head.at));
	const identifier = head.op === 'type' && head.key === 'Identifier';
	const props = [`type: ${type}`, 'start: S.P[p]', 'end: S.P[p + 1]'];
	if ((config & LINES) !== 0) props.push(LOC);
	if (ops.slice(at).some((op) => op.op === 'object' && op.inner.some((inner) => CONDITIONAL.has(inner.op)))) return slow;
	const links: string[] = [];
	for (; at < ops.length && !CONDITIONAL.has(ops[at].op); at++) props.push(`${JSON.stringify(key_of(ops[at]))}: ${value(B, ops[at], none, erase, links)}`);
	// a key some nodes of the kind leave out, and every key after it
	const tail: string[] = [];
	for (; at < ops.length; at++) {
		const op = ops[at];
		if (CONDITIONAL.has(op.op)) {
			const set = conditional(B, op, none, erase);
			if (set !== undefined) tail.push(set);
			continue;
		}
		const lead = B.lead;
		B.lead = tail;
		const after: string[] = [];
		const v = value(B, op, none, erase, after);
		B.lead = lead;
		tail.push(B.set(key_of(op), v), ...after);
	}
	// what the literal has no room for, the engine marks: parentheses, comments, a TypeScript extra, a binding on what is not an identifier
	const rare = [RARE];
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
		if (facts) after.push(LATE);
	}
	const lead = B.lead;
	const body = `const N = S.N, J = S.J, b = id * ${C.words}${ts ? ', T = S.TS' : ''}; ${lead.length !== 0 && lead[lead.length - 1].includes('return J[') ? lead.join(' ') : `if (${rare.join(' || ')}) return slow(S, id, t); ${before.join(' ')} const p = id * S.ps + S.po; ${lead.join(' ')} const n = { ${props.join(', ')} }; ${tail.join(' ')} ${links.join(' ')} ${after.join(' ')} return n;`}`;
	return new Function('K', 'slow', 'items', 'params', 'link_items', 'finite', 'bigint', 'late', 'PARENT', 'SCOPE', 'REFERENCE', `return (S, id, t) => { ${body} };`)(B.constants, slow, items, params, link_items, finite, bigint, late, PARENT, SCOPE, REFERENCE);
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
	const B = new Source((at) => `HV[from * 3 + ${at >> 2}]`, () => '', link, facts, C.words, C.kind);
	const links: string[] = [];
	const props = type === null ? [] : [`type: ${JSON.stringify(type)}`];
	if (type === null || span) {
		props.push('start: S.P[p]', 'end: S.P[p + 1]');
		if ((config & LINES) !== 0) props.push(LOC);
	}
	keys.forEach((key, i) => props.push(`${JSON.stringify(key)}: ${host_value(B, tags[i], i * 12, links)}`));
	// what the literal has no room for, as for the kinds; a node without a span has only its facts
	const rare = span ? [RARE] : [];
	const before: string[] = [], after: string[] = [];
	if (link) {
		props.push('[PARENT]: undefined');
		if (!facts) props.push('[SCOPE]: undefined');
		else {
			rare.push('S.name_only', 'S.adopted.length !== 0', RARE);
			before.push('const sc = S.of_node[id]; const s = sc === 0 ? undefined : S.scopes[sc - 1];');
			props.push('[SCOPE]: s');
			after.push('if (s !== undefined) s.node = n;', LATE);
		}
	}
	const body = `const N = S.N, J = S.J, HV = S.host_vals; ${rare.length === 0 ? '' : `if (${rare.join(' || ')}) return host(S, id, index);`} ${before.join(' ')} const p = id * S.ps + S.po; ${B.lead.join(' ')} const n = { ${props.join(', ')} }; ${links.join(' ')} ${after.join(' ')} return n;`;
	return new Function('K', 'host', 'items', 'comments', 'strs', 'link_items', 'late', 'PARENT', 'SCOPE', `return (S, id, index, from) => { ${body} };`)(B.constants, host, items, comments, strs, link_items, late, PARENT, SCOPE);
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
	const B = new Source((at) => `V[b + ${at >> 2}]`, (at) => `(V[b + ${at >> 2}] >>> ${(at & 3) << 3} & 255)`, false, false, C.words, C.kind);
	const after: string[] = [];
	const props = ops.map((op) => `${JSON.stringify(op.key)}: ${value(B, op, C.layout.none, false, after)}`);
	for (const key in linked) props.push(`${JSON.stringify(key)}: ${JSON.stringify(linked[key])}`);
	return new Function('K', `return (S, V, b) => { ${B.lead.join(' ')} return { ${props.join(', ')} }; };`)(B.constants);
}

// each builder is made the first time its kind is met
function builders(C: Compiled, G: Language, config: number, typescript: boolean): Builders {
	let B = G.sets.get(config);
	G.last = config;
	if (B !== undefined) return (G.builders = B);
	// one closure stands in for every kind not met yet: it makes the kind's builder and takes its place
	const js = (tag: number): Builder => {
		const ops = recipe(G, tag);
		return generated ? generate(C, G, config, ops, false, typescript && (config & ERASE) === 0 && G.adds[tag].length !== 0) : (S, id) => run(S, id, ops, S.N, id * C.words * 4 + C.kind);
	};
	const ts = (tag: number): Builder => (generated ? generate(C, G, config, G.ts[tag], true, false) : (S, id, record) => run(S, id, G.ts[tag], S.TS!, record * 4));
	const set: Builders = (B = {
		js: new Array<Builder>(C.layout.kinds.length).fill((S, id, record) => (set.js[S.N[id * C.words + (C.kind >> 2)]] = js(S.N[id * C.words + (C.kind >> 2)]))(S, id, record)),
		ts: new Array<Builder>(G.ts.length).fill((S, id, record) => (set.ts[S.TS![record]] = ts(S.TS![record]))(S, id, record)),
		hosts: [],
	});
	set.js[C.extension] = (S, id) => { const record = S.N[id * C.words + (C.kind >> 2) + 1] * (C.layout.ts!.size >> 2); return set.ts[S.TS![record]](S, id, record); };
	set.js[C.host] = generated
		? (S, id) => {
				const index = S.N[id * C.words + (C.kind >> 2) + 1];
				const build = S.H[S.hosts[index * 5 + 4]];
				return build === undefined ? host_by_shape(S, id, index, config) : build(S, id, index, S.hosts[index * 5 + 1]);
			}
		: (S, id) => host(S, id, S.N[id * C.words + (C.kind >> 2) + 1]);
	G.sets.set(config, set);
	return (G.builders = set);
}

function comments(S: State): Decoded[] {
	const out = new Array<Decoded>(S.comment_count);
	for (let i = 0; i < S.comment_count; i++) out[i] = comment(S, i);
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

// what `words[lens]` says of an answer; each view's length follows it, then where the tree's buffers sit
const TYPESCRIPT = 2, COMMENTS = 4, ERASED = 8, LINED = 16, LISTED = 32, RECOVERED = 64;

/** What parses, as the reader sees it. */
export interface Views {
	/** The tree's memory layout, the names of its views and the recipes, as JSON. */
	readonly layout: () => string;
	/** The views of the last parse's tree, JavaScript's or TypeScript's; `moved` when a buffer of it has since this reader last took them. The same array as long as no view in it changed, `moved` aside. */
	readonly tree: (typescript: boolean, moved: boolean) => Tree;
}

/** What the engine holds: a prepared source, or a host language's grammar read once. */
export interface Held {
	readonly free: () => void;
}

/** A source the engine prepared: it parses at an entry and offset, cut at `end`, the stop tokens as one string, the whole source as a document by a grammar `plan` holds; the answer is its words, or an error as JSON. */
export interface Prepared extends Held {
	readonly parse: (entry: number, offset: number, end: number | undefined, stop: string, plan: Held | undefined) => Uint32Array | string;
}

/** What parses: the addon or the WebAssembly module. */
export interface Engine extends Views {
	readonly create: (source: string, flags: number) => Prepared;
	/** The grammar of a host language, read once; a `TypeError` says where it could not be read. */
	readonly plan: (grammar: string) => Held;
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
	const G = typescript ? (C.ts ??= language(C.layout, C.layout.views.ts!, true)) : C.js;
	const at = G.at;
	const lo = words[lens + at.sits], hi = words[lens + at.sits + 1];
	const moved = lo !== G.sat[0] || hi !== G.sat[1];
	G.sat[0] = lo;
	G.sat[1] = hi;
	const tree = engine.tree(typescript, moved);
	const erase = (what & ERASED) !== 0, lines = (what & LINED) !== 0, listed = (what & LISTED) !== 0;
	const spans = words[lens + at.spans] !== 0;
	const scoped = words[lens + at.scopes] !== 0;
	const config = (link ? LINK : 0) | (lines ? LINES : 0) | (scoped ? FACTS : 0) | (erase ? ERASE : 0);
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
	S.names = names(G, tree, words[lens + at.name_starts] - 1, moved);
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
		const rows = C.rows ?? tables(C);
		S.scopes = table(S, tree, words, lens, rows.scopes, at.scopes);
		S.bindings = table(S, tree, words, lens, rows.bindings, at.bindings);
		S.references = table(S, tree, words, lens, rows.references, at.references);
		S.roots = table(S, tree, words, lens, rows.roots, at.roots);
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
