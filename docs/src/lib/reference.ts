import { Source } from '@teasel/parser';
import entry from '../../../npm/dist/index.d.ts?raw';
import options from '../../../npm/dist/lib/options.d.ts?raw';

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

const item = (text: string, statement: Declaration) => {
	const doc = statement.leadingComments?.at(-1);
	return `## ${name(statement)}\n\n${doc ? prose(doc.value) + '\n\n' : ''}\`\`\`ts\n${text.slice(statement.start, statement.end)}\n\`\`\``;
};

// the entry declares the surface, and names what it takes from options.ts
function parser() {
	const surface = body(entry);
	const named = new Set(surface.flatMap((statement) => (statement.type === 'ExportNamedDeclaration' && statement.declaration == null ? statement.specifiers!.map((specifier) => specifier.exported.name) : [])));
	const markdown = [
		...body(options).filter((statement) => exported(statement) && named.has(name(statement))).map((statement) => item(options, statement)),
		...surface.filter(exported).map((statement) => item(entry, statement)),
	].join('\n\n');
	return { meta: { href: '/reference/parser', title: '@teasel/parser', section: 'Reference', path: 'npm/src/index.ts' }, markdown: `Every export of the package, as its declarations say.\n\n${markdown}` };
}

export const reference = parser();
