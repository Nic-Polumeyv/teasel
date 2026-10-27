// `node scripts/children.ts` writes src/children.ts from the engine's layout: for every node type, the fields that hold nodes; the tests check they agree
import { writeFileSync } from 'node:fs';
import { field as find, type Field, type Kind, type Layout, type RawOp as Op, type RawRecipes as Recipes } from '../src/decode.ts';

/** From the layout the engine hands over: each type with the keys that hold nodes or lists of nodes, and the keys TypeScript may add to any node. */
export function generate(layout: Layout): { children: Record<string, string[]>; extras: string[]; stylesheet: Record<string, string[]> } {
	// the keys whose value is a node or a list of nodes, and the types the recipe spells: one, or one per name of an enum field
	const keys = (ops: Op[], fields: Field[]) => {
		const out: string[] = [];
		let types: string[] = [];
		for (const [op, key, path] of ops) {
			if (op === 'type') types = [key];
			else if (op === 'typeof') types = find(fields, key).names!;
			else if (op === 'node' || op === 'list' || op === 'optlistkey' || op === 'othername') out.push(key);
			else if ((op === 'opt' || op === 'maybe') && find(fields, path).ty === '?node') out.push(key);
			else if (op === 'maybe' && find(fields, path).ty === '?list') out.push(key);
			else if (op === 'object' && keys(path, fields).out.length !== 0) throw new Error(`${key} nests nodes`);
		}
		return { types, out };
	};
	const children: Record<string, string[]> = {};
	const add = (type: string, fields: string[]) => {
		const list = (children[type] ??= []);
		for (const field of fields) if (!list.includes(field)) list.push(field);
	};
	const kinds = (recipes: Recipes, of: Kind[]) => {
		for (const [name, ops] of recipes) {
			const { types, out } = keys(ops, of.find((kind) => kind.name === name)!.fields);
			for (const type of types) add(type, out);
		}
	};
	kinds(layout.recipes.js, layout.kinds);
	if (layout.ts !== undefined) kinds(layout.recipes.ts!, layout.ts.kinds);
	const adds = [...(layout.recipes.adds ?? []), ...(layout.recipes.ts_adds ?? [])].flatMap(([, ops]) => ops);
	const extras: string[] = [];
	for (const field of layout.extras?.fields ?? []) {
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
		`// written by scripts/children.ts from the engine's layout\n/** For every type of node a built-in plan answers with, the fields that hold a node or a list of nodes: what a walk follows. A document plan's \`children\` adds the host's. */\nexport const children = frozen({\n${lines(children)}\n});\n\n/** For every type of node a stylesheet holds, the fields that hold a node or a list of nodes. */\nexport const stylesheet = frozen({\n${lines(stylesheet)}\n});\n\n/** The fields TypeScript may add to a node of any type, holding nodes: annotations, type parameters and arguments, what a class implements, decorators. */\nexport const extras = Object.freeze([${extras.map((f) => `'${f}'`).join(', ')}]);\n\nexport function frozen(table: Readonly<Record<string, readonly string[]>>) {\n\tfor (const fields of Object.values(table)) Object.freeze(fields);\n\treturn Object.freeze(table);\n}\n`,
	);
	console.log(`${Object.keys(children).length} types, ${Object.keys(stylesheet).length} stylesheet types, ${extras.length} extras`);
}
