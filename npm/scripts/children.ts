// `node scripts/children.ts` writes src/children.ts from the engine's layout: for every node type, the fields that hold nodes; the tests check they agree
import { writeFileSync } from 'node:fs';

interface Field {
	name: string;
	ty: string;
	fields?: Field[];
}
type Op = [string, ...any[]];
type Recipes = [string, Op[]][];
interface Layout {
	kinds: { name: string; fields: Field[] }[];
	ts?: { kinds: { name: string; fields: Field[] }[] };
	extras?: { fields: Field[] };
	recipes: { js: Recipes; ts?: Recipes; extras?: Recipes };
}

/** From the layout the engine hands over: each type with the keys that hold nodes or lists of nodes, and the keys TypeScript may add to any node. */
export function generate(layout: Layout): { children: Record<string, string[]>; extras: string[] } {
	const find = (fields: Field[], path: string): Field => {
		const [head, rest] = path.split('.');
		const field = fields.find((f) => f.name === head)!;
		return rest === undefined ? field : field.fields!.find((f) => f.name === rest)!;
	};
	// the keys whose value is a node or a list of nodes, and the types the recipe spells: one, or one per name of an enum field
	const keys = (ops: Op[], fields: Field[]) => {
		const out: string[] = [];
		let types: string[] = [];
		for (const [op, key, path] of ops) {
			if (op === 'type') types = [key];
			else if (op === 'typeof') types = (find(fields, key) as Field & { names: string[] }).names;
			else if (op === 'node' || op === 'list' || op === 'optlistkey' || op === 'params' || op === 'othername') out.push(key);
			else if ((op === 'opt' || op === 'optkey') && find(fields, path).ty === '?node') out.push(key);
			else if (op === 'object' && keys(path, fields).out.length !== 0) throw new Error(`${key} nests nodes`);
		}
		return { types, out };
	};
	const children: Record<string, string[]> = {};
	const add = (type: string, fields: string[]) => {
		const list = (children[type] ??= []);
		for (const field of fields) if (!list.includes(field)) list.push(field);
	};
	const kinds = (recipes: Recipes, of: { name: string; fields: Field[] }[]) => {
		for (const [name, ops] of recipes) {
			const { types, out } = keys(ops, of.find((kind) => kind.name === name)!.fields);
			for (const type of types) add(type, out);
		}
	};
	kinds(layout.recipes.js, layout.kinds);
	if (layout.ts !== undefined) kinds(layout.recipes.ts!, layout.ts.kinds);
	const extras = layout.extras === undefined ? [] : keys(layout.recipes.extras!.find(([name]) => name === 'extras')![1], layout.extras.fields).out;
	return { children, extras };
}

if (import.meta.main) {
	const { engine } = await import('../dist/engine/native.js');
	const { children, extras } = generate(JSON.parse(engine.layout()));
	const lines = Object.entries(children).map(([type, fields]) => `\t${type}: [${fields.map((f) => `'${f}'`).join(', ')}],`);
	writeFileSync(
		new URL('../src/children.ts', import.meta.url),
		`// written by scripts/children.ts from the engine's layout\n/** For every type of node a built-in plan answers with, the fields that hold a node or a list of nodes: what a walk follows. A document plan's \`children\` adds the host's. */\nexport const children = frozen({\n${lines.join('\n')}\n} as const);\n\n/** The fields TypeScript may add to a node of any type, holding nodes: annotations, type parameters and arguments, what a class implements, decorators. */\nexport const extras = Object.freeze([${extras.map((f) => `'${f}'`).join(', ')}] as const);\n\nexport function frozen<T extends Readonly<Record<string, readonly string[]>>>(table: T): T {\n\tfor (const fields of Object.values(table)) Object.freeze(fields);\n\treturn Object.freeze(table);\n}\n`,
	);
	console.log(`${Object.keys(children).length} types, ${extras.length} extras`);
}
