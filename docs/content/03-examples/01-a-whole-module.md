---
title: Three modules
---

This example takes a small program of three files and answers three questions about it:

1. Which names does each file import from the other two?
2. Where is every export used, across the whole program?
3. Which functions change the score?

The parser reads one file at a time and knows nothing about the other files. Each parse lists what the file declares and what it uses. Your code connects the files through their `import` and `export` statements.

Everything here builds on [Scopes](/scopes), which explains bindings and references. Read that page first if they are new.

The program is a text adventure: the map, the player, the game. Click a module to read it.

![three module cards, world, player and game, with an arrow from each module to the one it imports from; each card opens the file](Modules.svelte "world.js player.js game.js")

## 1. Parse each file

```js link-modules.js
import { readFileSync } from 'node:fs';
import { Source, referenceOf, parentOf, scopeOf } from '@teasel/parser';

const modules = new Map();
for (const name of ['world.js', 'player.js', 'game.js']) {
	const text = readFileSync(`adventure/${name}`, 'utf8');
	using source = new Source(text, { sourceType: 'module', scopes: true });
	modules.set(name, { name, text, ...source.parse() });
}

const lineOf = (module, node) =>
	module.text.slice(0, node.start).split('\n').length;
```
```notes
using :: Releases the `Source` at the end of each loop turn. The answer stays usable, because it is plain objects that do not depend on the `Source`. See [Getting started](/getting-started#parse-it-again).
scopes: true :: Adds `scopes`, `bindings` and `references` to the answer. Without it the answer is only the tree. See [The answer](/the-answer#what-an-option-adds).
...source.parse() :: The answer is an object: `node` is the tree, and with `scopes` on it also has `scopes`, `bindings` and `references`. Spreading it puts those on the module record, so `module.node` and `module.bindings` below come from here.
lineOf :: A helper of this example, not part of the package. Every node has `start`, its offset in the file's text. Counting the newlines before that offset gives the line.
```

One `Source` holds one text, so three files are three `Source`s.

## 2. What each file exports

`export const x` and `export function f` are statements in the tree, of type `ExportNamedDeclaration`. The declaration inside has the identifier that names the export.

```js link-modules.js
function exportsOf(module) {
	const exports = new Map();
	for (const statement of module.node.body) {
		if (statement.type !== 'ExportNamedDeclaration' || !statement.declaration) continue;
		const { declaration } = statement;
		const ids = declaration.type === 'VariableDeclaration'
			? declaration.declarations.map((declarator) => declarator.id)
			: [declaration.id];
		for (const id of ids) exports.set(id.name, referenceOf(id));
	}
	return exports;
}

for (const module of modules.values()) module.exports = exportsOf(module);
```
```notes
module.node.body :: `node` is the `Program`, and `body` is its top-level statements, in ESTree shape.
declaration.type === 'VariableDeclaration' :: `export const a = 1, b = 2` declares several names, one per declarator. A function or a class declares one, in `declaration.id`.
referenceOf(id) :: Asks what the identifier refers to. On the identifier that declares a name, the answer is the binding itself. See [Three questions](/scopes#three-questions).
```

Each file now has `exports`, a map from an exported name to its binding. This reads `export const`, `export function` and `export class`. A program that also uses `export { a, b }` or `export default` needs those two statement types handled the same way.

## 3. Match each import to the export it names

An import statement names a file and some names. When the file is one of the three, each imported name is looked up in that file's `exports`.

```js link-modules.js
const importedFrom = new Map();

for (const module of modules.values()) {
	for (const statement of module.node.body) {
		if (statement.type !== 'ImportDeclaration') continue;
		const from = modules.get(statement.source.value.replace('./', ''));
		if (!from) continue;
		for (const specifier of statement.specifiers) {
			const local = referenceOf(specifier.local);
			const binding = from.exports.get(specifier.imported.name);
			importedFrom.set(local, { module: from, binding });
		}
	}
}
```
```notes
importedFrom :: The link between files: a map from an import's binding in one file to the export's binding in another. Both are objects from the answers, so the map connects two parses.
statement.source.value :: The string after `from`, `'./world.js'`. This example resolves it by dropping `./`; a real tool resolves paths as its module system does.
if (!from) continue :: `./dice.js` and `node:readline/promises` are not among the three files, so those imports stay unlinked.
specifier.local :: The name the importing file uses. `specifier.imported` is the name the other file exported. They differ in `import { setTimeout as sleep }`; in this program they are the same.
```

With the map in place, the first question has an answer. `usesOf` counts the references in a file that point at a binding.

