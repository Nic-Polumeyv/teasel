// `node scripts/children.ts` writes src/children.ts from the engine's layout: for every node type, the fields that hold nodes; the tests check they agree
import { writeFileSync } from 'node:fs';
import { holds_nodes, resolve, type Layout } from '../src/decode.ts';

/** From the layout the engine hands over: each type with the keys that hold nodes or lists of nodes, and the keys TypeScript may add to any node. */
export function generate(layout: Layout): { children: Record<string, string[]>; extras: string[]; stylesheet: Record<string, string[]> } {
	const extra_fields = layout.extras?.fields ?? [];
	const children: Record<string, string[]> = {};
	for (const [name, raw] of layout.recipes.kinds) {
		const ops = resolve(raw, layout.kinds.find((kind) => kind.name === name)!.fields, extra_fields);
		const keys = ops.filter(holds_nodes).map((op) => {
			if (op.op === 'object') throw new Error(`${op.key} nests nodes`);
			return op.key;
		});
		// the types the recipe spells: one, or one per name of an enum field
		for (const type of ops.flatMap((op) => (op.op === 'type' ? [op.key] : op.op === 'typeof' ? op.names : []))) {
			const list = (children[type] ??= []);
			for (const key of keys) if (!list.includes(key)) list.push(key);
		}
	}
	const adds = (layout.recipes.adds ?? []).flatMap(([, ops]) => ops);
	const extras: string[] = [];
	for (const field of extra_fields) {
		const add = adds.find(([op, , path]) => op === 'maybe' && path === field.name);
		if (add !== undefined && (field.ty === '?node' || field.ty === '?list')) extras.push(add[1]);
	}
	return { children, extras, stylesheet: layout.stylesheet };
}

if (import.meta.main) {
	const { engine } = await import('../dist/engine/node.js');
	const { children, extras, stylesheet } = generate(JSON.parse(engine.layout()));
	const lines = (table: Record<string, string[]>) => Object.entries(table).map(([type, fields]) => `\t${type}: [${fields.map((f) => `'${f}'`).join(', ')}],`).join('\n');
	writeFileSync(
		new URL('../src/children.ts', import.meta.url),
		`// written by scripts/children.ts from the engine's layout\n/** For every type of node a built-in language answers with, the fields that hold a node or a list of nodes: what a walk follows. A grammar's \`children\` adds the host's. */\nexport const children = frozen({\n${lines(children)}\n});\n\n/** For every type of node a stylesheet holds, the fields that hold a node or a list of nodes. */\nexport const stylesheet = frozen({\n${lines(stylesheet)}\n});\n\n/** The fields TypeScript may add to a node of any type, holding nodes: annotations, type parameters and arguments, what a class implements, decorators. */\nexport const extras = Object.freeze([${extras.map((f) => `'${f}'`).join(', ')}]);\n\nexport function frozen(table: Readonly<Record<string, readonly string[]>>) {\n\tfor (const fields of Object.values(table)) Object.freeze(fields);\n\treturn Object.freeze(table);\n}\n`,
	);
	console.log(`${Object.keys(children).length} types, ${Object.keys(stylesheet).length} stylesheet types, ${extras.length} extras`);
}
