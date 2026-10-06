import * as g from '../../src/grammar.ts';

// `v-for`'s left side, with or without its parentheses
const aliases = g.seq(
	g.opt({ value: g.optional(g.bind(g.js.pattern)) }),
	g.opt(',', g.opt({ key: g.optional(g.bind(g.js.pattern)) }), g.opt(',', g.opt({ index: g.optional(g.bind(g.js.pattern)) }))),
);

export default g.grammar('vue', {
	document: { node: 'Root', form: [{ children: g.content }] },
	text: { node: 'Text', form: [{ content: g.text.data }] },
	comment: { node: 'Comment', form: [{ content: g.text.data }] },
	void: ['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'],
	verbatim: 'v-pre',

	elements: {
		fields: { tag: g.element.tag, props: g.element.attributes, children: g.content },
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
		fields: { name: g.directive.kind, arg: g.directive.arg, modifiers: g.directive.modifiers, rawName: g.directive.raw },
		shorthands: { ':': ['bind'], '@': ['on'], '#': ['slot'], '.': ['bind', 'prop'] },
		rules: {
			for: { node: 'Directive', form: [g.opt(g.oneOf(['(', ...aliases, ')'], aliases)), g.oneOf(['in'], ['of']), { source: g.js.expression }] },
			slot: { node: 'Directive', form: [{ props: g.optional(g.js.pattern) }] },
			on: { node: 'Directive', form: [{ handler: g.optional(g.js.code) }] },
		},
		other: { node: 'Directive', form: [{ exp: g.optional(g.js.expression) }] },
	},

	constructs: {
		Interpolation: { in: ['content', 'rcdata'], open: { marker: ['{{'], form: [{ content: g.js.expression }, '}}'] } },
	},
});
