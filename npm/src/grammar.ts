import type { Expression, Identifier, Pattern, Program, SourceLocation, Statement, VariableDeclaration } from 'estree';
import type { Comment, HostNode } from './types.js';
import { wire as encode } from './wire.js';

declare const out: unique symbol;

/** Where a source may be read: a form, a block's body, a directive's attribute value, the document, an element, a `this` attribute, a text or comment node, the fragment node, a directive's name, a literal. */
export type Site = 'form' | 'body' | 'value' | 'document' | 'element' | 'this' | 'text' | 'fragment' | 'directive' | 'literal';

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
/** A node's children: the grammar's fragment node when it has one, else the list. */
export interface Children {
	readonly [children]: true;
}
interface Nodes {
	readonly [children]: 'list';
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
	({ from, read, optional: false, bind: false, orArg: false }) as unknown as Source<T, S, M>;

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
	/** The text up to the closing delimiter, unread. */
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

/** A text or comment node's text: as read, and as written. */
export const text = {
	data: source<string, 'text'>('text', 'data'),
	raw: source<string, 'text'>('text', 'raw'),
};

/** Where the host's content goes: a block's body, an element's children, the document's. */
export const content = source<Children, 'body' | 'document' | 'element'>('content', 'fragment');
/** The fragment node's own list. */
export const nodes = source<Nodes, 'fragment'>('nodes', 'nodes');

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
/** Fields a form reads, each named once, where its source is. */
export type Fields = { readonly [field: string]: Source<any, any, any> } & { readonly [K in Reserved]?: never };

/** One step of a form: a host word or punctuator, fields, or a group. */
export type Item = string | Fields | Opt<readonly Item[]> | OneOf<readonly (readonly Item[])[]> | Scope<readonly Item[]>;
export interface Opt<I extends readonly unknown[]> {
	readonly opt: I;
}
export interface OneOf<A extends readonly (readonly unknown[])[]> {
	readonly oneOf: A;
}
/** The document's or fragment's fields whose names the fields inside see. */
export interface Scope<I extends readonly unknown[]> {
	readonly scope: I;
}

type Field<S extends Site, B extends Mods['bind'] = false, O extends boolean = boolean> = Source<any, S, { optional: O; bind: B; orArg: boolean; missing: boolean }>;
/** A field the site always fills, so `optional` has no meaning on it. */
type Always<S extends Site> = Field<S, false, false>;
type Record<F> = { readonly [field: string]: F } & { readonly [K in Reserved]?: never };
/** A form whose fields read what `F` allows. */
export type Form<F> = readonly (string | Record<F> | Opt<Form<F>> | OneOf<readonly Form<F>[]>)[];
type Scoped<F> = readonly (Record<F> | Scope<Scoped<F>>)[];

type BlockForm = Form<Field<'form', false | 'inside' | 'outside'> | Field<'body'>>;
type TagForm = Form<Field<'form'>>;
type DirectiveForm = Form<Field<'form', false | 'inside'> | Field<'value', false | 'inside'> | Source<boolean, 'literal', any>>;

/** A sequence kept for reuse in several forms. */
export const seq = <const I extends readonly Item[]>(...items: I): I => items;
/** `[ … ]`: read when its first word or reader is there. */
export const opt = <const I extends readonly [Item, ...Item[]]>(...items: I): Opt<I> => ({ opt: items });
/** `{ a | b }`: exactly one, chosen by how it starts; wrap in `opt` for at most one. */
export const oneOf = <const A extends readonly [readonly [Item, ...Item[]], ...(readonly [Item, ...Item[]])[]]>(...alternatives: A): OneOf<A> => ({ oneOf: alternatives });
export const scope = <const I extends readonly Item[]>(...items: I): Scope<I> => ({ scope: items });

export interface Node<T extends string = string, I extends readonly Item[] = readonly Item[]> {
	readonly type: T;
	readonly items: I;
}
/** A node type and the form its fields come from. */
export const node = <const T extends string, const I extends readonly Item[]>(type: T, ...items: I): Node<T, I> => ({ type, items });

/** An `else if`: the block again, nested into this body field, with this flag set on it. */
export interface Reopen<F extends string = string, Flag extends string = string> {
	readonly reopen: F;
	readonly flag: Flag;
}
export const reopen = <const F extends string, const Flag extends string>(field: F, flag: Flag): Reopen<F, Flag> => ({ reopen: field, flag });

export interface Block<N extends Node = Node, B extends Branches = Branches> {
	readonly site: 'block';
	readonly node: N;
	readonly branches: B;
}
type Branches = { readonly [words: string]: BlockForm | Reopen };
export const block = <const N extends Node<string, BlockForm>, const B extends Branches = {}>(node: N, options?: { branches?: B }): Block<N, B> => ({
	site: 'block',
	node,
	branches: (options?.branches ?? {}) as B,
});

export interface Tag<N extends Node = Node, A extends 'content' | 'attributes' = 'content' | 'attributes'> {
	readonly site: 'tag';
	readonly node: N;
	readonly among: A;
}
export const tag = <const N extends Node<string, TagForm>, const A extends 'content' | 'attributes' = 'content'>(node: N, options?: { among?: A }): Tag<N, A> => ({
	site: 'tag',
	node,
	among: (options?.among ?? 'content') as A,
});

export interface Directive<N extends Node = Node> {
	readonly site: 'directive';
	readonly node: N;
	readonly unique: 'no' | 'kind' | 'attributes';
}
/** A directive's rule, and what its attribute name holds. */
export const directive = Object.assign(
	<const N extends Node<string, DirectiveForm>>(node: N, options?: { unique?: 'kind' | 'attributes' }): Directive<N> => ({
		site: 'directive',
		node,
		unique: options?.unique ?? 'no',
	}),
	{
		kind: source<string, 'directive'>('directive', 'name'),
		/** The argument; an expression when the directive syntax brackets a dynamic one. */
		arg: source<string | Expression | null, 'directive'>('directive', 'arg'),
		modifiers: source<string[], 'directive'>('directive', 'modifiers'),
		raw: source<string, 'directive'>('directive', 'raw'),
	},
);

export interface Element<N extends Node = Node, I extends string | undefined = string | undefined> {
	readonly site: 'element';
	readonly node: N;
	readonly root: boolean;
	readonly once: boolean;
	readonly inside?: I;
	readonly outside?: string;
	readonly content?: 'raw' | 'rcdata';
}
/** An element's rule, and what its tag holds. */
export const element = Object.assign(
	<const N extends Node<string, Form<Field<'this'>>>, const I extends string | undefined = undefined>(
		node: N,
		options?: { root?: true; once?: true; inside?: I; outside?: string; content?: 'raw' | 'rcdata' },
	): Element<N, I> => ({ site: 'element', node, root: options?.root ?? false, once: options?.once ?? false, ...options }),
	{
		tag: source<string, 'element'>('element', 'name'),
		attributes: source<Attributes, 'element'>('element', 'attributes'),
		/** The expression of a `this` attribute, which leaves the attributes. */
		this: source<Expression, 'this'>('element', 'this'),
		/** The same, a text value read as a string literal. */
		thisOrText: source<Expression, 'this'>('element', 'this:text'),
	},
);

type Marker = readonly [attribute: string] | readonly [attribute: string, value: string];

/** A host language: its document, its content, and the JavaScript inside them. */
export interface Definition {
	readonly document: Node<string, Scoped<Field<'document'> | Source<null | readonly [], 'literal', any>>>;
	readonly text: Node<string, Scoped<Always<'text'>>>;
	readonly comment: Node<string, Scoped<Always<'text'>>>;
	readonly fragment?: Node<string, Scoped<Always<'fragment'>>>;
	readonly delimiters: readonly [open: string, close: string];
	readonly attributes?: { readonly expressions?: true; readonly shorthand?: true };
	readonly autoclose?: true;
	readonly trim?: true;
	readonly void?: readonly string[];
	readonly verbatim?: string;
	readonly elements: {
		readonly fields: Record<Always<'element'>>;
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
		readonly dynamic?: readonly [open: string, close: string];
		readonly unique?: 'raw';
		readonly fields: Record<Always<'directive'>>;
		readonly shorthands?: { readonly [token: string]: readonly [name: string, ...modifiers: string[]] };
		readonly rules?: { readonly [name: string]: Directive };
		readonly other?: Directive;
	};
	readonly spread?: string;
	readonly sigils?: {
		readonly open: string;
		readonly branch: string;
		readonly close: string;
		readonly tag: string;
		readonly blocks?: { readonly [name: string]: Block };
		readonly tags?: { readonly [name: string]: Tag };
	};
	readonly declaration?: Node<string, TagForm>;
	/** The node of an expression between the delimiters: one field, one expression. */
	readonly expression?: Node<string, readonly [Record<Source<Expression, 'form'>>]>;
}

declare const root: unique symbol;
/** What a parse by the grammar answers with, as a type. */
export interface Answers<T> {
	readonly [root]?: T;
}
export interface Grammar<D extends Definition = Definition> extends Answers<NodeOf<D['document'], D>> {
	readonly host: string;
	readonly definition: D;
	/** What the engine reads: the definition on its wire, written once when the grammar was made. */
	readonly wire: Uint8Array;
}
type Names<D extends Definition> = keyof NonNullable<D['elements']['rules']> & string;
/** An element's `inside` names another element rule. */
type Checked<D extends Definition> = {
	readonly elements: {
		readonly rules?: {
			readonly [K in keyof D['elements']['rules']]: D['elements']['rules'][K] extends Element<any, infer I>
				? I extends undefined | Names<D>
					? unknown
					: { readonly inside: Names<D> }
				: unknown;
		};
	};
};
export const grammar = <const D extends Definition>(host: string, definition: D & Checked<D>): Grammar<D> => ({
	host,
	definition,
	wire: encode({ name: host, definition }),
});

// ── inference

interface Entry<K extends string = string, S = unknown, Maybe extends boolean = boolean> {
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
				: { [K in keyof H & string]: Entry<K, H[K], Maybe> }[keyof H & string];

type Out<S> = S extends { readonly [out]?: { readonly t: infer T } } ? T : never;
type ModsOf<S> = S extends { readonly [out]?: { readonly m: infer M extends Mods } } ? M : Plain;
type Absent<E> = E extends Entry<any, infer S> ? (ModsOf<S>['optional'] extends true ? true : never) : never;
type Nullable<E, Force extends boolean> = E extends Entry<any, infer S, infer Maybe>
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
type Span = { start: number; end: number; loc?: SourceLocation };

type Shape<E extends Entry, G extends Definition, Force extends boolean> = Simplify<
	{
		[K in E['key'] as true extends Absent<Extract<E, { key: K }>> ? never : K]:
			| Resolve<Out<Extract<E, { key: K }>['source']>, G>
			| (true extends Nullable<Extract<E, { key: K }>, Force> ? null : never);
	} & {
		[K in E['key'] as true extends Absent<Extract<E, { key: K }>> ? K : never]?: Resolve<Out<Extract<E, { key: K }>['source']>, G>;
	}
>;

type Resolve<T, G extends Definition> = T extends Children
	? G['fragment'] extends Node ? FragmentNode<G> : Content<G>[]
	: T extends Nodes
		? Content<G>[]
		: T extends Written
			? AttributeNode<G>['value']
			: T extends Attributes
			? Attribute<G>[]
			: T extends Script
				? ScriptNode<G>
				: T;

type Typed<T extends string, F> = Simplify<{ type: T } & Span & F>;

type NodeOf<N, G extends Definition, Extra extends Entry = never, Force extends boolean = false> = N extends Node<infer T, infer I>
	? Typed<T, Shape<Collect<I, false> | Extra, G, Force>>
	: never;

type FragmentNode<G extends Definition> = G['fragment'] extends Node<infer T, infer I>
	? Simplify<{ type: T } & Shape<Collect<I, false>, G, false>>
	: never;

type BranchEntries<B> = {
	[W in keyof B]: B[W] extends Reopen<infer F, infer Flag>
		? Entry<F, Source<Children, 'form', Plain>, true> | Entry<Flag, Source<boolean, 'form', Plain>, false>
		: Collect<B[W], true>;
}[keyof B];

type BlockNode<B, G extends Definition> = B extends Block<infer N, infer Br> ? NodeOf<N, G, BranchEntries<Br>> : never;
type TagNode<T, G extends Definition> = T extends Tag<infer N> ? NodeOf<N, G> : never;
type DirectiveNode<D, G extends Definition> = D extends Directive<infer N>
	? G['directives'] extends { fields: infer F }
		? NodeOf<N, G, Collect<[F], false>, true>
		: never
	: never;
type ElementNode<E, G extends Definition> = E extends Element<infer N> ? NodeOf<N, G, Collect<[G['elements']['fields']], false>> : never;

type Values<T> = T extends object ? T[keyof T] : never;

type ScriptNode<G extends Definition> = Typed<'Script', { context: string; content: Program; attributes: Attribute<G>[] }>;
type AttributeNode<G extends Definition> = Typed<'Attribute', { name: string; value: true | Content<G> | Content<G>[] }>;

/** What a host's content can hold. */
export type Content<G extends Definition> =
	| ElementNode<Values<G['elements']['rules']> | G['elements']['component'] | G['elements']['other'], G>
	| NodeOf<G['text'], G>
	| NodeOf<G['comment'], G>
	| BlockNode<Values<NonNullable<G['sigils']>['blocks']>, G>
	| TagNode<Extract<Values<NonNullable<G['sigils']>['tags']>, Tag<any, 'content'>>, G>
	| NodeOf<G['expression'], G>
	| NodeOf<G['declaration'], G>;

/** What an element's attributes can hold. */
export type Attribute<G extends Definition> =
	| AttributeNode<G>
	| DirectiveNode<Values<NonNullable<G['directives']>['rules']> | NonNullable<G['directives']>['other'], G>
	| TagNode<Extract<Values<NonNullable<G['sigils']>['tags']>, Tag<any, 'attributes'>>, G>
	| (G['spread'] extends string ? Typed<G['spread'], { expression: Expression }> : never);

/** The tree a grammar's parse answers with. */
export type Infer<Gr extends Grammar> = Gr extends Grammar<infer G> ? NodeOf<G['document'], G> : never;
/** A node type of the grammar, by name. */
export type NodeType<Gr extends Grammar, T extends string> = Gr extends Grammar<infer G>
	? Extract<Content<G> | Attribute<G> | FragmentNode<G> | ScriptNode<G>, { type: T }>
	: never;
