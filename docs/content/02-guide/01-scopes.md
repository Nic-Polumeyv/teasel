---
title: Scopes
---

Turn `scopes` on and the tree is still plain ESTree. The scope facts don't live in the tree; they live beside it, and you reach them from a node with one of four functions.

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

- `scopeOf(node)`: the scope this node opens, if it opens one. Programs, functions, classes, blocks, catch clauses, `for` heads, `switch` statements, static blocks and `with` all open one.
- `bindingOf(identifier)`: what this identifier declares or refers to. `null` means it's a global. `undefined` means it isn't a value at all, a property key for instance.
- `referenceOf(identifier)`: the reference this identifier makes, with `read`, `write` and `mutate`. Globals get a reference too, with `binding: null`, even though no binding lists them.
- `parentOf(node)`: the node this one hangs from. `undefined` at the root.

## Bindings and references

A binding knows its `name`, its `kind` (`let`, `const`, `var`, `function`, `class`, `param`, `import`, `catch`, and the others in the [reference](/reference/parser#binding)), the scope it lives in, the identifier that declared it, and the declaration itself: the declarator, function, class, import specifier or catch clause.

A reference knows the scope it was made from and the binding it landed on. `write` means the identifier is assigned, updated, or bound by a destructuring assignment. `read` means its value is used, which is every reference except a plain assignment target. A compound assignment is both. `mutate` means something is assigned to, updated, or deleted on the value, like `x.y = 1`. `writeExpr` is what a write assigns: the right-hand side, or the thing iterated in a `for-of`.

If you'd rather not walk the tree at all, the answer's `scopes`, `bindings` and `references` tables list every one of them in source order.

## Two things to know

- The facts are symbol properties on the nodes. Spread a node and they come along. Push it through `structuredClone` or JSON and they're gone; the four functions answer `undefined` for the copy.
- Facts belong to one answer. Parse again, ask again.
