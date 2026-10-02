import * as g from '@teasel/parser/grammar';

export const tpl = g.grammar('tpl', {
	document: g.node('Template', { children: g.content }),
	text: g.node('Text', { data: g.text.data }),
	comment: g.node('Comment', { data: g.text.data }),
	delimiters: ['{{', '}}'],
	attributes: { expressions: true },
	elements: {
		fields: {
			name: g.element.tag,
			attributes: g.element.attributes,
			children: g.content,
		},
		component: g.element(g.node('Component')),
		other: g.element(g.node('Element')),
	},
	sigils: {
		open: '#',
		branch: ':',
		close: '/',
		tag: '@',
		blocks: {
			repeat: g.block(
				g.node(
					'RepeatBlock',
					{ item: g.bind(g.js.pattern) },
					g.opt(',', { index: g.optional(g.bind(g.js.identifier)) }),
					'in',
					{ list: g.js.expression },
					g.opt('by', { key: g.optional(g.js.expression) }),
					{ body: g.content },
				),
				{ branches: { empty: [{ fallback: g.optional(g.content) }] } },
			),
		},
	},
	expression: g.node('Expression', { expression: g.js.expression }),
});
