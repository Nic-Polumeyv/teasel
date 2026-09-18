---
title: A document
---

A template language can hand the parser its whole syntax, and the parser then reads a whole document: the host's own nodes around the JavaScript ones, in one tree, scopes across both. The syntax is a grammar, a short text in the format the [host grammar reference](/host-grammar) describes. Here is a small one, whole.

```text mini.grammar
host mini

document Root  script?=script  { children=fragment }
delimiters {{ }}
sigils open=# branch=: close=/ tag=@
void br hr img input
fragment Fragment nodes scope
elements name=name attributes=attributes children=children

element script  Element  raw
element *       Element

script script

block if  IfBlock
  open    test=expression  -> consequent
  branch  else  -> alternate

block each  EachBlock
  open    list=expression as item=pattern  -> body declares item

expression  ExpressionTag  expression=expression
```

A `Plan` made from the grammar reads documents of it. Make it once, at module level: the engine reads the grammar the first time the plan is used and keeps it.

```js
import { Source, Plan } from '@teasel/parser';

const mini = new Plan(grammar);

const { node } = new Source('<p>{{ greeting }}</p>').parse(mini);
node.type;                                   // 'Root'
node.children.nodes[0].type;                 // 'Element', name 'p'
node.children.nodes[0].children.nodes[0];    // ExpressionTag, its expression the Identifier greeting
```

A plan made from a grammar reads the whole source: it takes no position, and `until` isn't for it. A grammar the parser can't read throws when the plan is made, naming the line.

## What comes back

The tree is the host's. Every node has the `type` the grammar names, `start` and `end` into the text, `loc` with `locations`, and the fields the grammar's rule lists. Where a field holds JavaScript, it holds ESTree: the expression of an `{{ }}`, the program of a `<script>`, the pattern an `each` head declares. Offsets are offsets into the whole document, so nothing has to be added back.

The options work as they do on JavaScript, with two additions.

- `scopes` adds `roots`: one entry per piece of JavaScript the parser read, in source order, with the piece's `node`, the `scope` it sits in, and the `scopes`, `bindings` and `references` inside it. A host that keeps tables per piece takes them from here; `scopes`, `bindings` and `references` on the answer are the whole document's, as always.
- `typescript` turns on by itself when the grammar says a script tag can ask for it, `<script lang="ts">` say, and `'erase'` still applies: give `typescript: 'erase'` and the scripts come back as JavaScript, with the answer's `typescript` list of what could not be erased.

Scopes cross the boundary. The grammar says where a scope opens, the `each` body that declares its item here, and what a script declares lands in the scope around the template, so an identifier in the template resolves to a binding in the script.

```js
const text = '<script>let names = []</script>{{#each names as name}}<b>{{ name }}</b>{{/each}}';
const { node, roots } = new Source(text, { sourceType: 'module', scopes: true }).parse(mini);
const block = node.children.nodes[0];

referenceOf(block.list).binding;             // { name: 'names', kind: 'let', … }, declared in the script
scopeOf(block.body);                         // { kind: 'fragment', parent, node: block.body }
roots.map((piece) => piece.node.type);       // ['Program', 'Identifier', 'Identifier', 'Identifier']
```

The scope kinds a document adds are `fragment` for what the grammar opens and `module` or `script` for the document itself; a script block of its own has the kind of its program.

## When it's broken

`errorRecovery` works on a document as it does on JavaScript: an unclosed element, an expression cut short, a block without its end, all come back as the tree that could be read, with `errors` listing each one. The host's errors have codes of their own, `unclosed`, `expected`, `duplicate` and the others in the [reference](/reference/parser#parseerror), beside JavaScript's. Without recovery the first one throws.

```js
const { node, errors } = new Source('<p>{{ a', { errorRecovery: true }).parse(mini);
errors.map((e) => e.code);                   // ['unclosed', 'expected']
node.children.nodes[0].type;                 // 'Element', read as far as it went
```

## Two grammars that ship

The parser's own tests carry two whole grammars, one for a component language with script and style blocks, directives and `{#each}`-style blocks, and one for a template language with prefixed directives and `{{ }}` interpolation. They're what the [reference](/host-grammar) quotes; between them they use every statement of the format.
