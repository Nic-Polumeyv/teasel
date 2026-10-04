import * as g from '../../dist/grammar.js';

// `v-for`'s left side, with or without its parentheses
const aliases = g.seq(
	g.opt({ value: g.optional(g.bind(g.js.pattern)) }),
	g.opt(',', g.opt({ key: g.optional(g.bind(g.js.pattern)) }), g.opt(',', g.opt({ index: g.optional(g.bind(g.js.pattern)) }))),
);

export default g.grammar('vue', {
	document: g.node('Root', { children: g.content }),
	text: g.node('Text', { content: g.text.data }),
	comment: g.node('Comment', { content: g.text.data }),
	delimiters: ['{{', '}}'],
	void: ['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'],
	verbatim: 'v-pre',

	elements: {
		fields: { tag: g.element.tag, props: g.element.attributes, children: g.content },
		rules: {
			slot: g.element(g.node('Slot')),
			template: g.element(g.node('Template')),
			component: g.element(g.node('Component')),
			textarea: g.element(g.node('Element'), { content: 'rcdata' }),
			title: g.element(g.node('Element'), { content: 'rcdata' }),
			script: g.element(g.node('Element'), { content: 'raw' }),
			style: g.element(g.node('Element'), { content: 'raw' }),
		},
		component: g.element(g.node('Component')),
		other: g.element(g.node('Element')),
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
			for: g.directive(g.node('Directive', g.opt(g.oneOf(['(', ...aliases, ')'], aliases)), g.oneOf(['in'], ['of']), { source: g.js.expression })),
			slot: g.directive(g.node('Directive', { props: g.optional(g.js.pattern) })),
			on: g.directive(g.node('Directive', { handler: g.optional(g.js.code) })),
		},
		other: g.directive(g.node('Directive', { exp: g.optional(g.js.expression) })),
	},

	expression: g.node('Interpolation', { content: g.js.expression }),
});