```js link-modules.js
const usesOf = (module, binding) =>
	module.references.filter((reference) => reference.binding === binding).length;

for (const module of modules.values()) {
	const imports = module.bindings.filter((binding) => binding.kind === 'import');
	for (const local of imports) {
		const target = importedFrom.get(local);
		const where = target
			? `${target.binding.kind} in ${target.module.name} line ${lineOf(target.module, target.binding.node)}, used ${usesOf(module, local)}x here`
			: 'from outside the program';
		console.log(`${module.name.padEnd(10)} ${local.name.padEnd(16)} ${where}`);
	}
}
```
```notes
module.references :: Every use of a name in the file, in source order. Each has `binding`, the declaration it refers to. See [What a reference knows](/scopes#what-a-reference-knows).
binding.kind === 'import' :: Every binding has a `kind`: `import`, `const`, `let`, `function`, `class`, `param` and more. The full list is in the [reference](/reference/parser#binding).
target.binding.node :: The identifier that declared the export, in the other file. Its `start` is an offset into that file's text, so the line is computed against that file.
```

```text
world.js   describe         from outside the program
world.js   say              from outside the program
player.js  rooms            const in world.js line 6, used 1x here
game.js    roll             from outside the program
game.js    say              from outside the program
game.js    createInterface  from outside the program
game.js    DIRECTIONS       const in world.js line 3, used 1x here
game.js    LAMP_LIFE        const in world.js line 4, used 2x here
game.js    look             function in world.js line 31, used 3x here
game.js    restock          function in world.js line 40, used 1x here
game.js    Player           class in player.js line 5, used 1x here
```

## 4. Every export, everywhere

An export is used in two places: in its own file, and in every file that imports it. The second kind is found by going through `importedFrom` for the imports that point at it.

```js link-modules.js
for (const module of modules.values()) {
	for (const [name, binding] of module.exports) {
		const elsewhere = [];
		for (const [local, target] of importedFrom) {
			if (target.binding !== binding) continue;
			const importer = [...modules.values()].find((m) => m.bindings.includes(local));
			elsewhere.push(`${importer.name} ${usesOf(importer, local)}x`);
		}
		const here = usesOf(module, binding);
		console.log(`${module.name.padEnd(10)} ${name.padEnd(11)} used here ${here}x; ${elsewhere.join(', ') || 'imported nowhere'}`);
	}
}
```

```text
world.js   DIRECTIONS  used here 0x; game.js 1x
world.js   LAMP_LIFE   used here 0x; game.js 2x
world.js   rooms       used here 5x; player.js 1x
world.js   look        used here 0x; game.js 3x
world.js   restock     used here 0x; game.js 1x
player.js  Player      used here 0x; game.js 1x
game.js    parse       used here 1x; imported nowhere
game.js    step        used here 1x; imported nowhere
game.js    won         used here 2x; imported nowhere
game.js    play        used here 0x; imported nowhere
game.js    reset       used here 0x; imported nowhere
```

`game.js` is where the program starts, so no other file imports its exports. An export in any other file with no uses here and no importer would be dead code.

## 5. Which functions change the score

`game.js` keeps the score, the turn count and the lamp in three top-level `let`s. A reference has `write` set when it assigns to its binding, and `scope`, the scope it sits in. Walking `scope.parent` upward reaches the function the assignment is in.

```js link-modules.js
const game = modules.get('game.js');

function functionAround(reference) {
	for (let scope = reference.scope; scope; scope = scope.parent) {
		if (scope.kind === 'function') return scope.node;
	}
	return null;
}

function nameOf(fn) {
	if (fn.id) return fn.id.name;
	const parent = parentOf(fn);
	if (parent.type === 'Property' || parent.type === 'MethodDefinition') return parent.key.name;
	if (parent.type === 'VariableDeclarator') return parent.id.name;
	return '(anonymous)';
}

const state = game.bindings.filter(
	(binding) => binding.kind === 'let' && binding.scope === scopeOf(game.node),
);

for (const binding of state) {
	const writes = game.references.filter(
		(reference) => reference.binding === binding && reference.write,
	);
	const where = writes.map((reference) => `${nameOf(functionAround(reference))}:${lineOf(game, reference.node)}`);
	console.log(`${binding.name.padEnd(10)} ${where.join(', ')}`);
}
```
```notes
scope.parent :: Every scope has the scope around it in `parent`; the outermost has `null`. See the [reference](/reference/parser#scope).
scope.kind === 'function' :: A scope's `kind` says what opened it: a function, a block, a class, the module. `scope.node` is the node that did, here the function.
parentOf(fn) :: The node this one hangs from. An arrow function has no name of its own, so its name is on its parent: the property it is the value of, or the variable it is assigned to. See [Three questions](/scopes#three-questions).
scopeOf(game.node) :: `scopeOf` gives the scope a node opens. `game.node` is the `Program`, which opens the file's outermost scope, so a binding whose `scope` is that one was declared at the top level.
reference.write :: True when the reference assigns: `score = 0`, `score += 5`, `turns++`. The other flags are `read`, `mutate` and `declares`.
```

```text
turns      step:50, reset:77
score      eat:21, act:28, reset:78
lampTurns  step:52, reset:79
```

The write at line 21 is `score += roll(6)`, inside an arrow function stored as a property: `eat: (player, item) => { … }`. An arrow function has no `id`, so `nameOf` reads the name from the property that holds it.

## Related

- [Scopes](/scopes) has everything a binding, a reference and a scope carry.
- [A component, piece by piece](/a-component) connects names across parses of one file instead of across files.
