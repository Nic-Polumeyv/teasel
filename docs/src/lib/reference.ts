import { Source } from '@teasel/parser';
import types from '../../../npm/dist/lib/api.d.ts?raw';
import entry from '../../../npm/dist/native.d.ts?raw';

type Declaration = { type: string; start: number; end: number; id?: { name: string }; declarations?: { id: { name: string } }[]; declaration?: Declaration | null; specifiers?: { exported: { name: string } }[]; leadingComments?: { value: string; end: number }[] };
const name = (statement: Declaration) => (statement.declaration!.id ?? statement.declaration!.declarations![0].id).name;
const exported = (statement: Declaration) => statement.type === 'ExportNamedDeclaration' && statement.declaration != null && (statement.declaration.id ?? statement.declaration.declarations?.[0]?.id) !== undefined;

const prose = (comment: string) =>
	comment
		.split('\n')
		.map((line) => line.replace(/^\s*\*+\s?/, ''))
		.join('\n')
		.trim();

const body = (text: string) => {
	using source = new Source(text, { sourceType: 'module', typescript: true, comments: true });
	return (source.parse() as unknown as { node: { body: Declaration[] } }).node.body;
};

// the entries re-export api.ts's types wholesale and its values by name: `Source` is theirs, with the engine bound
function parser() {
	const values = new Set(body(entry).flatMap((statement) => (exported(statement) ? [name(statement)] : statement.type === 'ExportNamedDeclaration' ? statement.specifiers!.map((specifier) => specifier.exported.name) : [])));
	const markdown = body(types)
		.filter((statement) => exported(statement) && (values.has(name(statement)) || statement.declaration!.type === 'TSInterfaceDeclaration' || statement.declaration!.type === 'TSTypeAliasDeclaration'))
		.map((statement) => {
			const doc = statement.leadingComments?.at(-1);
			const text = types.slice(statement.start, statement.end).replace('constructor(engine: Engine, ', 'constructor(');
			return `## ${name(statement)}\n\n${doc ? prose(doc.value) + '\n\n' : ''}\`\`\`ts\n${text}\n\`\`\``;
		})
		.join('\n\n');
	return { meta: { href: '/reference/parser', title: '@teasel/parser', section: 'Reference', path: 'npm/src/lib/api.ts' }, markdown: `Every export of the package, as its declarations say.\n\n${markdown}` };
}

export const reference = parser();
