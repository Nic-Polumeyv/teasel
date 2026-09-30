import type { Expression, Identifier, Pattern, Program, SourceLocation, Statement, VariableDeclaration } from 'estree';
import type { Comment, HostNode } from './index.js';
import { Writer, writeGrammar } from './wire.js';
import type * as w from './wire.js';

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
	raw: source<Children, 'value'>('value', 'value'),
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
export const optional = <T, S extends Site, M extends Mods>(s: Source<T, S, M>): Source<T, S, With<M, 'optional', true>> =>
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

type Reserved = 'type' | 'start' | 'end' | 'loc';
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

type Field<S extends Site, B extends Mods['bind'] = false> = Source<any, S, { optional: boolean; bind: B; orArg: boolean; missing: boolean }>;
type Record<F> = { readonly [field: string]: F } & { readonly [K in Reserved]?: never };
/** A form whose fields read what `F` allows. */
export type Form<F> = readonly (string | Record<F> | Opt<Form<F>> | OneOf<readonly Form<F>[]>)[];
type Scoped<F> = readonly (Record<F> | Scope<Scoped<F>>)[];

type BlockForm = Form<Field<'form', false | 'inside' | 'outside'> | Field<'body'>>;
type TagForm = Form<Field<'form'>>;
type DirectiveForm = Form<Field<'form', false | 'inside'> | Field<'value', false | 'inside'> | Field<'literal'>>;

/** A sequence kept for reuse in several forms. */
export const seq = <const I extends readonly Item[]>(...items: I): I => items;
/** `[ … ]`: read when its first word or reader is there. */
export const opt = <const I extends readonly Item[]>(...items: I): Opt<I> => ({ opt: items });
/** `{ a | b }`: exactly one, chosen by how it starts; wrap in `opt` for at most one. */
export const oneOf = <const A extends readonly (readonly Item[])[]>(...alternatives: A): OneOf<A> => ({ oneOf: alternatives });
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
		arg: source<string | null, 'directive'>('directive', 'arg'),
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
	readonly document: Node<string, Scoped<Field<'document'> | Field<'literal'>>>;
	readonly text: Node<string, Scoped<Field<'text'>>>;
	readonly comment: Node<string, Scoped<Field<'text'>>>;
	readonly fragment?: Node<string, Scoped<Field<'fragment'>>>;
	readonly delimiters: readonly [open: string, close: string];
	readonly attributes?: { readonly expressions?: true; readonly shorthand?: true };
	readonly autoclose?: true;
	readonly trim?: true;
	readonly void?: readonly string[];
	readonly verbatim?: string;
	readonly elements: {
		readonly fields: Record<Field<'element'>>;
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
		readonly fields: Record<Field<'directive'>>;
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
	wire: encode(lower(host, definition)),
});

// ── the wire the engine reads

function encode(grammar: w.Grammar): Uint8Array {
	const w = new Writer();
	writeGrammar(w, grammar);
	return w.bytes();
}

type Raw = Source<unknown, any, Mods> & { readonly literal?: unknown };
type Group = { readonly opt?: readonly unknown[]; readonly oneOf?: readonly (readonly unknown[])[]; readonly scope?: readonly unknown[] };
type At = 'block' | 'tag' | 'directive';

const isGroup = (item: unknown): item is Group => typeof item === 'object' && item !== null && ('opt' in item || 'oneOf' in item || 'scope' in item);
const entries = (items: readonly unknown[]): [string, Raw][] =>
	items.flatMap((item) => (typeof item === 'object' && item !== null && !isGroup(item) ? (Object.entries(item) as [string, Raw][]) : []));
const opens = (items: readonly unknown[]) => entries(items).some(([, s]) => s.from === 'content');
const part = (items: readonly unknown[], read: string) => entries(items).find(([, s]) => s.read === read)?.[0];

