import { Source } from '@teasel/parser';
import types from '../../../npm/dist/lib/api.d.ts?raw';

type Declaration = { type: string; start: number; end: number; id?: { name: string }; declarations?: { id: { name: string } }[]; declaration?: Declaration; leadingComments?: { value: string; end: number }[] };

const prose = (comment: string) =>
	comment
		.split('\n')
		.map((line) => line.replace(/^\s*\*+\s?/, ''))
		.join('\n')
		.trim();

function parser() {
	const source = new Source(types, { sourceType: 'module', typescript: true, comments: true });
	try {
		const { node } = source.parse() as unknown as { node: { body: Declaration[] } };
		const body = node.body
			.filter((statement) => statement.type === 'ExportNamedDeclaration' && statement.declaration && (statement.declaration.id ?? statement.declaration.declarations?.[0]?.id))
			.map((statement) => {
				const doc = statement.leadingComments?.at(-1);
				const text = types.slice(statement.start, statement.end);
				return `## ${(statement.declaration!.id ?? statement.declaration!.declarations![0].id).name}\n\n${doc ? prose(doc.value) + '\n\n' : ''}\`\`\`ts\n${text}\n\`\`\``;
			})
			.join('\n\n');
		return { meta: { href: '/reference/parser', title: '@teasel/parser', section: 'Reference', path: 'npm/src/lib/api.ts' }, markdown: `Every export of the package, as its declarations say.\n\n${body}` };
	} finally {
		source[Symbol.dispose]();
	}
}

export const reference = parser();
