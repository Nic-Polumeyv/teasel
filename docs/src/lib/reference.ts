import { Source } from '@teasel/parser';
import entry from '../../../npm/dist/index.d.ts?raw';
import options from '../../../npm/dist/options.d.ts?raw';
import types from '../../../npm/dist/types.d.ts?raw';
import grammar from '../../../npm/dist/grammar.d.ts?raw';
import wire from '../../../npm/src/wire.ts?raw';
import errors from '../../../crates/teasel/src/error.rs?raw';

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

// the entry declares the surface, and names what it takes from options.ts and types.ts
function parser() {
	const surface = body(entry);
	const named = new Set(surface.flatMap((statement) => (statement.type === 'ExportNamedDeclaration' && statement.declaration == null ? statement.specifiers!.map((specifier) => specifier.exported.name) : [])));
	const taken = (text: string) => body(text).filter((statement) => exported(statement) && named.has(name(statement))).map((statement) => item(text, statement));
	const markdown = [...taken(options), ...surface.filter(exported).map((statement) => item(entry, statement)), ...taken(types)].join('\n\n');
	return { meta: { href: '/reference/parser', title: '@teasel/parser', section: 'Reference', path: 'npm/src/index.ts' }, markdown: `Every export of the package, as its declarations say.\n\n${markdown}` };
}

function builders() {
	const markdown = body(grammar).filter(exported).map((statement) => item(grammar, statement)).join('\n\n');
	return { meta: { href: '/reference/grammar', title: '@teasel/parser/grammar', section: 'Reference', path: 'npm/src/grammar.ts' }, markdown: `Every export of the grammar builders, as their declarations say.\n\n${markdown}` };
}

// the wire's types stay inside their module; the page shows them all, as the engine reads them
function crossing() {
	const declared = (statement: Declaration) => (statement.type === 'ExportNamedDeclaration' ? statement : ({ ...statement, type: 'ExportNamedDeclaration', declaration: statement } as Declaration));
	const markdown = body(wire).map(declared).filter(exported).map((statement) => item(wire, statement)).join('\n\n');
	return { meta: { href: '/reference/wire', title: 'Grammar wire', section: 'Reference', path: 'npm/src/wire.ts' }, markdown: `What a grammar's \`wire\` holds and the Rust crate's \`parse_document\` and the command line's \`--host\` read: the definition as words in the order these types declare them, then a pool of strings, as \`Writer\` lays them out.\n\n${markdown}` };
}

// the engine's table, one row per name: the codes that share a name share its row
function codes() {
	const rows = new Map<string, string[]>();
	for (const [, name, message] of errors.matchAll(/^\t\w+ "(\w+)" => "((?:[^"\\]|\\.)*)",$/gm)) {
		rows.set(name, [...(rows.get(name) ?? []), message.replace(/\\(.)/g, '$1')]);
	}
	const cell = (message: string) => (message === '' ? 'Names the offset or option the request cannot take' : message.replaceAll('{}', '…'));
	const table = [...rows].map(([name, messages]) => `| \`${name}\` | ${messages.map(cell).join('<br>')} |`).join('\n');
	return { meta: { href: '/reference/error-codes', title: 'Error codes', section: 'Reference', path: 'crates/teasel/src/error.rs' }, markdown: `Every \`code\` an error can have, with its messages. \`…\` is the name or text the error is about.\n\n| code | message |\n| --- | --- |\n${table}` };
}

export const references = [parser(), codes(), builders(), crossing()];
