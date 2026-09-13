// Authoring a host plan. The functions build plain data that `Plan::read` in Rust validates and
// runs; the generics infer field names, reader kinds and region ids from that data so a misuse is
// a compile error. `plan.js` and `plan.d.ts` are emitted from this file, never edited.

declare const kind: unique symbol;
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
/** Writes a declare may name: pattern reads, and emitted values, whose shape Rust checks at load. */
export type Bindable = 'pattern' | 'bindingIdentifier' | 'params' | 'value';
export type Mode = 'normal' | 'raw' | 'rcdata' | 'verbatim';
export type Stop = { prefixes: string[] } | { matchingElement: true } | { documentEnd: true };
export type Reader =
	| { kind: 'token'; text: string; gap: 'space*' | 'none'; word: boolean }
	| { kind: 'space'; min: number }
	| { kind: 'test'; value: V }
	| { kind: 'rule'; name: string }
	| { kind: 'javascript'; entry: JS; boundary?: 'last-shared-word' }
	| { kind: 'html-single'; entry: JS }
	| { kind: 'html-attributes'; mode: 'normal' | 'static' }
	| { kind: 'html-attribute-parts' }
	| { kind: 'html-children'; mode: Mode; stop: Stop }
	| { kind: 'css-stylesheet' };

/** A value expression. Its runtime shape is data; T is the phantom result type. */
export interface V<out T = unknown> {
	readonly [out]?: T;
}
/** A value expression that is not a slot of some rule: slots carry a name. */
export type Expr = V & { readonly name?: never };
/** A field or local of the rule being built; `f.context` is a `Slot<'context'>`. */
export interface Slot<out N extends string> extends V {
	readonly name: N;
}
/** `null`: an unwritten field is null. `omit`: an unwritten field is absent. */
export type Schema = Record<string, 'null' | 'omit'>;
export type Slots<S extends Schema, L extends readonly string[]> = {
	readonly [N in (keyof S & string) | L[number]]: Slot<N>;
};

/** A form. F is the names it may write; W is what it writes, as a union of `Write`s. */
export interface Form<out F extends string = string, out W = never> {
	readonly [kind]?: [F, W];
}
export type Write<N extends string, K extends string> = { into: N; kind: K };
export type FieldsOf<F> = F extends Form<infer N, unknown> ? N : never;
export type WritesOf<F> = F extends Form<string, infer W> ? W : never;
export type Writes<N extends string, K extends string> = [N] extends [never] ? never : Write<N, K>;
export type ReaderKind<R extends Reader> = R extends { kind: 'javascript' | 'html-single'; entry: infer E } ? E : R['kind'];
export type BindableNames<W> = W extends Write<infer N, infer K> ? (K extends Bindable ? N : never) : never;

export interface Region<out Id extends string, out F extends string> {
	readonly id: Id;
	readonly parent: V;
	readonly kind: 'module' | 'script' | 'fragment' | 'block' | 'function';
	readonly covers: V;
	readonly when?: V;
	readonly each?: { list: V; as: string };
	readonly [out]?: F;
}
export type Ids<R extends readonly Region<string, string>[]> = R[number]['id'];
export interface Declare<out N extends string, out R extends string | Expr> {
	readonly patterns: V;
	readonly into: V;
	readonly kind: 'pattern' | 'param';
	readonly [out]?: [N, R];
}

/** The data a rule serializes to; what `Plan::read` in Rust receives. */
export interface RuleData {
	readonly type: string;
	readonly fields: Schema;
	readonly locals?: readonly string[];
	form: Form<string, unknown>;
	regions?: readonly Region<string, string>[];
	declares?: readonly Declare<string, string | Expr>[];
	span?: 'none' | 'through-next-token-start';
}
export type Dispatch = { when: V; rule: string; type?: string; attributes?: 'static'; content?: Mode };
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
const slot = <N extends string>(base: string, name: N): Slot<N> => {
	const value = { op: 'get', base, path: [name] };
	Object.defineProperty(value, 'name', { value: name });
	return data<Slot<N>>(value);
};
const named = (value: unknown) =>
	typeof value === 'object' && value !== null && 'name' in value ? (value as { name: string }).name : value;
const list = (value: unknown) => (Array.isArray(value) ? array(...value) : value);

