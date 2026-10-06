import * as g from '../../src/grammar.ts';

// `v-for`'s left side, with or without its parentheses
const aliases = g.seq(
	g.opt({ value: g.optional(g.bind(g.js.pattern)) }),
	g.opt(',', g.opt({ key: g.optional(g.bind(g.js.pattern)) }), g.opt(',', g.opt({ index: g.optional(g.bind(g.js.pattern)) }))),
);

export default g.grammar('vue', {
	document: g.node('Root', { children: g.content }),
	text: g.node('Text', { content: g.text.data }),
	comment: g.node('Comment', { content: g.text.data }),
	void: ['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'],
	verbatim: 'v-pre',

	elements: {
		fields: { tag: g.element.tag, props: g.element.attributes, children: g.content },
		rules: {
			slot: g.node('Slot'),
			template: g.node('Template'),
			component: g.node('Component'),
			textarea: { node: 'Element', content: 'rcdata' },
			title: { node: 'Element', content: 'rcdata' },
			script: { node: 'Element', content: 'raw' },
			style: { node: 'Element', content: 'raw' },
		},
		component: g.node('Component'),
		other: g.node('Element'),
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
			for: g.node('Directive', g.opt(g.oneOf(['(', ...aliases, ')'], aliases)), g.oneOf(['in'], ['of']), { source: g.js.expression }),
			slot: g.node('Directive', { props: g.optional(g.js.pattern) }),
			on: g.node('Directive', { handler: g.optional(g.js.code) }),
		},
		other: g.node('Directive', { exp: g.optional(g.js.expression) }),
	},

	constructs: {
		interpolation: { node: 'Interpolation', in: ['content', 'rcdata'], open: { marker: ['{{'], form: [{ content: g.js.expression }, '}}'] } },
	},
});
