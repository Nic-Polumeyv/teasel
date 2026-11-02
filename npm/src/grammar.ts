import type { Expression, Identifier, Pattern, Program, Statement, VariableDeclaration } from 'estree';
import { LANGUAGE, type Comment, type HostNode, type Language, type Span } from './types.ts';
import { engine } from '#engine';
import { children as builtin, frozen } from './children.ts';

declare const out: unique symbol;

/** Where a source may be read: a form, a block's child list, a directive's attribute value, the document, a `this` attribute, a directive's name, a literal. */
export type Site = 'form' | 'body' | 'value' | 'document' | 'this' | 'directive' | 'literal';

interface Mods {
	optional: boolean;
	bind: false | 'inside' | 'outside';
	orArg: boolean;
	/** Unread whatever the syntax says: a document's script when the source has none. */
	missing: boolean;
}
type Plain = { optional: false; bind: false; orArg: false; missing: false };
type With<M extends Mods, K extends keyof Mods, V> = { [P in keyof Mods]: P extends K ? V : M[P] };

/** What fills a field: `T` is what a parse puts there, `S` where it may be read. */
export interface Source<T = unknown, S extends Site = Site, M extends Mods = Plain> {
	readonly from: string;
	readonly read: string;
	readonly optional: M['optional'];
	readonly bind: M['bind'];
	readonly orArg: M['orArg'];
	readonly [out]?: { readonly t: T; readonly m: M; readonly s: (site: S) => void };
}

declare const children: unique symbol;
declare const attributes: unique symbol;
declare const script: unique symbol;
declare const written: unique symbol;
/** A list of child nodes, which a `Fragment` holds as its `nodes`. */
export interface Children {
	readonly [children]: true;
}
interface Attributes {
	readonly [attributes]: true;
}
/** An attribute's value as written: true without one, else its text and expressions. */
interface Written {
	readonly [written]: true;
}
interface Script {
	readonly [script]: true;
}

const source = <T, S extends Site, M extends Mods = Plain>(from: string, read: string): Source<T, S, M> =>
	({ from, read, optional: false, bind: false, orArg: false }) as Source<T, S, M>;

/** JavaScript read at a position of a form. */
export const js = {
	expression: source<Expression, 'form'>('js', 'expression'),
	pattern: source<Pattern, 'form'>('js', 'pattern'),
	params: source<Pattern[], 'form'>('js', 'params'),
	identifier: source<Identifier, 'form'>('js', 'identifier'),
	identifiers: source<Identifier[], 'form'>('js', 'identifiers'),
	statement: source<Statement, 'form'>('js', 'statement'),
	const: source<VariableDeclaration, 'form'>('js', 'const'),
	/** An expression, or its statements as a program when it is not one. */
	code: source<Expression | Program, 'form'>('js', 'code'),
	/** The type parameters' text, without the angle brackets. */
	typeParameters: source<string, 'form'>('js', 'typeParameters'),
	/** The text up to the tag's end word, unread. */
	text: source<string, 'form'>('js', 'text'),
};

/** A directive's attribute value. */
export const value = {
	expression: source<Expression, 'value'>('value', 'expression'),
	pattern: source<Pattern, 'value'>('value', 'pattern'),
	/** The value as written: text and expressions. */
	raw: source<Written, 'value'>('value', 'value'),
};

/** What the document holds. */
export const doc = {
	script: source<Script, 'document', With<Plain, 'missing', true>>('doc', 'script'),
	module: source<Script, 'document', With<Plain, 'missing', true>>('doc', 'script:module'),
	style: source<HostNode, 'document', With<Plain, 'missing', true>>('doc', 'style'),
	comments: source<Comment[], 'document'>('doc', 'comments'),
};

/**
 * One of the child lists of a block that has several, under the name the block gives it. A block
 * with one list names none: it is the block's `fragment`, as the document's and every element's is.
 */
export const content = source<Children, 'body'>('content', 'fragment');

/** A value the site always writes. */
export const literal = <const T extends boolean | null | readonly []>(v: T): Source<T, 'literal'> =>
	({ ...source<T, 'literal'>('literal', 'literal'), literal: v }) as Source<T, 'literal'>;

