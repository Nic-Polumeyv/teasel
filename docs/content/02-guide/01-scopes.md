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

<svg viewBox="0 0 720 220" width="100%" role="img" aria-label="the module scope declares x; the function scope of f declares y; inside it, x is a write reference to the outer binding and y a read reference to the parameter" fill="none" stroke="currentColor" stroke-width="1.5" font-size="14" class="my-8">
  <rect x="8" y="8" width="704" height="204" rx="10"/>
  <text x="24" y="32" fill="currentColor" stroke="none">scope: module</text>
  <text x="24" y="60" fill="currentColor" stroke="none" font-family="monospace">let x = 1</text>
  <rect x="140" y="44" width="150" height="24" rx="6" stroke-dasharray="3 3"/><text x="215" y="61" text-anchor="middle" fill="currentColor" stroke="none" font-size="12">x  binding  let</text>
  <rect x="24" y="84" width="672" height="112" rx="10"/>
  <text x="40" y="108" fill="currentColor" stroke="none">scope: function</text>
  <text x="40" y="136" fill="currentColor" stroke="none" font-family="monospace">function f(y) {</text>
  <rect x="190" y="120" width="170" height="24" rx="6" stroke-dasharray="3 3"/><text x="275" y="137" text-anchor="middle" fill="currentColor" stroke="none" font-size="12">y  binding  param</text>
  <text x="40" y="176" fill="currentColor" stroke="none" font-family="monospace">  x = y</text>
  <rect x="140" y="160" width="190" height="24" rx="6" stroke-dasharray="3 3"/><text x="235" y="177" text-anchor="middle" fill="currentColor" stroke="none" font-size="12">x  reference  write</text>
  <rect x="350" y="160" width="180" height="24" rx="6" stroke-dasharray="3 3"/><text x="440" y="177" text-anchor="middle" fill="currentColor" stroke="none" font-size="12">y  reference  read</text>
  <path d="M140 172 C 100 172, 100 56, 140 56" marker-end="url(#b)" class="text-muted-foreground"/>
  <path d="M350 172 C 380 150, 330 150, 360 144" marker-end="url(#b)" class="text-muted-foreground"/>
  <defs><marker id="b" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor" stroke="none"/></marker></defs>
</svg>

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

- A copy of a node carries no facts: `structuredClone` and spreading make plain objects the four functions answer `undefined` for.
- Facts live on the nodes of one answer. Parse again and ask again.
