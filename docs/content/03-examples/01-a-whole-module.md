---
title: Three modules
---

A text adventure in three files: the map, the player, the game. Each is parsed on its own, and then the three answers are linked through what they import from each other. Click a module to read it.

![three module cards, world, player and game, with an arrow from each module to the one it imports from; each card opens the file](Modules.svelte "world.js player.js game.js")

## Parse each file

One `Source` per file, one answer per file, kept in a map by name. `line` turns a node's `start` into a line number in its own file.

```js
import { readFileSync } from 'node:fs';
import { Source, referenceOf, parentOf } from '@teasel/parser';

const modules = new Map();
for (const name of ['world.js', 'player.js', 'game.js']) {
	const text = readFileSync(`adventure/${name}`, 'utf8');
	const answer = new Source(text, { sourceType: 'module', scopes: true }).parse();
	modules.set(name, { name, text, ...answer, line: (n) => text.slice(0, n.start).split('\n').length });
}
```

## What each module exports

An export statement is in the tree. The name it exports is a binding, and `referenceOf` on the declaring identifier is that binding.

```js
const exported = (m) => {
	const out = new Map();
	for (const s of m.node.body) {
		if (s.type !== 'ExportNamedDeclaration' || !s.declaration) continue;
		const d = s.declaration;
		const ids = d.type === 'VariableDeclaration' ? d.declarations.map((x) => x.id) : [d.id];
		for (const id of ids) out.set(id.name, referenceOf(id));
	}
	return out;
};
for (const m of modules.values()) m.exports = exported(m);
```

## Link the imports

An import statement names a file and some names. When the file is one of ours, each name resolves to the exporting module's binding. The local binding gets an `imported` property pointing at it: that's the link, one object to another, across two answers.

```js
for (const m of modules.values()) {
	for (const s of m.node.body) {
		if (s.type !== 'ImportDeclaration') continue;
		const from = modules.get(s.source.value.replace('./', ''));
		for (const spec of s.specifiers) {
			const local = referenceOf(spec.local);
			local.imported = from ? from.exports.get(spec.imported.name) ?? null : undefined;
			const uses = m.references.filter((r) => r.binding === local).length;
			if (from) console.log(`${m.name.padEnd(10)} ${spec.local.name.padEnd(11)} <- ${from.name.padEnd(10)} ${local.imported.kind.padEnd(8)} line ${from.line(local.imported.node)}, used ${uses}x here`);
			else console.log(`${m.name.padEnd(10)} ${spec.local.name.padEnd(11)} <- ${s.source.value} (not ours)`);
		}
	}
}
```

```text
world.js   describe    <- ./narrator.js (not ours)
world.js   say         <- ./narrator.js (not ours)
player.js  rooms       <- world.js   const    line 6, used 1x here
game.js    roll        <- ./dice.js (not ours)
game.js    say         <- ./narrator.js (not ours)
game.js    createInterface <- node:readline/promises (not ours)
game.js    DIRECTIONS  <- world.js   const    line 3, used 1x here
game.js    LAMP_LIFE   <- world.js   const    line 4, used 2x here
game.js    look        <- world.js   function line 31, used 3x here
game.js    restock     <- world.js   function line 40, used 1x here
game.js    Player      <- player.js  class    line 5, used 1x here
```

`spec.local` is the identifier the importing file uses, `spec.imported` the name as the other file exported it. They differ in `import { setTimeout as sleep }`; here they are the same.

## Every export, everywhere

With the links in place, an export's uses across the program are its own module's references to it, plus the references in every other module to the import binding that points at it.

```js
for (const m of modules.values()) {
	for (const [name, binding] of m.exports) {
		const here = m.references.filter((r) => r.binding === binding).length;
		const elsewhere = [];
		for (const other of modules.values()) {
			if (other === m) continue;
			for (const b of other.bindings) {
				if (b.kind === 'import' && b.imported === binding) elsewhere.push(`${other.name} ${other.references.filter((r) => r.binding === b).length}x`);
			}
		}
		console.log(`${m.name.padEnd(10)} ${name.padEnd(11)} used here ${here}x; ${elsewhere.join(', ') || 'imported nowhere'}`);
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

`game.js` is the entry point, so its exports are imported by nobody in the program; those are the public surface. An export used by nobody anywhere would be dead code.

## Who changes the score

The score, the turn count and the lamp live in `game.js` as module-level `let`s, and nothing outside that file can touch them: `player.js` reports points back instead. Their writers are the references with `write` set, named by walking `scope.parent` up to the function they sit in.

```js
const game = modules.get('game.js');
const nameOf = (fn) => {
	if (fn.id) return fn.id.name;
	const p = parentOf(fn);
	if (p.type === 'Property') return `${parentOf(parentOf(p)).id.name}.${p.key.name}`;
	if (p.type === 'MethodDefinition') return `${parentOf(parentOf(p)).id.name}.${p.key.name}`;
	return '(anonymous)';
};
const inside = (r) => { for (let s = r.scope; s; s = s.parent) if (s.kind === 'function') return s.node; return null; };

for (const b of game.bindings.filter((b) => b.kind === 'let' && b.scope === game.scopes[0])) {
	const writes = game.references.filter((r) => r.binding === b && r.write);
	console.log(`${b.name.padEnd(10)} ${writes.map((r) => `${nameOf(inside(r))}:${game.line(r.node)}`).join(', ')}`);
}
```

```text
turns      step:50, reset:77
score      verbs.eat:21, act:28, reset:78
lampTurns  step:52, reset:79
```

`nameOf` is `parentOf` twice: a verb's arrow function is the value of a property, so it climbs to the property, whose `key` is the verb, and then to the object and the `verbs` declarator that holds it.
