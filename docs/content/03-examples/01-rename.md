---
title: Rename a variable
---

Renaming by search and replace goes wrong the moment two things share a name. With the scope facts, a rename is one binding and its references, and nothing else.

```js
import { Source, referenceOf } from '@teasel/parser';

const text = `let count = 0;
function tick() { count += 1; }
function reset() { let count = 0; count = 1; }
`;

const { node, references } = new Source(text, { scopes: true }).parse();

const binding = referenceOf(node.body[0].declarations[0].id);
const spots = [binding.node, ...references.filter((r) => r.binding === binding).map((r) => r.node)];

spots.map((n) => [n.start, n.end]);   // [[4, 9], [33, 38]]
```

Two spots, not four. The binding is the first spot, its declaring identifier, and the references are the rest, a name declared twice included. The `count` inside `reset` is a different binding, declared by the inner `let`, and its reference resolves there. The outer binding never sees it.

![three lines of code; the outer count and its one reference in tick are marked as the rename; the inner count in reset, and its reference, resolve to each other and stay out of it](Rename.svelte)

Every node carries `start` and `end`, so the rewrite is slicing, last spot first so earlier offsets stay true.

```js
let out = text;
for (const { start, end } of spots.sort((a, b) => b.start - a.start)) {
	out = out.slice(0, start) + 'total' + out.slice(end);
}
```

```text
let total = 0;
function tick() { total += 1; }
function reset() { let count = 0; count = 1; }
```

The same shape answers "where is this used" for a hover, or "is this ever read" for a lint. The `references` table is every reference in the file in source order; filter it by binding and you have them all.