// a form's items; a body declares what its sequence bound before it
function form(items: readonly unknown[], site: At, bound: w.Declare[] = []): w.Form {
	const out: w.Item[] = [];
	let body: w.Body | undefined;
	items.forEach((item, at) => {
		if (typeof item === 'string') return void out.push({ literal: item });
		if (isGroup(item)) {
			if (item.oneOf) return void out.push({ group: { alternatives: alternatives(item.oneOf, site, bound), required: true } });
			const [only] = item.opt!;
			const list = item.opt!.length === 1 && isGroup(only) && only.oneOf ? only.oneOf : [item.opt!];
			return void out.push({ group: { alternatives: alternatives(list, site, bound), required: false } });
		}
		for (const [field, source] of entries([item])) {
			if (source.from === 'literal') continue;
			if (source.from === 'content') {
				if (site !== 'block' || at !== items.length - 1) throw new TypeError(`${field} is a body: only a block's form ends in one`);
				body = { field, omit: source.optional, declares: bound.splice(0) };
				continue;
			}
			const outside = source.bind === 'outside';
			if (source.bind && !bound.some((d) => d.field === field && d.outside === outside)) bound.push({ field, outside });
			out.push({ entry: { field, entry: source.read as w.Entry, omit: source.optional } });
		}
	});
	return { items: out, body };
}
function alternatives(list: readonly (readonly unknown[])[], site: At, bound: w.Declare[]): w.Alternative[] {
	return list.map((items) => form(items, site, items.some((item) => opens([item])) ? [] : bound));
}

