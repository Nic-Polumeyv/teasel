import * as g from '@teasel/parser/grammar';

export const tpl = g.grammar('tpl', {
	document: { node: 'Template' },
	elements: {
		component: { node: 'Component' },
		other: { node: 'Element' },
	},
	constructs: {
		Expression: {
			in: ['content', 'value'],
			open: { marker: ['{{'], form: [{ expression: g.js.expression }, '}}'] },
		},
		RepeatBlock: {
			open: {
				marker: ['{{', '#repeat'],
				space: true,
				form: [
					{ item: g.bind(g.js.pattern) },
					g.opt(',', { index: g.optional(g.bind(g.js.identifier)) }),
					'in',
					{ list: g.js.expression },
					g.opt('by', { key: g.optional(g.js.expression) }),
					'}}',
					{ body: g.content },
				],
			},
			branches: [{ marker: ['{{', ':empty'], form: ['}}', { fallback: g.optional(g.content) }] }],
			close: { marker: ['{{', '/repeat'], form: ['}}'] },
		},
	},
});
