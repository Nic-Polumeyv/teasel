---
title: Scopes
---

Turn `scopes` on when you make the [`Source`](/getting-started), and every identifier in the tree can tell you what it refers to.

```js
import { Source, referenceOf } from '@teasel/parser';

const { node } = new Source('let x = 1; x = 2', { scopes: true }).parse();
const assignment = node.body[1].expression;

referenceOf(assignment.left).binding;   // { name: 'x', kind: 'let', scope, node, declaration }
```

The tree is still plain ESTree. The facts live beside it, and you reach them from a node with a function.

## Three questions

`referenceOf` is one of three. Each takes a node from the answer and gives back a fact about it.

- `referenceOf(identifier)`: the reference this identifier makes, whether it declares a name or uses one, with the `binding` it resolves to. `binding` is `null` for a global. The whole answer is `undefined` when the identifier isn't a value at all, a property key for instance.
- `scopeOf(node)`: the scope this node opens, if it opens one. Programs, functions, classes, blocks, catch clauses, `for` heads, `switch` statements, static blocks and `with` all do.
- `parentOf(node)`: the node this one hangs from. `undefined` at the root. This one works without `scopes`.

Here they are on a function with a parameter.

```js
import { Source, scopeOf, referenceOf, parentOf } from '@teasel/parser';

const { node } = new Source('let x = 1; function f(y) { x = y; }', { scopes: true }).parse();
const [declaration, fn] = node.body;
const assignment = fn.body.body[0].expression;

referenceOf(declaration.declarations[0].id) // { declares: true, write: true, read: false, binding, scope, node, writeExpr }
referenceOf(assignment.left)                // { declares: false, write: true, read: false, binding, scope, node, writeExpr }
scopeOf(fn)                                 // { kind: 'function', parent, node, topLevelAwait: false }
parentOf(assignment.left)                   // the assignment
```

![the script scope declares x; the function scope of f declares y; inside it, x is a write reference to the outer binding and y a read reference to the parameter](Scopes.svelte)

## What a reference knows

A reference points from the scope it was made in to the binding it landed on, and says what the code does with it.

| flag | true when |
| --- | --- |
| `declares` | this is the identifier that declares the binding. |
| `read` | the value is used. Every reference except a declaration or a plain assignment target. |
| `write` | the identifier is assigned, updated, or bound by a destructuring assignment. A declaration writes when a value is bound there: an initializer, a parameter, a function. |
| `mutate` | something on the value is assigned, updated or deleted, like `x.y = 1`. |

A compound assignment like `x += 1` is both a read and a write. For a write, `writeExpr` is what's being assigned: the right-hand side, the thing iterated in a `for-of`, or what a declaration is initialized with. A bare `let x;` declares without writing.

A binding knows its `name`, its `kind`, the scope it lives in, the identifier that declared it, and the declaration itself. The kinds are listed in the [reference](/reference/parser#binding).

## All of them at once

If you'd rather not walk the tree, the answer's `scopes`, `bindings` and `references` tables list every one in source order.

## Two things to know

- The facts are symbol properties on the nodes. Spread a node and they come along. Push it through `structuredClone` or JSON and they're gone.
- Facts belong to one answer. Parse again, ask again.
