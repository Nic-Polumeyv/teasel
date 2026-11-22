// `node scripts/children.ts` writes src/children.ts from the engine's layout: for every node type, the fields that hold nodes; the tests check they agree
import { writeFileSync } from 'node:fs';
import { holds_nodes, operations, type Layout } from '../src/decode.ts';

/** From the layout the engine hands over: each type with the keys that hold nodes or lists of nodes, and the keys TypeScript may add to any node. */
export function generate(layout: Layout): { children: Record<string, string[]>; extras: string[]; svelte: Record<string, string[]> } {
	const extra_fields = layout.extras?.fields ?? [];
	// Svelte's root lists the scripts it never has, `js`, and its comments are not its children
	const table = (kinds: Layout['recipes']['kinds'], svelte = false) => {
		const children: Record<string, string[]> = {};
		for (const raw of kinds) {
			const ops = operations(raw);
			const keys = ops.filter((op) => (svelte ? op.op === 'emptylist' || (holds_nodes(op) && op.op !== 'comments') : holds_nodes(op))).map((op) => {
				if (op.op === 'object') throw new Error(`${op.key} nests nodes`);
				return op.key;
			});
			// the types the recipe spells: one, or one per name of an enum field
			for (const type of ops.flatMap((op) => (op.op === 'type' || op.op === 'bare' ? [op.key] : op.op === 'typeof' ? op.names : []))) {
				const list = (children[type] ??= []);
				for (const key of keys) if (!list.includes(key)) list.push(key);
			}
		}
		return children;
	};
	// JavaScript's kinds, then Svelte's from its first on
	const first = layout.kinds.indexOf('Fragment');
	const children = table(layout.recipes.kinds.slice(0, first));
	const svelte = table(layout.recipes.kinds.slice(first), true);
	const adds = (layout.recipes.adds ?? []).flat();
	const extras: string[] = [];
	for (const field of extra_fields) {
		const add = adds.find(([op, , at]) => op === 'maybe' && at === field.at);
		if (add !== undefined && (field.ty === '?node' || field.ty === '?list')) extras.push(add[1]);
	}
	return { children, extras, svelte };
}

if (import.meta.main) {
	const { engine } = await import('../dist/engine/node.js');
	const { children, extras, svelte } = generate(JSON.parse(engine.layout()));
	const lines = (table: Record<string, string[]>) => Object.entries(table).map(([type, fields]) => `\t${type}: [${fields.map((f) => `'${f}'`).join(', ')}],`).join('\n');
	writeFileSync(
		new URL('../src/children.ts', import.meta.url),
		`// written by scripts/children.ts from the engine's layout\n/** For every type of node JavaScript answers with, the fields that hold a node or a list of nodes: what a walk follows. */\nexport const children = frozen({\n${lines(children)}\n});\n\n/** For every type of node a Svelte component or a stylesheet holds, JavaScript's among them, the fields that hold a node or a list of nodes. */\nexport const svelte = frozen({\n\t...children,\n${lines(svelte)}\n});\n\n/** The fields TypeScript may add to a node of any type, holding nodes: annotations, type parameters and arguments, what a class implements, decorators. */\nexport const extras = Object.freeze([${extras.map((f) => `'${f}'`).join(', ')}]);\n\nexport function frozen(table: Readonly<Record<string, readonly string[]>>) {\n\tfor (const fields of Object.values(table)) Object.freeze(fields);\n\treturn Object.freeze(table);\n}\n`,
	);
	console.log(`${Object.keys(children).length} types, ${Object.keys(svelte).length} Svelte types, ${extras.length} extras`);
}