function lower(host: string, d: Definition): w.Grammar {
	const holds = (s: Raw): w.RootField => {
		if (s.from === 'literal') return s.literal === null ? 'null' : 'emptyList';
		if (s.read === 'script') return { script: { module: false } };
		if (s.read === 'script:module') return { script: { module: true } };
		return s.read as 'fragment' | 'style' | 'comments';
	};
	const scoped = (items: readonly unknown[]): w.DocField[] =>
		items.flatMap((item): w.DocField[] => (isGroup(item) && item.scope ? [{ scope: scoped(item.scope) }] : entries([item]).map(([field, s]) => ({ field: { field, holds: holds(s), omit: s.optional } }))));
	const texts = (node: Node) => ({ ty: node.type, data: part(node.items, 'data')!, raw: part(node.items, 'raw') });
	const element = (name: w.Match, rule: Element): w.ElementRule => {
		const [[field, self] = []] = entries(rule.node.items);
		return {
			name,
			ty: rule.node.type,
			this: field && self ? [field, self.read === 'this:text'] : undefined,
			root: rule.root,
			once: rule.once,
			inside: rule.inside,
			outside: rule.outside,
			raw: rule.content === 'raw',
			rcdata: rule.content === 'rcdata',
		};
	};
	const markers = (list: readonly Marker[] = []): (readonly [string, string | undefined])[] => list.map(([attribute, value]) => [attribute, value]);
	const directive = (name: w.Match, rule: Directive): w.DirectiveRule => {
		const flags = entries(rule.node.items).filter(([, s]) => s.from === 'literal').map(([field, s]): [string, boolean] => [field, s.literal === true]);
		const rest = rule.node.items.filter((item) => isGroup(item) || typeof item === 'string' || entries([item]).some(([, s]) => s.from !== 'literal'));
		const unique = rule.unique === 'kind' ? 'kind' : rule.unique === 'attributes' ? 'attribute' : 'no';
		const [only] = rest;
		const wrapped = rest.length === 1 && isGroup(only) && only.opt?.length === 1 ? only.opt : undefined;
		const single = entries(wrapped ?? rest);
		if (rest.length === 1 && single.length === 1 && single[0][1].from === 'value') {
			const [field, s] = single[0];
			const fixed = s.read === 'value' ? 'value' : 'expression';
			if (field !== fixed) throw new TypeError(`a directive's value is read into ${fixed}`);
			const one = { optional: wrapped !== undefined || s.orArg, name: s.orArg };
			const value: w.DirectiveValue = s.read === 'value' ? 'value' : s.read === 'pattern' ? { pattern: one } : { expression: one };
			return { name, ty: rule.node.type, value, flags, unique, declares: s.bind ? [] : undefined };
		}
		const bound: w.Declare[] = [];
		const value = form(rest, 'directive', bound);
		return { name, ty: rule.node.type, value: { form: value }, flags, unique, declares: bound.length > 0 ? bound.map((d) => d.field) : undefined };
	};
	const block = (name: string, rule: Block): w.BlockRule => {
		const reopen = Object.values(rule.branches).find((b): b is Reopen => 'reopen' in b);
		const branches = Object.entries(rule.branches).map(([words, branch]): w.BranchRule => {
			if (!('reopen' in branch)) return { words: words.split(' '), form: form(branch, 'block') };
			const [[own]] = entries(rule.node.items.slice(-1));
			const head = form(rule.node.items.slice(0, -1), 'block');
			return { words: words.split(' '), form: { ...head, body: { field: branch.reopen, omit: false, chain: own, declares: [] } } };
		});
		return { name, ty: rule.node.type, open: form(rule.node.items, 'block'), branches, chainFlag: reopen?.flag };
	};
	const tag = (name: string, node: Node, attribute = false): w.TagRule => ({ name, ty: node.type, form: form(node.items, 'tag'), attribute });
	const x = d.directives;
	return {
		name: host,
		document: { ty: d.document.type, fields: scoped(d.document.items) },
		delimiters: d.delimiters,
		attributeExpressions: d.attributes?.expressions === true,
		attributeShorthand: d.attributes?.shorthand === true,
		sigils: d.sigils && { open: d.sigils.open, branch: d.sigils.branch, close: d.sigils.close, tag: d.sigils.tag },
		autoclose: d.autoclose === true,
		trim: d.trim === true,
		void: d.void ?? [],
		fragment: d.fragment && [d.fragment.type, entries(isGroup(d.fragment.items[0]) && d.fragment.items[0].scope ? d.fragment.items[0].scope : d.fragment.items)[0][0]],
		fragmentScope: d.fragment !== undefined && isGroup(d.fragment.items[0]) && d.fragment.items[0].scope !== undefined,
		elementFields: { name: part([d.elements.fields], 'name')!, attributes: part([d.elements.fields], 'attributes')!, children: part([d.elements.fields], 'fragment')! },
		text: texts(d.text),
		comment: texts(d.comment),
		verbatim: d.verbatim,
		elements: [
			...Object.entries(d.elements.rules ?? {}).map(([match, rule]) => element({ exact: match }, rule)),
			...(d.elements.component ? [element('component', d.elements.component)] : []),
			...(d.elements.other ? [element('any', d.elements.other)] : []),
		],
		script: d.script && { name: d.script.element, module: markers(d.script.module), typescript: markers(d.script.typescript) },
		style: d.style,
		directiveSyntax: x && {
			prefix: x.prefix,
			arg: x.arg ?? ':',
			modifier: x.modifier ?? '|',
			dynamic: x.dynamic,
			nameField: part([x.fields], 'name'),
			argField: part([x.fields], 'arg'),
			modifiersField: part([x.fields], 'modifiers'),
			rawField: part([x.fields], 'raw'),
			unique: x.unique === 'raw',
		},
		shorthands: Object.entries(x?.shorthands ?? {}).map(([token, [name, ...modifiers]]) => ({ token, name, modifiers })),
		directives: [
			...Object.entries(x?.rules ?? {}).map(([match, rule]) => directive({ exact: match }, rule)),
			...(x?.other ? [directive('any', x.other)] : []),
		],
		spread: d.spread,
		blocks: Object.entries(d.sigils?.blocks ?? {}).map(([name, rule]) => block(name, rule)),
		tags: Object.entries(d.sigils?.tags ?? {}).map(([name, rule]) => tag(name, rule.node, rule.among === 'attributes')),
		declaration: d.declaration && tag('', d.declaration),
		expression: d.expression && tag('', d.expression),
	};
}

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
