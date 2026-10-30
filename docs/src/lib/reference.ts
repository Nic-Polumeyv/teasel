import { Source } from '@teasel/parser';
import entry from '../../../npm/dist/index.d.ts?raw';
import options from '../../../npm/dist/options.d.ts?raw';
import types from '../../../npm/dist/types.d.ts?raw';
import grammar from '../../../npm/dist/grammar.d.ts?raw';
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
	const source = new Source(text, { sourceType: 'module', typescript: true, comments: true });
	return (source.parse() as unknown as { node: { body: Declaration[] } }).node.body;
};

// the slot that types a piece's answer is no part of the surface; a function's overloads are one item
const item = (text: string, statements: Declaration[]) => {
	const doc = statements[0].leadingComments?.at(-1);
	const code = statements.map((statement) => text.slice(statement.start, statement.end)).join('\n');
	return `## ${name(statements[0])}\n\n${doc ? prose(doc.value) + '\n\n' : ''}\`\`\`ts\n${code.replace(/\n[^\n]*\[(?:answers|LANGUAGE|PIECE)\][^\n]*/g, '')}\n\`\`\``;
};
const grouped = (statements: Declaration[]) =>
	statements.reduce<Declaration[][]>((groups, statement) => {
		const last = groups.at(-1);
		if (last !== undefined && name(last[0]) === name(statement)) last.push(statement);
		else groups.push([statement]);
		return groups;
	}, []);

// the entry declares the surface, and names what it takes from options.ts and types.ts
function parser() {
	const surface = body(entry);
	const named = new Set(surface.flatMap((statement) => (statement.type === 'ExportNamedDeclaration' && statement.declaration == null ? statement.specifiers!.map((specifier) => specifier.exported.name) : [])));
	const taken = (text: string) => grouped(body(text).filter((statement) => exported(statement) && named.has(name(statement)))).map((statements) => item(text, statements));
	const markdown = [...taken(options), ...grouped(surface.filter(exported)).map((statements) => item(entry, statements)), ...taken(types)].join('\n\n');
	return { meta: { href: '/reference/parser', title: '@teasel/parser', section: 'Reference', path: 'npm/src/index.ts' }, markdown: `Every export of the package, as its declarations say.\n\n${markdown}` };
}

function builders() {
	const markdown = grouped(body(grammar).filter(exported)).map((statements) => item(grammar, statements)).join('\n\n');
	return { meta: { href: '/reference/grammar', title: '@teasel/parser/grammar', section: 'Reference', path: 'npm/src/grammar.ts' }, markdown: `Every export of the grammar builders, as their declarations say. A grammar crosses to the engine as \`JSON.stringify({ name, definition })\`, which is also what the Rust crate's \`parse_document\` and the command line's \`--host\` read.\n\n${markdown}` };
}

// the engine's enum: each code's doc comment says what it means, its name is the variant's in snake case
function codes() {
	const rows = [...errors.matchAll(/^((?:\t\/\/\/ .*\n)+)\t(\w+),$/gm)].map(([, doc, variant]) => `| \`${variant.replace(/(?<!^)[A-Z]/g, '_$&').toLowerCase()}\` | ${doc.replace(/^\t\/\/\/ /gm, '').trim().replaceAll('\n', ' ').replaceAll('|', '\\|')} |`);
	return { meta: { href: '/reference/error-codes', title: 'Error codes', section: 'Reference', path: 'crates/teasel/src/error.rs' }, markdown: `Every \`code\` an error can have and what it means. \`args\` holds what the error is about when the code says so, and \`message\` is the code followed by each argument in quotes: \`expected '}'\`.\n\n| code | meaning |\n| --- | --- |\n${rows.join('\n')}` };
}

export const references = [parser(), codes(), builders()];
