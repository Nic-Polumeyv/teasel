---
title: A whole module
---

One parse of a real file, then every question answered from the tables. Nothing is walked twice.

The file is 183 lines: two classes with private fields, an async generator, closures over module state, three imports, seven exports.

```file scheduler.js
```

## The parse

```js
import { readFileSync } from 'node:fs';
import { Source, referenceOf, scopeOf, parentOf } from '@teasel/parser';

const text = readFileSync('scheduler.js', 'utf8');
const { node, scopes, bindings, references } = new Source(text, { sourceType: 'module', scopes: true }).parse();

const line = (n) => text.slice(0, n.start).split('\n').length;
const module = scopes[0];
const top = bindings.filter((b) => b.scope === module);
```

```text
16 statements, 44 scopes, 62 bindings, 119 references
```

That's the whole cost. `node` is the tree, the three tables are every scope, binding and reference in the file in source order, and each row points at the others: a reference at its binding and the scope it's made from, a binding at its scope and its declaring node. Every analysis below is a filter over one table and a few hops along those pointers.

![one parse gives a tree and three tables; five analyses read them: imports read bindings and references, module state reads references and scopes, exports reads the tree and bindings, captures reads scopes and references, calls reads references and the tree](WholeModule.svelte)

## Where each import is used

An import is a binding of kind `import`. Its uses are the references that resolve to it.

```js
for (const b of top.filter((b) => b.kind === 'import')) {
	const uses = references.filter((r) => r.binding === b);
	console.log(`${b.name.padEnd(12)} line ${line(b.node)}  used ${uses.length}x at lines ${uses.map((r) => line(r.node)).join(', ') || 'never'}`);
}
```

```text
EventEmitter line 1  used 1x at lines 60
sleep        line 2  used 1x at lines 117
randomUUID   line 3  used 1x at lines 14
```

An import with no uses is the unused-import lint, three lines long.

## Who writes the module's state

`started`, `finished` and `failed` are module-level `let`s. Their writers are the references with `write` set. To say which function each write sits in, walk `scope.parent` up to the first function scope; that scope's `node` is the function.

```js
const nameOf = (fn) => {
	if (fn.id) return fn.id.name;
	const method = parentOf(fn);
	if (method?.type !== 'MethodDefinition') return '(anonymous)';
	const cls = parentOf(parentOf(method));
	const key = method.key.type === 'PrivateIdentifier' ? '#' + method.key.name : method.key.name;
	return `${cls.id.name}.${key}`;
};
const inside = (r) => { for (let s = r.scope; s; s = s.parent) if (s.kind === 'function') return s.node; return null; };

for (const b of top.filter((b) => b.kind === 'let')) {
	const writes = references.filter((r) => r.binding === b && r.write);
	const where = writes.map((r) => { const fn = inside(r); return fn ? `${nameOf(fn)}:${line(r.node)}` : `top:${line(r.node)}`; });
	console.log(`${b.name.padEnd(10)} written ${writes.length}x: ${where.join(', ')}`);
}
```

```text
started    written 2x: Scheduler.#start:104, resetStats:155
finished   written 2x: Scheduler.#start:109, resetStats:156
failed     written 2x: Scheduler.#start:122, resetStats:157
```

`nameOf` is the one place the tree is consulted: a method's function expression has no name of its own, so `parentOf` climbs to the method definition and then to the class. That's `parentOf` doing what it's for, answering "what is this node inside of" without a walk.

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
Job          class    line 13  referenced inside the module 1x
Scheduler    class    line 60  referenced inside the module 1x
withTimeout  function line 141  referenced inside the module 1x
stats        function line 150  referenced inside the module 0x
resetStats   function line 154  referenced inside the module 0x
group        function line 160  referenced inside the module 0x
runAll       function line 171  referenced inside the module 0x
```

## What every function closes over

A function captures a variable when a reference inside it resolves to a binding declared outside it. `within` walks `parent` to decide "inside". Module-level names are reported apart, since every function reaches those.

```js
const within = (scope, of) => { for (let s = scope; s; s = s.parent) if (s === of) return true; return false; };

for (const s of scopes.filter((s) => s.kind === 'function' && s.node.type !== 'ArrowFunctionExpression')) {
	const inHere = references.filter((r) => within(r.scope, s) && r.binding);
	const captured = new Set(inHere.filter((r) => !within(r.binding.scope, s) && r.binding.scope !== module).map((r) => r.binding.name));
	const fromModule = new Set(inHere.filter((r) => r.binding.scope === module).map((r) => r.binding.name));
	if (captured.size || fromModule.size) console.log(`${nameOf(s.node).padEnd(22)} closes over ${[...captured].join(', ') || 'nothing'}; from the module: ${[...fromModule].join(', ') || 'nothing'}`);
}
```

```text
Job.constructor        closes over nothing; from the module: DEFAULT_RETRIES
Job.execute            closes over nothing; from the module: withTimeout
Scheduler.constructor  closes over nothing; from the module: DEFAULT_CONCURRENCY
Scheduler.add          closes over nothing; from the module: Job
Scheduler.#start       closes over nothing; from the module: started, finished, BACKOFF, sleep, failed
stats                  closes over nothing; from the module: started, finished, failed
resetStats             closes over nothing; from the module: started, finished, failed
(anonymous)            closes over scheduler, jobs; from the module: nothing
runAll                 closes over nothing; from the module: Scheduler
```

The anonymous one is `settled()` inside `group`, the only real closure in the file: it holds `scheduler` and `jobs` from the call that made it. A bundler deciding what a function can be hoisted past reads exactly this list.

## Every call of a function

A reference says a name was used; whether it was called is the parent node's business.

```js
const wt = top.find((b) => b.name === 'withTimeout');
for (const r of references.filter((r) => r.binding === wt)) {
	const p = parentOf(r.node);
	console.log(`line ${line(r.node)}: ${p.type === 'CallExpression' && p.callee === r.node ? 'called with ' + p.arguments.length + ' arguments' : 'not a call, ' + p.type}`);
}
```

```text
line 49: called with 3 arguments
```

Five analyses, one parse, and the file was never walked: the tables were built while it was parsed, and everything after that is filtering rows and following pointers.
