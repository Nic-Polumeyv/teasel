// Builds ESTree objects from the parser's own tree, read in place: the layout says where each
// kind's fields sit, the recipes how the kind is spelled.

// symbol keys: ten times cheaper than a WeakMap entry, and skipped by JSON, Object.keys and for-in
export const SCOPE = Symbol('scope');
export const REFERENCE = Symbol('reference');
export const PARENT = Symbol('parent');

/** A decoded object: the tree decides its shape, `api.ts` describes it. */
export type Decoded = Record<string | symbol, any>;

type View = Uint32Array | Float64Array | Uint8Array;
/** Whether the tree is the TypeScript one, then each view of the layout's `views` followed by its length in elements; `undefined` for a table the parse did not fill. */
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
	extras?: { size: number; none: number; fields: Field[] };
	none: { enum: number; bool: number; list: Tagged; str: Tagged };
	views: { js: [string, string][]; ts?: [string, string][] };
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

function language(layout: Layout, views: [string, string][], typescript: boolean): Language {
	const by = (recipes: RawRecipes, kinds: Kind[]) => kinds.map((kind) => resolve(recipes.find(([name]) => name === kind.name)?.[1] ?? [], kind.fields));
	const at: Record<string, number> = {};
	views.forEach(([name], i) => (at[name] = 1 + 2 * i));
	const out: Language = { at, recipes: by(layout.recipes.js, layout.kinds), ts: [], adds: [], extras: [], erased: [] };
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

function compile(engine: { layout: () => string }): Compiled {
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

/** What the answer's words and the caller add to the tree. */
export interface Context {
	source: string;
	/** The engine's constant names, which a host's types and keys are numbers into. */
	constants: string[];
	link: boolean;
	erase: boolean;
	/** Lines and columns are on: every node and comment has a `loc`. */
	lines: boolean;
	scopes: Decoded[];
	bindings: Decoded[];
	references: Decoded[];
	roots: Decoded[];
}

interface State extends Context {
	C: Compiled;
	G: Language;
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
	/** The bindings the node being begun declares, linked once its keys are in. */
	pending: number[] | null;
}

// a leading U+FEFF is text, not a mark
const utf8 = new TextDecoder('utf-8', { ignoreBOM: true });
const NO_WORDS = new Uint32Array(0);

/** The tree's interned strings, by their number. */
export function strings(tree: Tree, engine: { layout: () => string }): string[] {
	const C = compile(engine);
	const at = (tree[0] === 1 ? C.ts! : C.js).at;
	const bytes = tree[at.text + 1] as number;
	const count = (tree[at.starts + 1] as number) - 1;
	const text = utf8.decode((tree[at.text] as Uint8Array).subarray(0, bytes));
	const ends = (tree[at.units + 1] as number) !== 0 ? (tree[at.units] as Uint32Array) : (tree[at.starts] as Uint32Array);
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
	for (let i = 0; i <= adopted.length; i++) {
		const node = i < adopted.length ? adopted[i] : id;
		const root = S.root_of[node];
		if (root !== 0) {
			if (S.link) S.roots[root - 1].node = n;
			else n.root = root - 1;
		}
		const declared = S.declared_by_at[node];
		if (declared !== 0) {
			const ids = Array.from(S.declared_by.subarray(declared, declared + S.declared_by[declared - 1]));
			if (S.link) S.pending = S.pending === null ? ids : S.pending.concat(ids);
			else n.defines = ids;
		}
		const written = S.writes_of_at[node];
		if (written !== 0) {
			const ids = S.writes_of.subarray(written, written + S.writes_of[written - 1]);
			if (S.link) for (let j = 0; j < ids.length; j++) S.references[ids[j]].writeExpr = n;
			else n.writes = Array.from(ids);
		}
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
		const slots = S.extras_slots;
		const slot = slots !== null && id < slots.length ? slots[id] : S.C.layout.extras!.none;
		const [record, base] = slot === S.C.layout.extras!.none ? [S.C.blank, 0] : [S.extras, slot * S.C.layout.extras!.size];
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

function list(S: State, n: Decoded, key: string, start: number, len: number) {
	const out: (Decoded | null)[] = [];
	const linked = S.link && n.type !== undefined;
	for (let i = start; i < start + len; i++) {
		const word = S.L[i];
		if (word === 0) out.push(null);
		else if (S.erased === null || !bit(S.erased, word - 1)) {
			const c = build(S, word - 1);
			if (linked) c[PARENT] = n;
			out.push(c);
		}
	}
	n[key] = out;
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
				list(S, n, op.key, view[at >> 2], view[(at >> 2) + 1]);
				break;
			case 'optlistkey': {
				const value = tagged(view, at, none.list);
				if (value !== -1) list(S, n, op.key, view[value], view[value + 1]);
				break;
			}
			case 'params': {
				let start = view[at >> 2], len = view[(at >> 2) + 1];
				// erasing drops TypeScript's `this` parameter
				if (S.erase && len > 0 && S.L[start] !== 0) {
					const first = (S.L[start] - 1) * S.C.words * 4 + S.C.kind;
					if (S.N[first >> 2] === S.C.identifier && S.strings[S.N[(first + S.C.name) >> 2]] === 'this') [start, len] = [start + 1, len - 1];
				}
				list(S, n, op.key, start, len);
				break;
			}
			case 'bool':
				n[op.key] = byte(view, at) === 1;
				break;
			case 'boolif':
				if (byte(view, at) === 1) n[op.key] = true;
				break;
			case 'boolifextension':
				if (byte(view, at) === 1 && S.N[id * S.C.words + (S.C.kind >> 2)] === S.C.extension) n[op.key] = true;
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

function settle(S: State, n: Decoded, pending: number[] | null) {
	if (pending === null) return;
	for (let i = 0; i < pending.length; i++) {
		const d = S.bindings[pending[i]];
		d.declaration = n;
		if (n.init !== undefined) d.writeExpr = n.init;
	}
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
	settle(S, n, pending);
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
				list(S, n, key, a, b);
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
	settle(S, n, pending);
	return n;
}

function build(S: State, id: number): Decoded {
	const at = id * S.C.words + (S.C.kind >> 2);
	const tag = S.N[at];
	if (tag === S.C.extension) {
		const base = S.N[at + 1] * S.C.layout.ts!.size;
		return run(S, id, S.G.ts[S.TS![base >> 2]], S.TS!, base);
	}
	if (tag === S.C.host) return host(S, id, S.N[at + 1]);
	return run(S, id, S.G.recipes[tag], S.N, at * 4);
}

function comments(S: State): Decoded[] {
	const out = new Array<Decoded>(S.comment_count);
	for (let i = 0; i < S.comment_count; i++) out[i] = comment(S, i);
	return out;
}

/** What the tree adds to an answer. */
export interface Built {
	/** Each root, in order. */
	nodes: Decoded[];
	/** Every comment read. */
	comments: () => Decoded[];
	/** What erasure left in place, in source order. */
	kept: () => Decoded[];
}

/** The `roots` of the tree as ESTree, a list of them leaving out what erasure does; `interned` is what `strings` gave for the same tree. */
export function build_roots(tree: Tree, engine: { layout: () => string }, roots: number[], listed: boolean, interned: string[], context: Context): Built {
	const C = compile(engine);
	const G = tree[0] === 1 ? C.ts! : C.js;
	const view = (name: string) => tree[G.at[name]] as Uint32Array | undefined;
	const filled = (name: string) => {
		const len = (tree[G.at[name] + 1] ?? 0) as number;
		return len === 0 ? null : (tree[G.at[name]] as Uint32Array).subarray(0, len);
	};
	const spans = (tree[G.at.spans + 1] as number) !== 0;
	const N = view('nodes')!;
	const S: State = {
		...context,
		C,
		G,
		N,
		L: view('lists')!,
		numbers: tree[G.at.numbers] as Float64Array,
		strings: interned,
		P: spans ? view('spans')! : N,
		ps: spans ? 2 : C.words,
		po: spans ? 0 : C.layout.node.start >> 2,
		locs: view('locs')!,
		parenthesized: filled('parenthesized'),
		erased: context.erase ? filled('erased') : null,
		comments: view('comments')!,
		comment_count: (tree[G.at.comments + 1] as number) / 9,
		attached_slots: filled('attached_slots'),
		attached: view('attached')!,
		hosts: view('hosts')!,
		host_keys: view('host_keys')!,
		host_vals: view('host_vals')!,
		host_strings: view('host_strings')!,
		of_node: context.scopes.length === 0 ? null : view('of_node')!,
		of_identifier: view('of_identifier') ?? NO_WORDS,
		root_of: view('root_of') ?? NO_WORDS,
		declared_by: view('declared_by') ?? NO_WORDS,
		declared_by_at: view('declared_by_at') ?? NO_WORDS,
		writes_of: view('writes_of') ?? NO_WORDS,
		writes_of_at: view('writes_of_at') ?? NO_WORDS,
		TS: tree[0] === 1 ? view('ts')! : null,
		extras_slots: tree[0] === 1 ? filled('extras_slots') : null,
		extras: tree[0] === 1 ? view('extras')! : NO_WORDS,
		name_only: false,
		adopted: [],
		kept: [],
		pending: null,
	};
	const nodes: Decoded[] = [];
	for (const root of roots) if (!listed || S.erased === null || !bit(S.erased, root)) nodes.push(build(S, root));
	return {
		nodes,
		comments: () => comments(S),
		kept() {
			const out: Decoded[] = [];
			for (let i = 0; i < S.kept.length; i += 2) {
				const id = S.kept[i + 1] as number;
				const at = id * S.ps + S.po;
				const n: Decoded = { type: S.kept[i], start: S.P[at], end: S.P[at + 1] };
				if (S.lines) n.loc = loc(S.locs, id * 4);
				out.push(n);
			}
			return out.sort((a, b) => a.start - b.start);
		},
	};
}
