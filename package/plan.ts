// Authoring a host plan: data for `Plan::read` in Rust, typed so that a misuse fails to compile.
// `plan.js` and `plan.d.ts` are emitted from this file.

declare const out: unique symbol;

export type JSONValue = null | boolean | number | string | JSONValue[] | { [key: string]: JSONValue };
export type JS =
	| 'expression'
	| 'assignmentExpression'
	| 'pattern'
	| 'bindingIdentifier'
	| 'identifierReference'
	| 'params'
	| 'typeParameters'
	| 'statement'
	| 'program';
export type Mode = 'normal' | 'raw' | 'rcdata' | 'verbatim';
export type Stop = { prefixes: string[] } | { matchingElement: true } | { documentEnd: true };
export type Reader =
	| { kind: 'token'; text: string; gap: 'space*' | 'none'; word: boolean }
	| { kind: 'space'; min: number }
	| { kind: 'test'; value: V<boolean> }
	| { kind: 'rule'; name: string }
	| { kind: 'javascript'; entry: JS; boundary?: 'last-shared-word' }
	| { kind: 'html-single'; entry: JS }
	| { kind: 'html-attributes'; mode: 'normal' | 'static' }
	| { kind: 'html-attribute-parts' }
	| { kind: 'html-children'; mode: Mode; stop: Stop }
	| { kind: 'css-stylesheet' };

export interface V<out T = unknown> {
	readonly [out]: T;
}
export type Expr = V & { readonly name?: never };
export interface Slot<out N extends string> extends V<any> {
	readonly name: N;
}
export type Schema = Record<string, 'null' | 'omit'>;
type Names<S extends Schema, L extends readonly string[]> = (keyof S & string) | L[number];
type Slots<S extends Schema, L extends readonly string[]> = { readonly [N in Names<S, L>]: Slot<N> };

export interface Form<out W = unknown> {
	readonly [out]: W;
}
export type Write<N extends string, K extends string> = { into: N; kind: K };
type Wrote<Fs extends readonly Form[]> = Form<Fs[number][typeof out]>;
type Kind<R extends Reader> = R extends { kind: 'javascript' | 'html-single'; entry: infer E } ? E : R['kind'];
// every write of the field must bind: emitted values count, since Vue's v-for declares fields it copied
// out of a sub-rule and Rust checks their shape
type Written<W> = W extends Write<infer N, string> ? N : never;
type Bindable<W> = BindableOf<W, Written<W>>;
type BindableOf<W, N> = N extends string ? (Kinds<W, N> extends 'pattern' | 'bindingIdentifier' | 'params' | 'value' ? N : never) : never;

export interface Region<out Id extends string, out F extends string> {
	readonly id: Id;
	readonly parent: V;
	readonly kind: 'module' | 'script' | 'fragment' | 'block' | 'function';
	readonly covers: V;
	readonly when?: V<boolean>;
	readonly each?: { list: V; as: string };
	readonly [out]: F;
}
export interface Declare<out N extends string, out R extends string | Expr> {
	readonly patterns: V;
	readonly into: V;
	readonly kind: 'pattern' | 'param';
	readonly [out]: [N, R];
}
export interface RuleData {
	readonly type: string;
	readonly fields: Schema;
	readonly locals?: readonly string[];
	form: Form;
	regions?: readonly Region<string, string>[];
	declares?: readonly Declare<string, string | Expr>[];
	span?: 'none' | 'through-next-token-start';
}
export type Dispatch = { when: V<boolean>; rule: string; type?: string; attributes?: 'static'; content?: Mode };
export type Plan = {
	version: 1;
	document: string;
	rules: Record<string, { toJSON(): RuleData }>;
	html: {
		delimiters: [string, string];
		attributeInterpolations: boolean;
		attributeComments: 'javascript' | 'none';
		autoclose: boolean;
		trimEnd: boolean;
		void: string[];
		text: string;
		comment: string;
		content: { prefix: string; rule: string }[];
		attribute: { prefix: string; rule: string }[];
		plainAttribute: { type: string; name: string; value: string; text: string; expression: string };
		elements: Dispatch[];
		directiveNames: {
			prefix: string;
			argument: string;
			modifier: string;
			requireArgument: boolean;
			dynamic: [string, string] | null;
			unknown: 'plain-attribute' | 'wildcard-rule';
		};
		directives: { name: string; rule: string }[];
	};
};

const data = <T>(value: object) => value as unknown as T;
// the name rides along for `into`, hidden from JSON because Rust's reader knows only op/base/path
const slot = <N extends string>(base: string, name: N) =>
	data<Slot<N>>(Object.defineProperty({ op: 'get', base, path: [name] }, 'name', { value: name }));
