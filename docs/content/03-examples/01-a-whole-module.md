---
title: A whole module
---

Five things a tool asks about a file: where imports are used, who changes shared state, what is exported, what each function reaches for, where a function is called. The file is a text adventure, 175 lines: a map of rooms, a `Player` with private fields, a table of verbs, a little command parser of its own, and a score everyone keeps poking at.

```file adventure.js
```

## The parse

```js
import { readFileSync } from 'node:fs';
import { Source, referenceOf, scopeOf, parentOf } from '@teasel/parser';

const text = readFileSync('adventure.js', 'utf8');
const { node, scopes, bindings, references } = new Source(text, { sourceType: 'module', scopes: true }).parse();

const line = (n) => text.slice(0, n.start).split('\n').length;
const module = scopes[0];
const top = bindings.filter((b) => b.scope === module);
```

```text
19 statements, 27 scopes, 60 bindings, 163 references
```

`node` is the tree. `scopes`, `bindings` and `references` are the three tables, in source order, and their rows point at each other: a reference has its `binding` and the `scope` it's made from, a binding has its `scope` and the `node` that declares it. `line` turns a node's `start` into a line number for the output below, and `top` is the module scope's bindings, the names declared at the top level.

![the parse gives a tree and three tables; the five analyses below each read one or two of them: imports read bindings and references, module state reads references and scopes, exports reads the tree and bindings, reaches reads scopes and references, calls reads references and the tree](WholeModule.svelte)

## Where each import is used

An import is a binding of kind `import`. Its uses are the references that resolve to it.

```js
for (const b of top.filter((b) => b.kind === 'import')) {
	const uses = references.filter((r) => r.binding === b);
	console.log(`${b.name.padEnd(16)} line ${line(b.node)}  used ${uses.length}x at lines ${uses.map((r) => line(r.node)).join(', ') || 'never'}`);
}
```

```text
roll             line 1  used 1x at lines 110
describe         line 2  used 1x at lines 93
listen           line 2  used 1x at lines 171
say              line 2  used 17x at lines 61, 62, 66, 70, 73, 79, 80, 92, 97, 105, 107, 111, 113, 132, 134, 138, 157
createInterface  line 3  used 1x at lines 149
```

An import with no uses is an unused import.

## Who changes the score

`turns`, `score` and `lampTurns` are module-level `let`s. Whoever changes them is a reference with `write` set. To say which function each write sits in, walk `scope.parent` up to the first function scope; that scope's `node` is the function.

```js
const nameOf = (fn) => {
	if (fn.id) return fn.id.name;
	const parent = parentOf(fn);
	if (parent.type === 'MethodDefinition') {
		const key = parent.key.type === 'PrivateIdentifier' ? '#' + parent.key.name : parent.key.name;
		return `${parentOf(parentOf(parent)).id.name}.${key}`;
	}
	if (parent.type === 'Property') return `${parentOf(parentOf(parent)).id.name}.${parent.key.name}`;
	if (parent.type === 'VariableDeclarator') return parent.id.name;
	if (parent.type === 'CallExpression' && parentOf(parent).type === 'VariableDeclarator') return parentOf(parent).id.name;
	return '(anonymous)';
};
const inside = (r) => { for (let s = r.scope; s; s = s.parent) if (s.kind === 'function') return s.node; return null; };

for (const b of top.filter((b) => b.kind === 'let')) {
	const writes = references.filter((r) => r.binding === b && r.write);
	const where = writes.map((r) => { const fn = inside(r); return fn ? `${nameOf(fn)}:${line(r.node)}` : `top:${line(r.node)}`; });
	console.log(`${b.name.padEnd(10)} written ${writes.length}x: ${where.join(', ')}`);
}
```

```text
turns      written 2x: step:135, reset:162
score      written 4x: Player.take:65, Player.go:84, verbs.eat:110, reset:163
lampTurns  written 2x: step:137, reset:164
```

`nameOf` is all `parentOf`. A method's function has no name of its own, so it climbs to the method definition, whose `key` is the name, and from there to the class. A verb's arrow function is the value of a property, so it climbs to the property, then to the object, then to the `verbs` declarator that holds it. That is how `verbs.eat` gets its name.

## What is exported, and is it used here

Exports are statements in the tree. Their names are bindings in the module scope.

```js
for (const s of node.body) {
	if (s.type !== 'ExportNamedDeclaration' || !s.declaration) continue;
	const d = s.declaration;
	const names = d.type === 'VariableDeclaration' ? d.declarations.map((x) => x.id.name) : [d.id.name];
	for (const name of names) {
		const b = top.find((b) => b.name === name);
		const uses = references.filter((r) => r.binding === b);
		console.log(`${name.padEnd(12)} ${b.kind.padEnd(8)} line ${line(b.node)}  referenced inside the module ${uses.length}x`);
	}
}
```

```text
Player       class    line 38  referenced inside the module 1x
look         function line 90  referenced inside the module 3x
parse        function line 116  referenced inside the module 1x
step         function line 130  referenced inside the module 1x
won          function line 143  referenced inside the module 2x
play         function line 147  referenced inside the module 0x
reset        function line 161  referenced inside the module 0x
hint         const    line 171  referenced inside the module 0x
```

## What each function reaches for

Every function scope in the table, and the module-level names referenced inside it, split by whether the reference reads or writes. `within` walks `parent` to decide whether a reference sits inside the function.

```js
const within = (scope, of) => { for (let s = scope; s; s = s.parent) if (s === of) return true; return false; };

for (const s of scopes.filter((s) => s.kind === 'function')) {
	const here = references.filter((r) => within(r.scope, s) && r.binding?.scope === module);
	const reads = new Set(here.filter((r) => r.read).map((r) => r.binding.name));
	const writes = new Set(here.filter((r) => r.write).map((r) => r.binding.name));
	if (here.length) console.log(`${nameOf(s.node).padEnd(16)} reads ${[...reads].join(', ') || 'nothing'}${writes.size ? `; writes ${[...writes].join(', ')}` : ''}`);
}
```

```text
Player.room      reads rooms
Player.take      reads say, MAX_CARRY, score; writes score
Player.drop      reads say
Player.go        reads say, score, look; writes score
look             reads lampTurns, say, describe
verbs.look       reads look
verbs.inventory  reads say
verbs.eat        reads say, score, roll; writes score
verbs.score      reads say, score, turns
parse            reads DIRECTIONS
step             reads parse, say, verbs, turns, lampTurns; writes turns, lampTurns
play             reads Player, createInterface, look, won, step, say, score, turns
reset            reads LAMP_LIFE, rooms; writes turns, score, lampTurns
```

`score += 5` is one reference that both reads and writes, so `Player.take` lists `score` on both sides. `Player.drop` touches nothing but `say`, and `hint` is missing because its arrow function only ever looks at its `player` argument.

## Every call of a function

A reference says a name was used. Whether it was called is in the tree: the parent of the identifier is a `CallExpression` with the identifier as its `callee`.

```js
const look = top.find((b) => b.name === 'look');
for (const r of references.filter((r) => r.binding === look)) {
	const p = parentOf(r.node);
	console.log(`line ${line(r.node)}: ${p.type === 'CallExpression' && p.callee === r.node ? 'called with ' + p.arguments.length + ' arguments' : 'not a call, ' + p.type}`);
}
```

```text
line 86: called with 1 arguments
line 101: called with 1 arguments
line 150: called with 1 arguments
```