export const constant = <const T extends JSONValue>(value: T): V<T> => data({ op: 'constant', value });
export const get = (base: string | V, ...path: (string | number)[]): V => data({ op: 'get', base, path });
export const equal = (left: V, right: V): V<boolean> => data({ op: 'compare', relation: 'equal', left, right });
export const less = (left: V, right: V): V<boolean> => data({ op: 'compare', relation: 'less', left, right });
export const present = (left: V): V<boolean> => data({ op: 'compare', relation: 'present', left });
export const choose = <A, B>(condition: V, yes: V<A>, no: V<B>): V<A | B> => data({ op: 'choose', condition, yes, no });
export const flatMap = (list: V, as: string, body: V): V<unknown[]> => data({ op: 'flatMap', list, as, body });
export const length = (list: V): V<number> => data({ op: 'length', list });
export const at = (list: V, index: number): V => data({ op: 'at', list, index: constant(index) });
export const array = (...items: V[]): V<unknown[]> => data({ op: 'construct', shape: 'array', items });
export const record = (type: string | null, fields: Record<string, V>, span: V = constant(null)): V =>
	data({ op: 'construct', shape: 'record', type, fields, span });
export const not = (p: V): V<boolean> => data(choose(p, constant(false), constant(true)));
export const and = (a: V, b: V): V<boolean> => data(choose(a, b, constant(false)));
export const or = (a: V, b: V): V<boolean> => data(choose(a, constant(true), b));
export const filter = (list: V, as: string, predicate: V): V<unknown[]> =>
	flatMap(list, as, choose(predicate, array(get(as)), array()));
export const map = (list: V, as: string, value: V): V<unknown[]> => flatMap(list, as, array(value));
export const any = (list: V): V<boolean> => less(constant(0), length(list));
export const member = (value: V, literals: JSONValue[]): V<boolean> =>
	any(filter(constant(literals), '$candidate', equal(get('$candidate'), value)));
export const concat = (...lists: V[]): V<unknown[]> => flatMap(array(...lists), '$list', get('$list'));
export const scope = (name: string): V => get('scopes', name);
export const incoming: V = get('incoming');
export const isType = (value: V, ...types: string[]): V<boolean> => member(get(value, 'type'), types);
export const attributes = (node: V): V<unknown[]> =>
	data(choose(present(get(node, 'header', 'attributes')), get(node, 'header', 'attributes'), array()));
export const attr = (node: V, name: string): V<unknown[]> =>
	filter(
		attributes(node),
		'$attribute',
		and(equal(get('$attribute', 'kind'), constant('ordinary')), equal(get('$attribute', 'name'), constant(name))),
	);
export const hasAttribute = (node: V, name: string): V<boolean> => any(attr(node, name));
export const staticAttribute = (node: V, name: string): V => get(at(attr(node, name), 0), 'staticText');

export const seq = <Fs extends readonly Form<string, unknown>[]>(
	...items: Fs
): Form<FieldsOf<Fs[number]>, WritesOf<Fs[number]>> => data({ op: 'seq', items });
export const choice = <Fs extends readonly Form<string, unknown>[]>(
	...alternatives: Fs
): Form<FieldsOf<Fs[number]>, WritesOf<Fs[number]>> => data({ op: 'choice', alternatives });
export const optional = <F extends Form<string, unknown>>(form: F): F => data(choice(form, seq()));
export const read = <R extends Reader, N extends string = never>(
	reader: R,
	into?: Slot<N>,
	input?: V,
): Form<N, Writes<N, ReaderKind<R>>> =>
	data({ op: 'read', reader, ...(into ? { into: named(into) } : {}), ...(input ? { input } : {}) });
export const emit = <N extends string>(into: Slot<N>, value: V): Form<N, Write<N, 'value'>> =>
	data({ op: 'emit', into: named(into), value });
export const token = (text: string, tight = false): Form<never, never> =>
	read({ kind: 'token', text, gap: tight ? 'none' : 'space*', word: /[A-Za-z0-9_]$/.test(text) });
export const space = (): Form<never, never> => read({ kind: 'space', min: 1 });
export const test = (value: V): Form<never, never> => read({ kind: 'test', value });
export const call = <N extends string = never>(name: string, into?: Slot<N>): Form<N, Writes<N, 'rule'>> =>
	read({ kind: 'rule', name }, into);