const list = (value: readonly V[] | V): V => (Array.isArray(value) ? array(...value) : (value as V));

export const constant = <const T extends JSONValue>(value: T): V<T> => data({ op: 'constant', value });
export const get = (base: string | V, ...path: (string | number)[]): V<any> => data({ op: 'get', base, path });
export const equal = (left: V, right: V): V<boolean> => data({ op: 'compare', relation: 'equal', left, right });
export const less = (left: V, right: V): V<boolean> => data({ op: 'compare', relation: 'less', left, right });
export const present = (left: V): V<boolean> => data({ op: 'compare', relation: 'present', left });
export const choose = <A, B>(condition: V<boolean>, yes: V<A>, no: V<B>): V<A | B> =>
	data({ op: 'choose', condition, yes, no });
export const flatMap = (list: V, as: string, body: V): V<unknown[]> => data({ op: 'flatMap', list, as, body });
export const length = (list: V): V<number> => data({ op: 'length', list });
export const at = (list: V, index: number): V<any> => data({ op: 'at', list, index: constant(index) });
export const array = (...items: V[]): V<unknown[]> => data({ op: 'construct', shape: 'array', items });
export const record = (type: string | null, fields: Record<string, V>, span: V = constant(null)): V =>
	data({ op: 'construct', shape: 'record', type, fields, span });
export const not = (p: V<boolean>) => choose(p, constant(false), constant(true));
export const and = (a: V<boolean>, b: V<boolean>) => choose(a, b, constant(false));
export const or = (a: V<boolean>, b: V<boolean>) => choose(a, constant(true), b);
export const filter = (list: V, as: string, predicate: V<boolean>) =>
	flatMap(list, as, choose(predicate, array(get(as)), array()));
export const map = (list: V, as: string, value: V) => flatMap(list, as, array(value));
export const any = (list: V) => less(constant(0), length(list));
// helper bindings start with `$$`, a prefix an author's own aliases must not use
export const member = (value: V, literals: JSONValue[]) =>
	any(filter(constant(literals), '$$candidate', equal(get('$$candidate'), value)));
export const concat = (...lists: V[]) => flatMap(array(...lists), '$$list', get('$$list'));
export const scope = (name: string) => get('scopes', name);
export const incoming = get('incoming');
export const isType = (value: V, ...types: string[]) => member(get(value, 'type'), types);
export const attributes = (node: V): V<unknown[]> =>
	choose(present(get(node, 'header', 'attributes')), get(node, 'header', 'attributes'), array());
export const attr = (node: V, name: string) =>
	filter(
		attributes(node),
		'$$attribute',
		and(equal(get('$$attribute', 'kind'), constant('ordinary')), equal(get('$$attribute', 'name'), constant(name))),
	);
export const hasAttribute = (node: V, name: string) => any(attr(node, name));
export const staticAttribute = (node: V, name: string) => get(at(attr(node, name), 0), 'staticText');

export const seq = <Fs extends readonly Form[]>(...items: Fs): Wrote<Fs> => data({ op: 'seq', items });
export const choice = <Fs extends readonly Form[]>(...alternatives: Fs): Wrote<Fs> =>
	data({ op: 'choice', alternatives });
export const optional = <F extends Form>(form: F): F => data(choice(form, seq()));
export const read = <R extends Reader, N extends string = never>(
	reader: R,
	into?: Slot<N>,
	input?: V,
): Form<Write<N, Kind<R>>> =>
	data({ op: 'read', reader, ...(into && { into: into.name }), ...(input && { input }) });
export const emit = <N extends string>(into: Slot<N>, value: V): Form<Write<N, 'value'>> =>
	data({ op: 'emit', into: into.name, value });
export const token = (text: string, tight = false) =>
	read({ kind: 'token', text, gap: tight ? 'none' : 'space*', word: /[A-Za-z0-9_]$/.test(text) });
export const space = () => read({ kind: 'space', min: 1 });
export const test = (value: V<boolean>) => read({ kind: 'test', value });
export const call = <N extends string = never>(name: string, into?: Slot<N>) => read({ kind: 'rule', name }, into);
export const js = <E extends JS, N extends string = never>(
	entry: E,
	into?: Slot<N>,
	input?: V,
	boundary?: 'last-shared-word',
) => read({ kind: 'javascript', entry, ...(boundary && { boundary }) }, into, input);
const item = slot('iteration', 'item');
type Repeated<B extends Form> = [Kinds<B[typeof out], 'item'>] extends [never] ? 'value' : Kinds<B[typeof out], 'item'>;
export const repeat = <N extends string, B extends Form>(
	build: (r: { readonly item: Slot<'item'> }) => B,
	into: Slot<N>,
	value: V = item,
	min = 0,
	max: number | null = null,
): Form<Write<N, Repeated<B>>> =>
	data({ op: 'repeat', body: build({ item }), min, max, locals: ['item'], yield: value, into: into.name });
