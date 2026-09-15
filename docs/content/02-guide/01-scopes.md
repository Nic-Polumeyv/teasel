---
title: Scopes
---

With `scopes` on, the tree stays plain ESTree and the facts hang beside it, reached from a node with four functions.

```js
import { Source, scopeOf, bindingOf, referenceOf, parentOf } from '@teasel/parser';

const { node } = new Source('let x = 1; function f(y) { x = y; }', { scopes: true }).parse();
const [declaration, fn] = node.body;
const assignment = fn.body.body[0].expression;

bindingOf(declaration.declarations[0].id)   // { name: 'x', kind: 'let', scope, node, declaration }
referenceOf(assignment.left)                // { scope, binding, write: true, read: false, mutate: false, node, writeExpr }
scopeOf(fn)                                 // { kind: 'function', parent, node, topLevelAwait: false }
parentOf(assignment.left)                   // the assignment
```

![the module scope declares x; the function scope of f declares y; inside it, x is a write reference to the outer binding and y a read reference to the parameter](Scopes.svelte)

## The four questions

- `scopeOf(node)`: the scope a node opens, when it opens one. A program, a function, a class, a block, a catch clause, a `for` head, a `switch`, a static block and a `with` each open one.
- `bindingOf(identifier)`: what an identifier declares or refers to. `null` for a global; `undefined` when the identifier names no value, a property key say.
- `referenceOf(identifier)`: the reference an identifier makes, with `read`, `write` and `mutate`. A global's reference exists too, with `binding: null`, though no binding lists it.
- `parentOf(node)`: the node it is a child of; `undefined` for the root of an answer.

## Bindings and references

A binding knows its `name`, its `kind` (`let`, `const`, `var`, `function`, `class`, `param`, `import`, `catch` and the rest the [reference](/reference/parser#binding) lists), the scope it lives in, the identifier that declares it and the declaration node, the declarator, function, class, import specifier or catch clause.

A reference knows the scope it is made from and the binding it resolves to. `write` says the identifier is assigned to, updated or bound by a destructuring assignment; `read` says its value is read, which is every reference but a plain assignment's target; a compound assignment reads and writes. `mutate` says a member of its value is assigned to, updated or deleted. `writeExpr` is what a write assigns, the right side of the assignment or the iterated expression of a `for-of`.

The answer's `scopes`, `bindings` and `references` tables list all of them in source order, so a pass over every binding needs no walk of the tree.

## Caveats

- A copy of a node made by `structuredClone` or through JSON carries no facts; the four functions answer `undefined` for it. Spreading keeps them, since the facts are symbol properties of the node.
- Facts live on the nodes of one answer. Parse again and ask again.