export const js = <E extends JS, N extends string = never>(
	entry: E,
	into?: Slot<N>,
	input?: V,
	boundary?: 'last-shared-word',
): Form<N, Writes<N, E>> => read({ kind: 'javascript', entry, ...(boundary ? { boundary } : {}) }, into, input);
const item = slot('iteration', 'item');
export const repeat = <N extends string>(
	build: (r: { readonly item: Slot<'item'> }) => Form<string, unknown>,
	into: Slot<N>,
	value: V = item,
	min = 0,
	max: number | null = null,
): Form<N, Write<N, 'repeat'>> =>
	data({ op: 'repeat', body: build({ item }), min, max, locals: ['item'], yield: value, into: named(into) });
export const many = <N extends string>(name: string, into: Slot<N>, min: number, max: number | null) =>
	repeat((r) => call(name, r.item), into, item, min, max);

export const region = <const Id extends string, F extends string>(
	id: Id,
	parent: V,
	covers: readonly (Slot<F> | Expr)[] | Expr,
	kind: Region<Id, F>['kind'] = 'fragment',
	when?: V,
): Region<Id, F> => data({ id, parent, kind, covers: list(covers), ...(when ? { when } : {}) });
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

/** A rule under construction. Each step fixes one type parameter from the data it is given, so
 *  the next step is checked against it: fields, then the form's writes, then region ids. */
export class Rule<Type extends string, S extends Schema, L extends readonly string[], W, R extends string> {
	declare readonly [out]?: { type: Type; fields: S; writes: W };
	readonly #data: RuleData;
	readonly #slots: Slots<S, L>;
	constructor(type: Type, fields: S, locals: L) {
		this.#data = { type, fields, ...(locals.length ? { locals: [...locals] } : {}), form: seq() };
		const slots: Record<string, Slot<string>> = Object.fromEntries(
			Object.keys(fields).map((name) => [name, slot('record', name)]),
		);
		for (const name of locals) slots[name] = slot('locals', name);
		this.#slots = Object.freeze(slots) as Slots<S, L>;
	}
	form<W2>(build: (f: Slots<S, L>) => Form<(keyof S & string) | L[number], W2>): Rule<Type, S, L, W2, R> {
		this.#data.form = build(this.#slots);
		return this as unknown as Rule<Type, S, L, W2, R>;
	}
	regions<const Rs extends readonly Region<string, (keyof S & string) | L[number]>[]>(
		build: (f: Slots<S, L>) => Rs,
	): Rule<Type, S, L, W, R | Ids<Rs>> {
		this.#data.regions = [...(this.#data.regions ?? []), ...build(this.#slots)];
		return this as unknown as Rule<Type, S, L, W, R | Ids<Rs>>;
	}
	declares(build: (f: Slots<S, L>) => readonly Declare<BindableNames<W>, R | 'incoming' | Expr>[]): this {
		this.#data.declares = [...(this.#data.declares ?? []), ...build(this.#slots)];
		return this;
	}
	span(policy: 'none' | 'through-next-token-start'): this {
		this.#data.span = policy;
		return this;
	}
	toJSON(): RuleData {
		return this.#data;
	}
}
export const rule = <const Type extends string, const S extends Schema, const L extends readonly string[] = []>(
	type: Type,
	fields: S,
	locals: L = [] as unknown as L,
) => new Rule<Type, S, L, never, never>(type, fields, locals);

// The node type a rule produces: `null` fields are required and nullable, `omit` fields optional.
export type NodeOf<K> = K extends 'expression' | 'assignmentExpression'
	? import('estree').Expression
	: K extends 'pattern'
		? import('estree').Pattern
		: K extends 'bindingIdentifier' | 'identifierReference'
			? import('estree').Identifier
			: K extends 'params'
				? import('estree').Pattern[]
				: K extends 'statement'
					? import('estree').Statement
					: K extends 'program'
						? import('estree').Program
						: unknown;
export type KindOf<W, N> = W extends Write<infer M, infer K> ? (M extends N ? K : never) : never;
export type Build<Type, S extends Schema, W> = { type: Type; start: number; end: number } & {
	[N in keyof S as S[N] extends 'null' ? N : never]: NodeOf<KindOf<W, N>> | null;
} & { [N in keyof S as S[N] extends 'omit' ? N : never]?: NodeOf<KindOf<W, N>> };
export type Infer<Rl extends { readonly [out]?: { type: string; fields: Schema; writes: unknown } }> = Build<
	NonNullable<Rl[typeof out]>['type'],
	NonNullable<Rl[typeof out]>['fields'],
	NonNullable<Rl[typeof out]>['writes']
>;