/** The field is left out when nothing was read into it. The only way a field is absent. */
export const optional = <T, S extends Exclude<Site, 'value'>, M extends Mods>(s: Source<T, S, M>): Source<T, S, With<M, 'optional', true>> =>
	({ ...s, optional: true }) as Source<T, S, With<M, 'optional', true>>;

type Bindable = Pattern | Pattern[];
/** What the field reads declares names in the scope its site opens. */
export const bind = Object.assign(
	<T extends Bindable, S extends 'form' | 'value', M extends Mods>(s: Source<T, S, M>): Source<T, S, With<M, 'bind', 'inside'>> =>
		({ ...s, bind: 'inside' }) as Source<T, S, With<M, 'bind', 'inside'>>,
	{
		/** In the scope around the block instead: a snippet's name. */
		outside: <T extends Bindable, M extends Mods>(s: Source<T, 'form', M>): Source<T, 'form', With<M, 'bind', 'outside'>> =>
			({ ...s, bind: 'outside' }) as Source<T, 'form', With<M, 'bind', 'outside'>>,
	},
);

/** Without a value, the directive's argument is the value: `bind:value`. */
export const orArg = <T, M extends Mods>(s: Source<T, 'value', M>): Source<T, 'value', With<M, 'orArg', true>> =>
	({ ...s, orArg: true }) as Source<T, 'value', With<M, 'orArg', true>>;

type Reserved = 'type' | 'start' | 'end' | 'loc' | 'opt' | 'oneOf' | 'scope';
/** Fields a form reads, each named once, where its source is; `R` names what teasel names there. */
export type Fields<F = Source<any, any, any>, R extends string = never> = { readonly [field: string]: F } & { readonly [K in Reserved | R]?: never };

/** One step of a form: a host word or punctuator, fields, or a group. */
export type Item = string | Fields | Opt<readonly Item[]> | OneOf<readonly (readonly Item[])[]> | Scope<readonly Item[]>;
export interface Opt<I extends readonly unknown[]> {
	readonly opt: I;
}
export interface OneOf<A extends readonly (readonly unknown[])[]> {
	readonly oneOf: A;
}
/** The document's fields whose names the fields inside see. The document's `fragment` comes last, inside the scope its form ends in. */
export interface Scope<I extends readonly unknown[]> {
	readonly scope: I;
}

type Field<S extends Site, B extends Mods['bind'] = false, O extends boolean = boolean> = Source<any, S, { optional: O; bind: B; orArg: boolean; missing: boolean }>;
/** A field the site always fills, so `optional` has no meaning on it. */
type Always<S extends Site> = Field<S, false, false>;
/** A form whose fields read what `F` allows, none named as `R`. */
export type Form<F, R extends string = never> = readonly (string | Fields<F, R> | Opt<Form<F, R>> | OneOf<readonly Form<F, R>[]>)[];
type Scoped<F, R extends string> = readonly (Fields<F, R> | Scope<Scoped<F, R>>)[];

type DirectiveForm = Form<Field<'form', false | 'inside'> | Field<'value', false | 'inside'> | Source<boolean, 'literal', any> | Always<'directive'>, 'name' | 'modifiers'>;

/** A sequence kept for reuse in several forms. */
export const seq = <const I extends readonly Item[]>(...items: I): I => items;
/** `[ … ]`: read when its first word or reader is there. */
export const opt = <const I extends readonly [Item, ...Item[]]>(...items: I): Opt<I> => ({ opt: items });
/** `{ a | b }`: exactly one, chosen by how it starts; wrap in `opt` for at most one. */
export const oneOf = <const A extends readonly [readonly [Item, ...Item[]], ...(readonly [Item, ...Item[]])[]]>(...alternatives: A): OneOf<A> => ({ oneOf: alternatives });
export const scope = <const I extends readonly Item[]>(...items: I): Scope<I> => ({ scope: items });

/** A node type and the form its fields come from. */
export interface Node<T extends string = string, I extends readonly Item[] = readonly Item[]> {
	readonly node: T;
	readonly form?: I;
}

