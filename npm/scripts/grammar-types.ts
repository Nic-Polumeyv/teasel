// `tsc -p npm` checks these: what Infer answers for the two hosts, and the forms the algebra refuses
import type { Expression, Identifier, Pattern } from 'estree';
import * as g from '../src/grammar.ts';
import type { Infer, NodeType } from '../src/grammar.ts';
import { Plan, type Source } from '../src/index.ts';
declare const open: Source;
import type svelte from './hosts/svelte.ts';
import type vue from './hosts/vue.ts';

type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
const expect = <T extends true>() => {};

type Svelte = typeof svelte;
type Fragment = NodeType<Svelte, 'Fragment'>;

type Each = NodeType<Svelte, 'EachBlock'>;
type Required<T> = { [K in keyof T]-?: {} extends Pick<T, K> ? never : K }[keyof T];
expect<Equal<Required<Each>, 'type' | 'start' | 'end' | 'expression' | 'context' | 'body'>>();
expect<Equal<Each['context'], Pattern | null>>();
expect<Equal<Each['index'], Identifier | undefined>>();
expect<Equal<Each['key'], Expression | undefined>>();
expect<Equal<Each['body'], Fragment>>();
expect<Equal<Each['fallback'], Fragment | undefined>>();

expect<Equal<NodeType<Svelte, 'IfBlock'>['alternate'], Fragment | null>>();
expect<Equal<NodeType<Svelte, 'IfBlock'>['elseif'], boolean>>();
expect<Equal<NodeType<Svelte, 'AwaitBlock'>['pending'], Fragment | null>>();
expect<Equal<NodeType<Svelte, 'SnippetBlock'>['typeParams'], string | undefined>>();
expect<Equal<keyof Fragment, 'type' | 'nodes'>>();
expect<Equal<NodeType<Svelte, 'BindDirective'>['expression'], Expression>>();
expect<Equal<NodeType<Svelte, 'OnDirective'>['expression'], Expression | null>>();
expect<Equal<NodeType<Svelte, 'TransitionDirective'>['intro'], boolean>>();
expect<Equal<Infer<Svelte>['instance'], NodeType<Svelte, 'Script'> | undefined>>();
expect<Equal<Infer<Svelte>['fragment'], Fragment>>();

type Vue = typeof vue;
const f = {} as Extract<NodeType<Vue, 'Directive'>, { source: unknown }>;
expect<Equal<typeof f.source, Expression | null>>();
expect<Equal<typeof f.value, Pattern | undefined>>();
expect<Equal<Infer<Vue>['children'][number]['type'], 'Element' | 'Text' | 'Comment' | 'Interpolation' | 'Slot' | 'Template' | 'Component'>>();

// @ts-expect-error a field may not be named type
g.node('X', { type: g.js.expression });
// @ts-expect-error only what reads a pattern, an identifier or parameters can declare
g.bind(g.js.expression);
// @ts-expect-error an argument stands in only for a directive's value
g.orArg(g.js.expression);
// @ts-expect-error a tag has no body
g.tag(g.node('T', { body: g.content }));
// @ts-expect-error a tag opens no scope to declare in
g.tag(g.node('T', { name: g.bind(g.js.identifier) }));
// @ts-expect-error only a block declares around itself
g.directive(g.node('D', { name: g.bind.outside(g.js.identifier) }));
g.grammar('x', {
	document: g.node('Root', { children: g.content }),
	text: g.node('Text', { data: g.text.data }),
	comment: g.node('Comment', { data: g.text.data }),
	delimiters: ['{', '}'],
	elements: {
		fields: { name: g.element.tag, attributes: g.element.attributes, children: g.content },
		// @ts-expect-error inside names an element rule
		rules: { head: g.element(g.node('Head')), title: g.element(g.node('Title'), { inside: 'haed' }) },
	},
});

// a plan's parse answers with the grammar's tree
declare const definition: Svelte;
const typed = new Plan(definition);
expect<Equal<typeof typed, Plan<Infer<Svelte>>>>();
expect<Equal<ReturnType<typeof open.parse<Infer<Svelte>>>['node'], Infer<Svelte>>>();
// @ts-expect-error a plan no longer takes the grammar's text
new Plan('host x');
