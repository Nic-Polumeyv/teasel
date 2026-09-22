import type {
	Json as JSONValue, Js as JS, Reader, Boundary,
	Plan as WirePlan, Rule as RuleData, Value as WireValue, Form as WireForm,
	Region as WireRegion, Declare as WireDeclare, Dispatch as WireDispatch,
} from './lib/wire.js';
export type { Json as JSONValue, Js as JS, Mode, Stop, Reader, Rule as RuleData } from './lib/wire.js';

declare const out: unique symbol;

interface Inferred<out T> {
	readonly [out]: T;
}
export type V<T = unknown> = WireValue & Inferred<T>;
export type Expr = V & { readonly name?: never };
export type Slot<N extends string> = V<any> & { readonly name: N };
export type Schema = RuleData['fields'];
type Names<S extends Schema, L extends readonly string[]> = (keyof S & string) | L[number];
type Slots<S extends Schema, L extends readonly string[]> = { readonly [N in Names<S, L>]: Slot<N> };

export type Form<W = unknown> = WireForm & Inferred<W>;
export type Write<N extends string, K extends string> = { into: N; kind: K };
type Wrote<Fs extends readonly Form[]> = Form<Fs[number][typeof out]>;
type Kind<R extends Reader> = R extends { kind: 'javascript' | 'html-single'; entry: infer E } ? E : R['kind'];
type Written<W> = W extends Write<infer N, string> ? N : never;
type Bindable<W> = BindableOf<W, Written<W>>;
type BindableOf<W, N> = N extends string ? (Kinds<W, N> extends 'pattern' | 'bindingIdentifier' | 'params' | 'value' ? N : never) : never;

export interface Region<out Id extends string, out F extends string> extends WireRegion {
	readonly id: Id;
	readonly [out]: F;
}
export interface Declare<out N extends string, out R extends string | Expr> extends WireDeclare {
	readonly [out]: [N, R];
}
export type Dispatch = WireDispatch & { when: V<boolean> };
export type Plan = Omit<WirePlan, 'rules'> & { rules: Record<string, { toJSON(): RuleData }> };

const data = <T extends object>(value: object): T => value as T;
const valueData = <T>(value: WireValue): V<T> => data(value);
const formData = <W>(form: WireForm): Form<W> => data(form);
const slot = <N extends string>(base: string, name: N) =>
	data<Slot<N>>(Object.defineProperty({ op: 'get', base, path: [name] } satisfies WireValue, 'name', { value: name }));
const list = (value: readonly V[] | V): V => (Array.isArray(value) ? array(...value) : (value as V));

export const constant = <const T extends JSONValue>(value: T): V<T> => valueData({ op: 'constant', value });
export const get = (base: string | V, ...path: (string | number)[]): V<any> => valueData({ op: 'get', base, path });
export const equal = (left: V, right: V): V<boolean> => valueData({ op: 'compare', relation: 'equal', left, right });
export const less = (left: V, right: V): V<boolean> => valueData({ op: 'compare', relation: 'less', left, right });
export const present = (left: V): V<boolean> => valueData({ op: 'compare', relation: 'present', left });
export const choose = <A, B>(condition: V<boolean>, yes: V<A>, no: V<B>): V<A | B> =>
	valueData({ op: 'choose', condition, yes, no });
export const flatMap = (list: V, as: string, body: V): V<unknown[]> => valueData({ op: 'flatMap', list, as, body });
export const length = (list: V): V<number> => valueData({ op: 'length', list });
export const at = (list: V, index: number): V<any> => valueData({ op: 'at', list, index: constant(index) });
export const array = (...items: V[]): V<unknown[]> => valueData({ op: 'construct', shape: 'array', items });
export const record = (type: string | null, fields: Record<string, V>, span: V = constant(null)): V =>
	valueData({ op: 'construct', shape: 'record', type, fields, span });
export const not = (p: V<boolean>): V<boolean> => choose(p, constant(false), constant(true));
export const and = (a: V<boolean>, b: V<boolean>): V<boolean> => choose(a, b, constant(false));
export const or = (a: V<boolean>, b: V<boolean>): V<boolean> => choose(a, constant(true), b);
export const filter = (list: V, as: string, predicate: V<boolean>): V<unknown[]> =>
	flatMap(list, as, choose(predicate, array(get(as)), array()));
export const map = (list: V, as: string, value: V): V<unknown[]> => flatMap(list, as, array(value));
export const any = (list: V): V<boolean> => less(constant(0), length(list));
export const member = (value: V, literals: JSONValue[]): V<boolean> =>
	any(filter(constant(literals), '$$candidate', equal(get('$$candidate'), value)));