type BlockForm = Form<Field<'form', false | 'inside' | 'outside'> | Field<'body'>, 'fragment'>;
type TagForm = Form<Field<'form'>>;

/** Where a construct may stand: in content, in an attribute value, among an element's attributes, in the text of an rcdata element. */
export type Place = 'content' | 'value' | 'attributes' | 'rcdata';

/** A marker and what its form reads after it: a construct's open, a branch, or its close. */
export interface Piece<I extends BlockForm = BlockForm> {
	/**
	 * Parts that whitespace and comments may stand between; a part is matched as written. A marker
	 * that stops partway is another word, `{.5}` no `{...}`, unless it stops after a part's leading
	 * punctuation or a whole part: `{#eac` is an error.
	 */
	readonly marker: readonly [string, ...string[]];
	/** Whitespace must follow the marker. */
	readonly space?: boolean;
	readonly form: I;
}

export interface Branch<I extends BlockForm = BlockForm> extends Piece<I> {
	/** The branch opens the construct again inside this field, with this flag true on the one it opens. */
	readonly reopen?: readonly [field: string, flag: string];
}

/** A construct without a close: it reads no content and opens no scope. */
export interface Tag {
	/** Content when left out. */
	readonly in?: readonly Place[];
	readonly open: Piece<TagForm>;
	readonly branches?: undefined;
	readonly close?: undefined;
}

/**
 * A construct with a close. Its content, up to the close, is its `fragment`; with branches, the open
 * and every branch end in a list each names with `content`.
 */
export interface Block {
	/** Content when left out. */
	readonly in?: readonly Place[];
	readonly open: Piece;
	readonly branches?: readonly Branch[];
	/** Words only: what ends the block. */
	readonly close: Piece<readonly string[]>;
}

/** A tag or a block; the type of the node it makes is the key it is written under. */
export type Construct = Tag | Block;

export interface Directive<T extends string = string> extends Node<T, DirectiveForm> {
	readonly unique?: 'kind' | 'attributes';
}
/** What a directive's attribute name holds besides the `name` and `modifiers` every directive node has. */
export const directive = {
	/** The directive's own name, for a host whose node types do not say it: `bind` of `v-bind:value`. */
	kind: source<string, 'directive'>('directive', 'name'),
	/** The attribute name as written. */
	raw: source<string, 'directive'>('directive', 'raw'),
};

export interface Element<T extends string = string> extends Node<T, Form<Field<'this'>, 'name' | 'attributes' | 'fragment'>> {
	readonly root?: true;
	readonly once?: true;
	/** Only inside the element this names. */
	readonly inside?: string;
	readonly outside?: string;
	readonly content?: 'raw' | 'rcdata';
}
/** What an element's tag holds besides the `name`, `attributes` and `fragment` every element node has. */
export const element = {
	/** The expression of a `this` attribute, which leaves the attributes. */
	this: source<Expression, 'this'>('element', 'this'),
	/** The same, a text value read as a string literal. */
	thisOrText: source<Expression, 'this'>('element', 'this:text'),
};

type Marker = readonly [attribute: string] | readonly [attribute: string, value: string];

/**
 * A host language: its document, its content, and the JavaScript inside them. The definition names
 * what the host's nodes mean; what every markup language has is named the same in every host: a
 * list of child nodes is a `Fragment` holding them as `nodes`, a node's one list is its `fragment`,
 * an element's tag is `name` and its attributes `attributes`, a `Text` holds `data` and `raw`, a
 * `Comment` holds `data`, and a directive's argument is `name` and its modifiers `modifiers`.
 */
