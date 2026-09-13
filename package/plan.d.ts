// The type program for a host plan. Every generic below is inferred from the data an author
// writes; the runtime in plan.js is data constructors and knows nothing of these types.

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
type FieldsOf<F> = F extends Form<infer N, unknown> ? N : never;
type WritesOf<F> = F extends Form<string, infer W> ? W : never;
type Writes<N extends string, K extends string> = [N] extends [never] ? never : Write<N, K>;
type ReaderKind<R extends Reader> = R extends { kind: 'javascript'; entry: infer E } ? E : R['kind'];
type BindableNames<W> = W extends Write<infer N, infer K> ? (K extends Bindable ? N : never) : never;

export interface Region<out Id extends string, out F extends string> {
	readonly id: Id;
	readonly parent: V;
	readonly kind: 'module' | 'script' | 'fragment' | 'block' | 'function';
	readonly covers: V;
	readonly when?: V;
	readonly each?: { list: V; as: string };
	readonly [out]?: F;
}
type Ids<R extends readonly Region<string, string>[]> = R[number]['id'];
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
	readonly form: Form;
	readonly regions?: readonly Region<string, string>[];
	readonly declares?: readonly Declare<string, string | Expr>[];
	readonly span?: 'none' | 'through-next-token-start';
}
/** A rule under construction. Each step fixes one type parameter from the data it is given, so
 *  the next step is checked against it: fields, then the form's writes, then region ids. */
export interface Fields<Type extends string, S extends Schema, L extends readonly string[]> {
	form<W>(build: (f: Slots<S, L>) => Form<(keyof S & string) | L[number], W>): Rule<Type, S, L, W, never>;
}
export interface Rule<Type extends string, S extends Schema, L extends readonly string[], W, R extends string> {
	regions<const Rs extends readonly Region<string, (keyof S & string) | L[number]>[]>(
		build: (f: Slots<S, L>) => Rs,
	): Rule<Type, S, L, W, R | Ids<Rs>>;
	declares(build: (f: Slots<S, L>) => readonly Declare<BindableNames<W>, R | 'incoming' | Expr>[]): this;
	span(policy: 'none' | 'through-next-token-start'): this;
	toJSON(): RuleData;
	readonly [out]?: { type: Type; fields: S; writes: W };
}
export declare function rule<const Type extends string, const S extends Schema, const L extends readonly string[] = []>(
	type: Type,
	fields: S,
	locals?: L,
): Fields<Type, S, L>;

export declare function seq<Fs extends readonly Form<string, unknown>[]>(
	...items: Fs
): Form<FieldsOf<Fs[number]>, WritesOf<Fs[number]>>;
export declare function choice<Fs extends readonly Form<string, unknown>[]>(
	...alternatives: Fs
): Form<FieldsOf<Fs[number]>, WritesOf<Fs[number]>>;
export declare function optional<F extends Form<string, unknown>>(form: F): F;
export declare function read<R extends Reader, N extends string = never>(
	reader: R,
	into?: Slot<N>,
	input?: V,
): Form<N, Writes<N, ReaderKind<R>>>;
export declare function emit<N extends string>(into: Slot<N>, value: V): Form<N, Write<N, 'value'>>;
export declare function token(text: string, tight?: boolean): Form<never, never>;
export declare function space(): Form<never, never>;
export declare function test(value: V): Form<never, never>;
export declare function call<N extends string = never>(name: string, into?: Slot<N>): Form<N, Writes<N, 'rule'>>;
export declare function js<E extends JS, N extends string = never>(
	entry: E,
	into?: Slot<N>,
	input?: V,
	boundary?: 'last-shared-word',
): Form<N, Writes<N, E>>;
export declare function repeat<N extends string>(
	build: (r: { readonly item: Slot<'item'> }) => Form<string, unknown>,
	into: Slot<N>,
	value?: V,
	min?: number,
	max?: number | null,
): Form<N, Write<N, 'repeat'>>;
export declare function many<N extends string>(
	name: string,
	into: Slot<N>,
	min: number,
	max: number | null,
): Form<N, Write<N, 'repeat'>>;

export declare function region<const Id extends string, F extends string>(
	id: Id,
	parent: V,
	covers: readonly (Slot<F> | Expr)[] | Expr,
	kind?: Region<Id, F>['kind'],
	when?: V,
): Region<Id, F>;
export declare function declare<N extends string, const R extends string | Expr>(
	patterns: readonly Slot<N>[] | Expr,
	into: R,
	kind?: 'pattern' | 'param',
): Declare<N, R>;

export declare function constant<const T extends JSONValue>(value: T): V<T>;
export declare function get(base: string | V, ...path: (string | number)[]): V;
export declare function equal(left: V, right: V): V<boolean>;
export declare function less(left: V, right: V): V<boolean>;
export declare function present(left: V): V<boolean>;
export declare function choose<A, B>(condition: V, yes: V<A>, no: V<B>): V<A | B>;
export declare function flatMap(list: V, as: string, body: V): V<unknown[]>;
export declare function length(list: V): V<number>;
export declare function at(list: V, index: number): V;
export declare function array(...items: V[]): V<unknown[]>;
export declare function record(type: string | null, fields: Record<string, V>, span?: V): V;
export declare function not(p: V): V<boolean>;
export declare function and(a: V, b: V): V<boolean>;
export declare function or(a: V, b: V): V<boolean>;
export declare function filter(list: V, as: string, predicate: V): V<unknown[]>;
export declare function map(list: V, as: string, value: V): V<unknown[]>;
export declare function any(list: V): V<boolean>;
export declare function member(value: V, literals: JSONValue[]): V<boolean>;
export declare function concat(...lists: V[]): V<unknown[]>;
export declare function scope(name: string): V;
export declare const incoming: V;
export declare function isType(value: V, ...types: string[]): V<boolean>;
export declare function attributes(node: V): V<unknown[]>;
export declare function attr(node: V, name: string): V<unknown[]>;
export declare function hasAttribute(node: V, name: string): V<boolean>;
export declare function staticAttribute(node: V, name: string): V;

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

// The node type a rule produces: `null` fields are required and nullable, `omit` fields optional.
type NodeOf<K> = K extends 'expression' | 'assignmentExpression'
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
type KindOf<W, N> = W extends Write<infer M, infer K> ? (M extends N ? K : never) : never;
type Build<Type, S extends Schema, W> = { type: Type; start: number; end: number } & {
	[N in keyof S as S[N] extends 'null' ? N : never]: NodeOf<KindOf<W, N>> | null;
} & { [N in keyof S as S[N] extends 'omit' ? N : never]?: NodeOf<KindOf<W, N>> };
export type Infer<Rl extends { readonly [out]?: { type: string; fields: Schema; writes: unknown } }> = Build<
	NonNullable<Rl[typeof out]>['type'],
	NonNullable<Rl[typeof out]>['fields'],
	NonNullable<Rl[typeof out]>['writes']
>;
