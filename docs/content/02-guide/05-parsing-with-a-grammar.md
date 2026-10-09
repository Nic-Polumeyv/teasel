---
title: Parsing with a grammar
---

A template language can describe its whole syntax to the parser as a grammar. The parser then reads a whole file of that language and returns one tree: the template's own nodes, the JavaScript nodes inside them, and scopes that cover both. These pages call the template language the host. A grammar is a definition made with the builders `@teasel/parser/grammar` exports, which the [grammar reference](/reference/grammar) lists. [Writing a grammar](/writing-a-grammar) builds one step by step. This is a complete small one:

```js mini.js
import * as g from '@teasel/parser/grammar';

export const grammar = g.grammar('mini', {
	document: { node: 'Root', form: [{ script: g.optional(g.doc.script) }, g.scope({ children: g.content })] },
	fragment: { node: 'Fragment', form: [g.scope({ nodes: g.nodes })] },
	text: { node: 'Text', form: [{ data: g.text.data }] },
	comment: { node: 'Comment', form: [{ data: g.text.data }] },
	void: ['br', 'hr', 'img', 'input'],
	elements: {
		fields: { name: g.element.tag, attributes: g.element.attributes, children: g.content },
		rules: { script: { node: 'Element', content: 'raw' } },
		other: { node: 'Element' },
	},
	script: { element: 'script' },
	constructs: {
		IfBlock: {
			open: { marker: ['{{', '#if'], space: true, form: [{ test: g.js.expression }, '}}', { consequent: g.content }] },
			branches: [{ marker: ['{{', ':else'], form: ['}}', { alternate: g.content }] }],
			close: { marker: ['{{', '/if'], form: ['}}'] },
		},
		EachBlock: {
			open: { marker: ['{{', '#each'], space: true, form: [{ list: g.js.expression }, 'as', { item: g.bind(g.js.pattern) }, '}}', { body: g.content }] },
			close: { marker: ['{{', '/each'], form: ['}}'] },
		},
		ExpressionTag: { open: { marker: ['{{'], form: [{ expression: g.js.expression }, '}}'] } },
	},
});
```

`parse` takes the grammar and reads a document of it. The engine reads a grammar on its first parse and keeps it while the grammar lives, so make it once, at module level, as `mini.js` does.

```js document.js
import { Source } from '@teasel/parser';
import { grammar } from './mini.js';

const { node } = new Source('<p>{{ greeting }}</p>').parse(grammar);
node.type;                                   // 'Root'
node.children.nodes[0].type;                 // 'Element', name 'p'
node.children.nodes[0].children.nodes[0];
// ExpressionTag, its expression the Identifier greeting
```

In TypeScript, and in JavaScript checked with JSDoc, the tree is typed from the definition: `node.children.nodes[0]` is an `Element`, an `IfBlock` or one of the others the grammar names, and `EachBlock`'s `item` is a `Pattern`. A field named `type`, or `bind` on a source that reads an expression, is a type error. A tag that reads content is a type error too, and what the engine could not parse with is refused when the grammar is first used, with a message naming the rule. A rule written apart from the grammar, to share it, keeps its types with `as const`.

A grammar reads the whole source, so `parse` takes no position with one.

## The tree

The tree's nodes are the ones the grammar names. Every node has the `type` the grammar names, `start` and `end` into the text, `loc` with `locations: true`, and the fields the grammar's rule lists; `locations: 'js'` puts `loc` on the JavaScript nodes and comments alone. Where a field holds JavaScript, it holds ESTree: the expression of an `{{ }}`, the program of a `<script>`, the pattern an `each` head declares. Offsets are offsets into the whole document, so nothing has to be added to them.

The options work as they do on JavaScript, with two additions.

- `scopes` adds `roots`: one entry per piece of JavaScript the parser read, in source order, with the piece's `node`, the `scope` it sits in, and the `scopes`, `bindings` and `references` inside it. A host that keeps tables per piece takes them from here; `scopes`, `bindings` and `references` on the answer cover the whole file.
- `typescript` turns on by itself when the grammar says a script tag can ask for it, as `<script lang="ts">` does, and `'erase'` still applies: give `typescript: 'erase'` and the scripts come back as JavaScript, with the answer's `typescript` list of what could not be erased.

Scopes cover the template and the script together. The grammar says where a scope opens, the `each` body that declares its item here, and what a script declares lands in the scope around the template, so an identifier in the template resolves to a binding in the script.

```js document.js
const text =
	'<script>let names = []</script>' +
	'{{#each names as name}}<b>{{ name }}</b>{{/each}}';
const source = new Source(text, { sourceType: 'module', scopes: true });
const { node, roots } = source.parse(grammar);
const block = node.children.nodes[0];

referenceOf(block.list).binding;
// { name: 'names', kind: 'let', … }, declared in the script

scopeOf(block.body);
// { kind: 'fragment', parent, node: block.body }

roots.map((piece) => piece.node.type);
// ['Program', 'Identifier', 'Identifier', 'Identifier']
```

The scope kinds a document adds are `fragment` for what the grammar opens and `module` or `script` for the document itself; a script block of its own has the kind of its program.

## Error recovery

`errorRecovery` works on a document as it does on JavaScript: an unclosed element, an expression cut short, a block without its end, all come back as the tree that could be read, with `errors` listing each one. The host's errors have codes of their own, `unclosed`, `expected`, `duplicate` and the others in the [reference](/reference/parser#parseerror), beside JavaScript's. Without recovery the first one throws.

```js document.js
const { node, errors } = new Source('<p>{{ a', { errorRecovery: true }).parse(grammar);
errors.map((e) => e.code);                   // ['unclosed', 'expected']
node.children.nodes[0].type;                 // 'Element', read as far as it went
```

## Example grammars

The package's own tests carry two whole grammars, one for a component language with script and style blocks, directives and `{#each}`-style blocks, and one for a template language with prefixed directives and `{{ }}` interpolation, in `npm/scripts/hosts`. Between them they use every builder.