export interface Definition {
	readonly document: Node<string, Scoped<Field<'document'> | Source<null | readonly [], 'literal', any>, 'fragment'>>;
	/** `{name}` among the attributes is `name={name}`: the marker and the word around the name. */
	readonly attributes?: { readonly shorthand?: readonly [open: string, close: string] };
	readonly autoclose?: true;
	readonly trim?: true;
	readonly void?: readonly string[];
	readonly verbatim?: string;
	readonly elements?: {
		readonly rules?: { readonly [name: string]: Element };
		readonly component?: Element;
		readonly other?: Element;
	};
	readonly script?: { readonly element: string; readonly module?: readonly Marker[]; readonly typescript?: readonly Marker[] };
	readonly style?: string;
	readonly directives?: {
		readonly prefix?: string;
		readonly arg?: string;
		readonly modifier?: string;
		/** The brackets around an argument that is an expression, which `name` then holds. */
		readonly dynamic?: readonly [open: string, close: string];
		readonly unique?: 'raw';
		readonly shorthands?: { readonly [token: string]: readonly [name: string, ...modifiers: string[]] };
		readonly rules?: { readonly [name: string]: Directive };
		readonly other?: Directive;
	};
	/** By the type of the node each makes. */
	readonly constructs?: { readonly [type: string]: Construct };
}

export interface Grammar<D extends Definition = Definition> extends Language<DocumentNode<D>> {
	readonly host: string;
	readonly definition: D;
}
// one literal, so every grammar has the shape of the first: a getter added after would make the rest dictionaries
export const grammar = <const D extends Definition>(host: string, definition: D): Grammar<D> => {
	const plan = engine.plan(JSON.stringify({ name: host, definition }));
	const about: { children: Record<string, string[]>; names: string[] } = JSON.parse(engine.about(plan));
	return { host, definition, children: frozen({ ...builtin, ...about.children }), [LANGUAGE]: Object.freeze({ entry: 0, plan, names: Object.freeze(about.names) }) };
};

// ── inference

interface Member<K extends string = string, S = unknown, Maybe extends boolean = boolean> {
	key: K;
	source: S;
	maybe: Maybe;
}

type Collect<I, Maybe extends boolean> = I extends readonly [infer H, ...infer R] ? Step<H, Maybe> | Collect<R, Maybe> : never;
type Step<H, Maybe extends boolean> = H extends string
	? never
	: H extends Opt<infer I>
		? Collect<I, true>
		: H extends OneOf<infer A>
			? Collect<A[number], true>
			: H extends Scope<infer I>
				? Collect<I, Maybe>
				: { [K in keyof H & string]: Member<K, H[K], Maybe> }[keyof H & string];

type Out<S> = S extends { readonly [out]?: { readonly t: infer T } } ? T : never;
type ModsOf<S> = S extends { readonly [out]?: { readonly m: infer M extends Mods } } ? M : Plain;
type Absent<E> = E extends Member<any, infer S> ? (ModsOf<S>['optional'] extends true ? true : never) : never;
type Nullable<E, Force extends boolean> = E extends Member<any, infer S, infer Maybe>
	? ModsOf<S>['orArg'] extends true
		? never
		: Maybe extends true
			? true
			: ModsOf<S>['missing'] extends true
				? true
				: Force extends true
					? S extends Source<any, 'form', any>
						? true
						: never
					: never
	: never;

type Simplify<T> = { [K in keyof T]: T[K] } & {};

type Shape<E extends Member, G extends Definition, Force extends boolean> = Simplify<
	{
		[K in E['key'] as true extends Absent<Extract<E, { key: K }>> ? never : K]:
			| Resolve<Out<Extract<E, { key: K }>['source']>, G>
			| (true extends Nullable<Extract<E, { key: K }>, Force> ? null : never);
	} & {
		[K in E['key'] as true extends Absent<Extract<E, { key: K }>> ? K : never]?: Resolve<Out<Extract<E, { key: K }>['source']>, G>;
	}
>;

type Resolve<T, G extends Definition> = T extends Children
	? FragmentNode<G>
	: T extends Written
		? AttributeNode<G>['value']
		: T extends Attributes
			? Attribute<G>[]
			: T extends Script
				? ScriptNode<G>
				: T;

type Typed<T extends string, F> = Simplify<{ type: T } & Span & F>;

type FormOf<N> = N extends { readonly form?: infer I } ? (NonNullable<I> extends readonly unknown[] ? NonNullable<I> : readonly []) : readonly [];

