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
	none: { enum: number; bool: number; list: Tagged; str: Tagged };
	views: { js: string[]; ts?: string[] };
	recipes: { js: RawRecipes; ts?: RawRecipes; adds?: RawRecipes; extras?: RawRecipes };
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
	at: Record<string, number>;
	recipes: Op[][];
	ts: Op[][];
	adds: Op[][];
	extras: Op[];
	erased: Op[];
	/** The builders of each way to build: linked, with lines, with facts, erasing; the last asked for kept near. */
	sets: Map<number, Builders>;
	last: number;
	builders: Builders | undefined;
}

type Builder = (S: State, id: number, record: number) => Decoded;
interface Builders {
	/** By a node's tag. */
	js: Builder[];
	/** By an extension record's tag, the record's first word given. */
	ts: Builder[];
}

export interface Compiled {
	layout: Layout;
	words: number;
	kind: number;
	extension: number;
	host: number;
	identifier: number;
	name: number;
	/** An extras record with nothing set. */
	blank: Uint32Array;
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

function language(layout: Layout, views: string[], typescript: boolean): Language {
	const by = (recipes: RawRecipes, kinds: Kind[]) => kinds.map((kind) => resolve(recipes.find(([name]) => name === kind.name)?.[1] ?? [], kind.fields));
	const at: Record<string, number> = {};
	views.forEach((name, i) => (at[name] = 1 + i));
	const out: Language = { at, recipes: by(layout.recipes.js, layout.kinds), ts: [], adds: [], extras: [], erased: [], sets: new Map(), last: -1, builders: undefined };
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

/** The engine's layout and recipes, read once. */
export function compile(engine: { layout: () => string }): Compiled {
	let C = compiled.get(engine);
	if (C !== undefined) return C;
	const layout: Layout = JSON.parse(engine.layout());
	const tag = (name: string) => layout.kinds.findIndex((kind) => kind.name === name);
	const blank = new Uint32Array((layout.extras?.size ?? 0) >> 2);
	for (const field of layout.extras?.fields ?? []) {
		const none = field.ty === '?enum' ? layout.none.enum : field.ty === '?bool' ? layout.none.bool : 0;
		blank[field.at >> 2] |= none << ((field.at & 3) << 3);
	}
	C = {
		layout,
		words: layout.node.size >> 2,
		kind: layout.node.kind,
		extension: tag('Extension'),
		host: tag('Host'),
		identifier: tag('Identifier'),
		name: layout.kinds[tag('Identifier')].fields[0].at,
		blank,
		js: language(layout, layout.views.js, false),
		ts: layout.views.ts === undefined ? undefined : language(layout, layout.views.ts, true),
	};
	compiled.set(engine, C);
	return C;
}

/** What the answer's words add to the tree. */
export interface Context {
	source: string;
	/** The engine's constant names, which a host's types and keys are numbers into. */
	constants: string[];
	scopes: Decoded[];
	bindings: Decoded[];
	references: Decoded[];
	roots: Decoded[];
}

interface State extends Context {
	link: boolean;
	erase: boolean;
	/** Lines and columns are on: every node and comment has a `loc`. */
	lines: boolean;
	C: Compiled;
	G: Language;
	/** The builder of each kind, by a node's tag. */
	J: Builder[];
	N: Uint32Array;
	L: Uint32Array;
	numbers: Float64Array;
	strings: string[];
	// a node's span: `P[id * ps + po]` and the word after, the nodes themselves when the source is ASCII
	P: Uint32Array;
	ps: number;
	po: number;
	locs: Uint32Array;
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
	/** Each root, in order. */
	nodes: Decoded[];
}

/** A tree built: its roots, and what `comments` and `kept` read. */
export type Built = State;

// a leading U+FEFF is text, not a mark
const utf8 = new TextDecoder('utf-8', { ignoreBOM: true });
const NO_WORDS = new Uint32Array(0);
const NONE_ADOPTED: number[] = [];

/** The tree's interned strings, by their number. */
export function strings(C: Compiled, tree: Tree, words: Uint32Array, lens: number): string[] {
	const at = (tree[0] === 1 ? C.ts! : C.js).at;
	const count = words[lens + at.starts] - 1;
	const text = utf8.decode((tree[at.text] as Uint8Array).subarray(0, words[lens + at.text]));
	const ends = words[lens + at.units] !== 0 ? (tree[at.units] as Uint32Array) : (tree[at.starts] as Uint32Array);
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
		const slot = S.extras_slots === null ? 0 : S.extras_slots[id];
		const [record, base] = slot > 0 ? [S.extras, (slot - 1) * S.C.layout.extras!.size] : [S.C.blank, 0];
		if (S.erase) apply(S, id, n, S.G.erased, record, base);
		else {
			apply(S, id, n, S.G.adds[S.N[id * S.C.words + (S.C.kind >> 2)]], record, base);
			apply(S, id, n, S.G.extras, record, base);
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
				if (op.ty === '?str') {
					const value = tagged(view, at, none.str);
					n[op.key] = value === -1 ? null : S.strings[view[value]];
				} else child(S, n, op.key, view[at >> 2]);
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
	const ty = S.hosts[index * 4], from = S.hosts[index * 4 + 1], len = S.hosts[index * 4 + 2], span = S.hosts[index * 4 + 3];
	let n: Decoded;
	if (ty === 0xffffffff) {
		// an object of the host's without a type, positions and all
		const at = id * S.ps + S.po;
		n = { start: S.P[at], end: S.P[at + 1] };
		if (S.lines) n.loc = loc(S.locs, id * 4);
	} else if (span === 1) n = begin(S, S.constants[ty], id);
	else {
		n = S.link ? { type: S.constants[ty], [PARENT]: undefined, [SCOPE]: undefined } : { type: S.constants[ty] };
		if (S.of_node !== null && !S.name_only) facts(S, n, id);
	}
	const pending = S.pending;
	S.pending = null;
	for (let i = from; i < from + len; i++) {
		const key = S.constants[S.host_keys[i]];
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
			case 4: {
				const out = new Array<string>(b);
				for (let j = 0; j < b; j++) out[j] = S.strings[S.host_strings[a + j]];
				n[key] = out;
				break;
			}
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
		else if (S.erased === null || !bit(S.erased, word - 1)) out.push(build(S, word - 1));
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
function generate(C: Compiled, G: Language, config: number, ops: Op[], ts: boolean, adds: boolean): Builder {
	const slow: Builder = ts ? (S, id, record) => run(S, id, ops, S.TS!, record * 4) : (S, id) => run(S, id, ops, S.N, id * C.words * 4 + C.kind);
	const link = (config & LINK) !== 0, facts = (config & FACTS) !== 0, erase = (config & ERASE) !== 0;
	// facts as keys, in the writer's order: only `run` spells them
	if ((facts && !link) || adds || ops.length === 0) return slow;
	const none = C.layout.none;
	const constants: unknown[] = [];
	const constant = (value: unknown) => `K[${constants.push(value) - 1}]`;
	const word = (at: number) => (ts ? `T[t + ${at >> 2}]` : `N[b + ${(C.kind + at) >> 2}]`);
	const byte = (at: number) => `(${word(at)} >>> ${(((ts ? 0 : C.kind) + at) & 3) << 3} & 255)`;
	const tagged = ({ tag }: Tagged, at: number) => [word(at + tag * 4), at + (tag === 0 ? 4 : 0)] as const;
	const lead: string[] = [];
	const links: string[] = [];
	// a child's builder is looked up where the child is read: each site sees its own few kinds
	const built = (id: string) => `J[N[${id} * ${C.words} + ${C.kind >> 2}]](S, ${id}, 0)`;
	const child = (local: string, at: number) => `const w${local} = ${word(at)}; const ${local} = w${local} === 0 ? null : ${built(`(w${local} - 1)`)};`;
	let at = 0;
	for (; ops[at].op === 'keep' || ops[at].op === 'through'; at++) {
		if (!erase) continue;
		if (ops[at].op === 'keep') lead.push(`S.kept.push(${JSON.stringify(ops[at].key)}, id);`);
		else lead.push(`S.adopted.push(id); return ${built(`(${word(ops[at].at)} - 1)`)};`);
	}
	const head = ops[at++];
	const type = head.op === 'type' ? JSON.stringify(head.key) : `${constant(head.names)}[${byte(head.at)}]`;
	const identifier = head.op === 'type' && head.key === 'Identifier';
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
				into.push(child(local, op.at));
				if (link) links.push(`if (${local} !== null && ${local}.type !== undefined) ${local}[PARENT] = n;`);
				return local;
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
	// what the literal has no room for: parentheses, comments, a TypeScript extra
	const rare = ['(S.parenthesized !== null && (S.parenthesized[id >>> 5] >>> (id & 31) & 1) === 1)', '(S.attached_slots !== null && S.attached_slots[id] > 0)', '(S.extras_slots !== null && S.extras_slots[id] > 0)'];
	const before: string[] = [];
	const after: string[] = [];
	if (link) {
		props.push('[PARENT]: undefined');
		if (!facts) props.push('[SCOPE]: undefined');
		else {
			rare.push('S.name_only', 'S.adopted.length !== 0');
			if (!identifier) rare.push('S.of_identifier[id] !== 0');
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
		if (facts) after.push('if (S.root_of[id] !== 0 || S.declared_by_at[id] !== 0 || S.writes_of_at[id] !== 0) late(S, n, id);');
	}
	const body = `const N = S.N, J = S.J, b = id * ${C.words}${ts ? ', T = S.TS' : ''}; ${lead.length !== 0 && lead[lead.length - 1].includes('return J[') ? lead.join(' ') : `if (${rare.join(' || ')}) return slow(S, id, t); ${before.join(' ')} const p = id * S.ps + S.po; ${lead.join(' ')} const n = { ${props.join(', ')} }; ${tail.join(' ')} ${links.join(' ')} ${after.join(' ')} return n;`}`;
	return new Function('K', 'slow', 'items', 'params', 'link_items', 'finite', 'bigint', 'late', 'PARENT', 'SCOPE', 'REFERENCE', `return (S, id, t) => { ${body} };`)(constants, slow, items, params, link_items, finite, bigint, late, PARENT, SCOPE, REFERENCE);
}

const generated = (() => {
	try {
		new Function('');
		return true;
	} catch {
		return false;
	}
})();

// each builder is made the first time its kind is met
function builders(C: Compiled, G: Language, config: number, typescript: boolean): Builders {
	let B = G.sets.get(config);
	G.last = config;
	if (B !== undefined) return (G.builders = B);
	const set: Builders = (B = { js: [], ts: [] });
	const lazy = (list: Builder[], tag: number, make: () => Builder): Builder => (S, id, record) => (list[tag] = make())(S, id, record);
	C.layout.kinds.forEach((_, tag) => {
		const ops = G.recipes[tag];
		if (tag === C.extension) set.js.push((S, id) => { const record = S.N[id * C.words + (C.kind >> 2) + 1] * (C.layout.ts!.size >> 2); return set.ts[S.TS![record]](S, id, record); });
		else if (tag === C.host) set.js.push((S, id) => host(S, id, S.N[id * C.words + (C.kind >> 2) + 1]));
		else set.js.push(lazy(set.js, tag, () => (generated ? generate(C, G, config, ops, false, typescript && (config & ERASE) === 0 && G.adds[tag].length !== 0) : (S, id) => run(S, id, ops, S.N, id * C.words * 4 + C.kind))));
	});
	G.ts.forEach((ops, tag) => set.ts.push(lazy(set.ts, tag, () => (generated ? generate(C, G, config, ops, true, false) : (S, id, record) => run(S, id, ops, S.TS!, record * 4)))));
	G.sets.set(config, set);
	return (G.builders = set);
}

/** Every comment read. */
export function comments(S: State): Decoded[] {
	const out = new Array<Decoded>(S.comment_count);
	for (let i = 0; i < S.comment_count; i++) out[i] = comment(S, i);
	return out;
}

const filled = (tree: Tree, words: Uint32Array, lens: number, at: number) => (words[lens + at] === 0 ? null : (tree[at] as Uint32Array).subarray(0, words[lens + at]));

/** What `words[lens]` says of an answer read in place; each view's length follows it. */
export const MOVED = 1, TYPESCRIPT = 2, COMMENTS = 4, ERASED = 8, LINED = 16, LISTED = 32;

/** The `roots` of the tree as ESTree in `nodes`, a list of them leaving out what erasure does; `interned` is what `strings` gave for the same tree. */
export function build_roots(C: Compiled, tree: Tree, words: Uint32Array, lens: number, roots: number[], interned: string[], context: Context, link: boolean): Built {
	const typescript = tree[0] === 1;
	const erase = (words[lens] & ERASED) !== 0, lines = (words[lens] & LINED) !== 0, listed = (words[lens] & LISTED) !== 0;
	const G = typescript ? C.ts! : C.js;
	const at = G.at;
	const spans = words[lens + at.spans] !== 0;
	const scoped = context.scopes.length !== 0;
	const N = tree[at.nodes] as Uint32Array;
	const config = (link ? LINK : 0) | (lines ? LINES : 0) | (scoped ? FACTS : 0) | (erase ? ERASE : 0);
	const B = G.last === config ? G.builders! : builders(C, G, config, typescript);
	// one state object per decode, young like everything it points at: no write barriers
	const S: Built = {
		source: context.source,
		constants: context.constants,
		link,
		erase,
		lines,
		scopes: context.scopes,
		bindings: context.bindings,
		references: context.references,
		roots: context.roots,
		C,
		G,
		J: B.js,
		N,
		L: tree[at.lists] as Uint32Array,
		numbers: tree[at.numbers] as Float64Array,
		strings: interned,
		P: spans ? (tree[at.spans] as Uint32Array) : N,
		ps: spans ? 2 : C.words,
		po: spans ? 0 : C.layout.node.start >> 2,
		locs: tree[at.locs] as Uint32Array,
		parenthesized: filled(tree, words, lens, at.parenthesized),
		erased: erase ? filled(tree, words, lens, at.erased) : null,
		comments: tree[at.comments] as Uint32Array,
		comment_count: words[lens + at.comments] / 9,
		attached_slots: filled(tree, words, lens, at.attached_slots),
		attached: tree[at.attached] as Uint32Array,
		hosts: tree[at.hosts] as Uint32Array,
		host_keys: tree[at.host_keys] as Uint32Array,
		host_vals: tree[at.host_vals] as Uint32Array,
		host_strings: tree[at.host_strings] as Uint32Array,
		of_node: scoped ? (tree[at.of_node] as Uint32Array) : null,
		of_identifier: scoped ? (tree[at.of_identifier] as Uint32Array) : NO_WORDS,
		root_of: scoped ? (tree[at.root_of] as Uint32Array) : NO_WORDS,
		declared_by: scoped ? (tree[at.declared_by] as Uint32Array) : NO_WORDS,
		declared_by_at: scoped ? (tree[at.declared_by_at] as Uint32Array) : NO_WORDS,
		writes_of: scoped ? (tree[at.writes_of] as Uint32Array) : NO_WORDS,
		writes_of_at: scoped ? (tree[at.writes_of_at] as Uint32Array) : NO_WORDS,
		TS: typescript ? (tree[at.ts] as Uint32Array) : null,
		extras_slots: typescript ? filled(tree, words, lens, at.extras_slots) : null,
		extras: typescript ? (tree[at.extras] as Uint32Array) : NO_WORDS,
		name_only: false,
		adopted: [],
		kept: [],
		pending: null,
		nodes: [],
	};
	for (const root of roots) if (!listed || S.erased === null || !bit(S.erased, root)) S.nodes.push(build(S, root));
	return S;
}

/** What erasure left in place, in source order. */
export function kept(S: Built): Decoded[] {
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
