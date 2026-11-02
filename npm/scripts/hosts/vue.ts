import * as g from '../../src/grammar.ts';

// `v-for`'s left side, with or without its parentheses
const aliases = g.seq(
	g.opt({ value: g.optional(g.bind(g.js.pattern)) }),
	g.opt(',', g.opt({ key: g.optional(g.bind(g.js.pattern)) }), g.opt(',', g.opt({ index: g.optional(g.bind(g.js.pattern)) }))),
);
const named = { kind: g.directive.kind, rawName: g.directive.raw };

export default g.grammar('vue', {
	document: { node: 'Root' },
	void: ['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'],
	verbatim: 'v-pre',

	elements: {
		rules: {
			slot: { node: 'Slot' },
			template: { node: 'Template' },
			component: { node: 'Component' },
			textarea: { node: 'Element', content: 'rcdata' },
			title: { node: 'Element', content: 'rcdata' },
			script: { node: 'Element', content: 'raw' },
			style: { node: 'Element', content: 'raw' },
		},
		component: { node: 'Component' },
		other: { node: 'Element' },
	},

	directives: {
		prefix: 'v-',
		arg: ':',
		modifier: '.',
		dynamic: ['[', ']'],
		unique: 'raw',
		shorthands: { ':': ['bind'], '@': ['on'], '#': ['slot'], '.': ['bind', 'prop'] },
		rules: {
			for: { node: 'Directive', form: [named, g.opt(g.oneOf(['(', ...aliases, ')'], aliases)), g.oneOf(['in'], ['of']), { source: g.js.expression }] },
			slot: { node: 'Directive', form: [named, { props: g.optional(g.js.pattern) }] },
			on: { node: 'Directive', form: [named, { handler: g.optional(g.js.code) }] },
		},
		other: { node: 'Directive', form: [named, { exp: g.optional(g.js.expression) }] },
	},

	constructs: {
		Interpolation: { in: ['content', 'rcdata'], open: { marker: ['{{'], form: [{ content: g.js.expression }, '}}'] } },
	},
});
