---
title: What a function closes over
---

A closure is a function that uses variables from outside itself. Which ones? Each reference knows the scope it was made in and the scope its binding lives in. If the binding's scope is outside the function, the function captured it.

```js
import { Source } from '@teasel/parser';

const text = `let total = 0;
function add(n) {
  let twice = n * 2;
  return () => { total += twice; };
}`;

const { scopes, references } = new Source(text, { scopes: true }).parse();

const arrow = scopes.find((s) => s.node.type === 'ArrowFunctionExpression');
const within = (scope, of) => {
	for (let s = scope; s; s = s.parent) if (s === of) return true;
	return false;
};

const captured = references.filter((r) => within(r.scope, arrow) && r.binding && !within(r.binding.scope, arrow));
captured.map((r) => r.binding.name);   // ['total', 'twice']
```

`within` walks up through `parent` until it finds the scope it's looking for, or runs out. A reference made inside the arrow whose binding lives outside it is a capture. `n` isn't one: it's used in `add`, not in the arrow. A variable declared inside the arrow isn't either, since its declaring reference and its binding sit in the same scope.

![three nested scopes, script, add and the arrow function; inside the arrow, total and twice are references whose arrows cross out to bindings in outer scopes, so they are captured; n is referenced inside add and stays there](Closures.svelte)

Each captured reference also says how the variable is used. `total` is written to, `twice` only read.

```js
captured.map((r) => [r.binding.name, r.write, r.read]);   // [['total', true, true], ['twice', false, true]]
```

That's enough to tell a hoisting optimisation which captures are read-only, or a linter which closure mutates state it doesn't own.
