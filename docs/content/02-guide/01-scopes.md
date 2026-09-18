---
title: Scopes
---

Turn `scopes` on when you make the [`Source`](/getting-started), and every identifier in the tree can tell you what it refers to.

```js scopes.js
import { Source, referenceOf } from '@teasel/parser';

const { node } = new Source('let x = 1; x = 2', { scopes: true }).parse();
const assignment = node.body[1].expression;

referenceOf(assignment.left).binding;   // { name: 'x', kind: 'let', scope, node, declaration }
```

The tree stays plain ESTree. Scope information is not in visible properties: you get it by passing a node to a function.

## referenceOf, scopeOf, parentOf

Three functions take a node from the answer and return what the parser knows about it.

- `referenceOf(identifier)`: the reference this identifier makes, with the `binding` it resolves to. For the identifier that declares a name, the answer is the binding itself: a binding is its own first reference, and its `binding` is itself. `binding` is `null` for a global. The whole answer is `undefined` when the identifier isn't a value at all, a property key for instance.
- `scopeOf(node)`: the scope this node opens, if it opens one. Programs, functions, classes, blocks, catch clauses, `for` heads, `switch` statements, static blocks and `with` all do; so does the name of a named function expression, a TypeScript namespace or enum, and a fragment of a file [read with a grammar](/parsing-with-a-grammar). The kinds are listed in the [reference](/reference/parser#scope).
- `parentOf(node)`: the node this one hangs from. `undefined` at the root. This one works without `scopes`.

All three, on a function with a parameter:

```js scopes.js
import { Source, scopeOf, referenceOf, parentOf } from '@teasel/parser';

const { node } = new Source('let x = 1; function f(y) { x = y; }', { scopes: true }).parse();
const [declaration, fn] = node.body;
const assignment = fn.body.body[0].expression;

referenceOf(declaration.declarations[0].id) // the binding: { name: 'x', kind: 'let', declares: true, write: true, scope, node, declaration, … }
referenceOf(assignment.left)                // { declares: false, write: true, read: false, binding, scope, node, writeExpr }
scopeOf(fn)                                 // { kind: 'function', parent, node, topLevelAwait: false }
parentOf(assignment.left)                   // the assignment
```

![the script scope declares x; the function scope of f declares y; inside it, x is a write reference to the outer binding and y a read reference to the parameter](Scopes.svelte)

## References and bindings

A reference points from the scope it was made in to the binding it landed on, and says what the code does with it.

| flag | true when |
| --- | --- |
| `declares` | the identifier declares the binding. True on every binding, and on a reference only for a name declared twice, `var x` and `var x` again. |
| `read` | the value is used. Every reference except a declaration or a plain assignment target. |
| `write` | the identifier is assigned, updated, or bound by a destructuring assignment. A declaration writes when a value is bound there: an initializer, a parameter, a function. |
| `mutate` | something on the value is assigned, updated or deleted, like `x.y = 1`. |

A compound assignment like `x += 1` is both a read and a write. For a write, `writeExpr` is what's being assigned: the right-hand side, or the thing iterated in a `for-of`. On a binding it's the declarator's initializer, `1` in `let x = 1`, and a bare `let x;` declares without writing.

A binding knows its `name`, its `kind`, the scope it lives in, the identifier that declared it, and the declaration itself. The kinds are listed in the [reference](/reference/parser#binding).

## The tables

The answer's `scopes`, `bindings` and `references` list every scope, binding and reference in source order, so none of this needs a walk of the tree.

## Limits

- The information is stored on the nodes under symbol keys. A spread copies it. `structuredClone` and JSON drop it.
- It belongs to one answer. After another parse, ask with the new answer's nodes.