type NodeOf<N, G extends Definition, Extra extends Member = never, Force extends boolean = false> = N extends { readonly node: infer T extends string }
	? Typed<T, Shape<Collect<FormOf<N>, false> | Extra, G, Force>>
	: never;

type Fixed<K extends string, T> = Member<K, Source<T, 'literal'>, false>;

type FragmentNode<G extends Definition> = { type: 'Fragment'; nodes: Content<G>[] };
type TextNode = Typed<'Text', { raw: string; data: string }>;
type CommentNode = Typed<'Comment', { data: string }>;
type DocumentNode<G extends Definition> = NodeOf<G['document'], G, Fixed<'fragment', Children>>;

/** A branch adds its fields to the construct, or, reopening it, the field it reopens into and its flag. */
type BranchMembers<B> = B extends { readonly reopen: readonly [infer F extends string, infer Flag extends string] }
	? Member<F, Source<Children, 'form', Plain>, true> | Member<Flag, Source<boolean, 'form', Plain>, false>
	: Collect<FormOf<B>, true>;

type Lists<E> = E extends Member<any, infer S> ? (Out<S> extends Children ? true : never) : never;
type Single<C, E> = C extends { readonly close: object } ? (true extends Lists<E> ? never : Fixed<'fragment', Children>) : never;

type ConstructNode<T extends string, C, G extends Definition> = C extends { readonly open: { readonly form: infer I } }
	? Typed<T, Shape<Collect<I, false> | Single<C, Collect<I, false>> | BranchMembers<C extends { readonly branches: readonly (infer B)[] } ? B : never>, G, false>>
	: never;
/** Whether a construct stands in `P`, content when it says nowhere. */
type Stands<C, P extends Place> = C extends { readonly in: readonly (infer In)[] } ? P extends In ? true : false : P extends 'content' ? true : false;
/** The nodes of the constructs that stand in `P`. */
type Constructs<G extends Definition, P extends Place> = {
	[T in keyof NonNullable<G['constructs']> & string]: Stands<NonNullable<G['constructs']>[T], P> extends true ? ConstructNode<T, NonNullable<G['constructs']>[T], G> : never;
}[keyof NonNullable<G['constructs']> & string];
type DirectiveNode<D, G extends Definition> = D extends Directive ? NodeOf<D, G, Fixed<'name', string | Expression | null> | Fixed<'modifiers', string[]>, true> : never;
type ElementNode<E, G extends Definition> = E extends Element ? NodeOf<E, G, Fixed<'name', string> | Fixed<'attributes', Attributes> | Fixed<'fragment', Children>> : never;

type Values<T> = T extends object ? T[keyof T] : never;

type ScriptNode<G extends Definition> = Typed<'Script', { context: string; content: Program; attributes: Attribute<G>[] }>;
type AttributeNode<G extends Definition> = Typed<
	'Attribute',
	{ name: string; value: true | Constructs<G, 'value'> | (TextNode | Constructs<G, 'value'>)[] }
>;

/** What a host's content can hold; an rcdata element's, what stands in rcdata. */
export type Content<G extends Definition> =
	| ElementNode<Values<NonNullable<G['elements']>['rules']> | NonNullable<G['elements']>['component'] | NonNullable<G['elements']>['other'], G>
	| TextNode
	| CommentNode
	| Constructs<G, 'content'>
	| Constructs<G, 'rcdata'>;

/** What an element's attributes can hold. */
export type Attribute<G extends Definition> =
	| AttributeNode<G>
	| DirectiveNode<Values<NonNullable<G['directives']>['rules']> | NonNullable<G['directives']>['other'], G>
	| Constructs<G, 'attributes'>;

/** The tree a grammar's parse answers with. */
export type Infer<Gr extends Grammar> = Gr extends Grammar<infer G> ? DocumentNode<G> : never;
/** A node type of the grammar, by name. */
export type NodeType<Gr extends Grammar, T extends string> = Gr extends Grammar<infer G>
	? Extract<Content<G> | Attribute<G> | Constructs<G, 'value'> | FragmentNode<G> | ScriptNode<G>, { type: T }>
	: never;