export const concat = (...lists: V[]): V<unknown[]> => flatMap(array(...lists), '$$list', get('$$list'));
export const scope = (name: string): V<any> => get('scopes', name);
export const incoming: V<any> = get('incoming');
export const isType = (value: V, ...types: string[]): V<boolean> => member(get(value, 'type'), types);
export const attributes = (node: V): V<unknown[]> =>
	choose(present(get(node, 'header', 'attributes')), get(node, 'header', 'attributes'), array());
export const attr = (node: V, name: string): V<unknown[]> =>
	filter(
		attributes(node),
		'$$attribute',
		and(equal(get('$$attribute', 'kind'), constant('ordinary')), equal(get('$$attribute', 'name'), constant(name))),
	);
export const hasAttribute = (node: V, name: string): V<boolean> => any(attr(node, name));
export const staticAttribute = (node: V, name: string): V<any> => get(at(attr(node, name), 0), 'staticText');

export const seq = <Fs extends readonly Form[]>(...items: Fs): Wrote<Fs> => formData({ op: 'seq', items });
export const choice = <Fs extends readonly Form[]>(...alternatives: Fs): Wrote<Fs> =>
	formData({ op: 'choice', alternatives });
export const optional = <F extends Form>(form: F): F => data(choice(form, seq()));
export const read = <R extends Reader, N extends string = never>(
	reader: R,
	into?: Slot<N>,
	input?: V,
): Form<Write<N, Kind<R>>> =>
	formData({ op: 'read', reader, ...(into && { into: into.name }), ...(input && { input }) });
export const emit = <N extends string>(into: Slot<N>, value: V): Form<Write<N, 'value'>> =>
	formData({ op: 'emit', into: into.name, value });
export const token = (text: string, tight = false): Form<Write<never, 'token'>> =>
	read({ kind: 'token', text, gap: tight ? 'none' : 'space*', word: /[A-Za-z0-9_]$/.test(text) });
export const space = (): Form<Write<never, 'space'>> => read({ kind: 'space', min: 1 });
export const test = (value: V<boolean>): Form<Write<never, 'test'>> => read({ kind: 'test', value });
export const call = <N extends string = never>(name: string, into?: Slot<N>): Form<Write<N, 'rule'>> => read({ kind: 'rule', name }, into);
export const js = <E extends JS, N extends string = never>(
	entry: E,
	into?: Slot<N>,
	input?: V,
	boundary?: Boundary,
): Form<Write<N, E>> => read({ kind: 'javascript', entry, ...(boundary && { boundary }) }, into, input);
const item = slot('iteration', 'item');
type Repeated<B extends Form> = [Kinds<B[typeof out], 'item'>] extends [never] ? 'value' : Kinds<B[typeof out], 'item'>;
export const repeat = <N extends string, B extends Form>(
	build: (r: { readonly item: Slot<'item'> }) => B,
	into: Slot<N>,
	value: V = item,
	min = 0,
	max: number | null = null,
): Form<Write<N, Repeated<B>>> =>
	formData({ op: 'repeat', body: build({ item }), min, max, locals: ['item'], yield: value, into: into.name });
export const many = <N extends string>(name: string, into: Slot<N>, min: number, max: number | null): Form<Write<N, 'rule'>> =>
	repeat((r) => call(name, r.item), into, item, min, max);

export const region = <const Id extends string, F extends string>(
	id: Id,
	parent: V,
	covers: readonly (Slot<F> | Expr)[] | Expr,
	kind: Region<Id, F>['kind'] = 'fragment',
	when?: V<boolean>,
): Region<Id, F> => data({ id, parent, kind, covers: list(covers), ...(when && { when }) } satisfies WireRegion);
export const declare = <N extends string, const R extends string | Expr>(
	patterns: readonly Slot<N>[] | Expr,
	into: R,
	kind: WireDeclare['kind'] = 'pattern',
): Declare<N, R> =>
	data({
		patterns: list(patterns),
		into: typeof into === 'string' ? (into === 'incoming' ? incoming : scope(into)) : into,
		kind,
	} satisfies WireDeclare);

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
	toJSON(): RuleData {
		return this.#data;
	}
}
export const rule = <const Type extends string, const S extends Schema, const L extends readonly string[] = []>(
	type: Type,
	fields: S,
	locals?: L,
): Rule<Type, S, L, never, never> => new Rule<Type, S, L, never, never>(type, fields, locals ?? data<L>([]));

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