export const many = <N extends string>(name: string, into: Slot<N>, min: number, max: number | null) =>
	repeat((r) => call(name, r.item), into, item, min, max);

export const region = <const Id extends string, F extends string>(
	id: Id,
	parent: V,
	covers: readonly (Slot<F> | Expr)[] | Expr,
	kind: Region<Id, F>['kind'] = 'fragment',
	when?: V<boolean>,
): Region<Id, F> => data({ id, parent, kind, covers: list(covers), ...(when && { when }) });
export const declare = <N extends string, const R extends string | Expr>(
	patterns: readonly Slot<N>[] | Expr,
	into: R,
	kind: 'pattern' | 'param' = 'pattern',
): Declare<N, R> =>
	data({
		patterns: list(patterns),
		into: typeof into === 'string' ? (into === 'incoming' ? incoming : scope(into)) : into,
		kind,
	});

// a chain, not one call with an options object: TypeScript fixes W before a deferred form callback runs
export class Rule<Type extends string, S extends Schema, L extends readonly string[], W, R extends string, P extends RuleData['span'] = undefined> {
	declare readonly [out]: { type: Type; fields: S; writes: W; span: P };
	readonly #data: RuleData;
	readonly #slots: Slots<S, L>;
	constructor(type: Type, fields: S, locals: L) {
		this.#data = { type, fields, ...(locals.length && { locals: [...locals] }), form: seq() };
		this.#slots = data(
			Object.fromEntries([
				...Object.keys(fields).map((name) => [name, slot('record', name)]),
				...locals.map((name) => [name, slot('locals', name)]),
			]),
		);
	}
	form<W2 extends Write<Names<S, L>, string>>(build: (f: Slots<S, L>) => Form<W2>): Rule<Type, S, L, W2, R, P> {
		this.#data.form = build(this.#slots);
		return data(this);
	}
	regions<const Rs extends readonly Region<string, Names<S, L>>[]>(
		build: (f: Slots<S, L>) => Rs,
	): Rule<Type, S, L, W, R | Rs[number]['id'], P> {
		this.#data.regions = [...(this.#data.regions ?? []), ...build(this.#slots)];
		return data(this);
	}
	declares(build: (f: Slots<S, L>) => readonly Declare<Bindable<W>, R | 'incoming' | Expr>[]): this {
		this.#data.declares = [...(this.#data.declares ?? []), ...build(this.#slots)];
		return this;
	}
	span<P2 extends NonNullable<RuleData['span']>>(policy: P2): Rule<Type, S, L, W, R, P2> {
		this.#data.span = policy;
		return data(this);
	}
	toJSON() {
		return this.#data;
	}
}
export const rule = <const Type extends string, const S extends Schema, const L extends readonly string[] = []>(
	type: Type,
	fields: S,
	locals?: L,
) => new Rule<Type, S, L, never, never>(type, fields, locals ?? data<L>([]));

interface Nodes {
	expression: import('estree').Expression;
	assignmentExpression: import('estree').Expression;
	pattern: import('estree').Pattern;
	bindingIdentifier: import('estree').Identifier;
	identifierReference: import('estree').Identifier;
	params: import('estree').Pattern[];
	statement: import('estree').Statement;
	program: import('estree').Program;
}
type Kinds<W, N> = W extends Write<infer M, infer K> ? ([M] extends [never] ? never : M extends N ? K : never) : never;
type Node<W, N> = Kinds<W, N> extends infer K ? (K extends keyof Nodes ? Nodes[K] : unknown) : never;
/** The node a rule produces: `null` fields are required and nullable, `omit` fields optional. */
export type Infer<Rl extends { readonly [out]: { type: string; fields: Schema; writes: unknown; span: RuleData['span'] } }> = {
	type: Rl[typeof out]['type'];
} & (Rl[typeof out]['span'] extends 'none' ? {} : { start: number; end: number }) & {
	[N in keyof Rl[typeof out]['fields'] as Rl[typeof out]['fields'][N] extends 'null' ? N : never]: Node<
		Rl[typeof out]['writes'],
		N
	> | null;
} & {
	[N in keyof Rl[typeof out]['fields'] as Rl[typeof out]['fields'][N] extends 'omit' ? N : never]?: Node<
		Rl[typeof out]['writes'],
		N
	>;
};
