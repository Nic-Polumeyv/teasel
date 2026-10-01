---
title: Writing a grammar
---

Imagine you are designing a template language for your UI library. A page of it looks like this:

```tpl list.tpl
<ul>
	{{#repeat item, i in items by item.id}}
		<Card title={{ item.name }} index={{ i }} />
	{{:empty}}
		<li>No items</li>
	{{/repeat}}
</ul>
```

`{{#repeat}}` renders a `<Card>` for every item of `items`, keyed by `item.id`. `{{:empty}}` is what shows when the list is empty, and `{{ … }}` drops a JavaScript expression into the page.

Your compiler needs this file as a tree: the HTML, your `repeat` block, and the JavaScript inside both, with every name traced to where it is declared. teasel reads the file into that tree once you describe the language to it as a grammar.

You don't need to write the whole grammar at once. Describe the part you know, run it on `list.tpl`, and read where the parser stops: that is the next thing to describe.

## 1. Start with the HTML

Most of the file is HTML, so start there. Every kind of node gets a type, and the names are yours to choose: your compiler is the one that reads them. A component is a kind of its own, since your compiler renders `<Card>` by calling it and `<ul>` by creating an element. Hover a piece of the grammar to see what it reads in `list.tpl`.

![list.tpl above the pieces of an HTML grammar: document, other, component, fields, text, comment and delimiters, each with arrows to the part of the file it reads](Html.svelte "start=document")

Put together, with the language's name:

```js tpl.js
import * as g from '@teasel/parser/grammar';

export const tpl = g.grammar('tpl', {
	document: g.node('Template', { children: g.content }),
	text: g.node('Text', { data: g.text.data }),
	comment: g.node('Comment', { data: g.text.data }),
	delimiters: ['{{', '}}'],
	elements: {
		fields: {
			name: g.element.tag,
			attributes: g.element.attributes,
			children: g.content,
		},
		component: g.element(g.node('Component')),
		other: g.element(g.node('Element')),
	},
});
```

Run it:

```js parse.js
import { Source, Plan } from '@teasel/parser';
import { tpl } from './tpl.js';

const plan = new Plan(tpl);
new Source(list).parse(plan);
```

![list.tpl read with the HTML grammar: the parser stops at the first {{](Steps.svelte "step=1")

The parser reads `<ul>` and stops at the first `{{`. `delimiters` told it that `{{` opens something, and nothing says what yet.

## 2. Read the expressions

`{{ item.name }}` is a JavaScript expression, and you want it in the tree as one: an `Expression` node holding the expression's ESTree. It also appears as an attribute's value, `title={{ item.name }}`, and an attribute's value is plain text until the grammar lets it hold one.

```js tpl.js
	attributes: { expressions: true },
	expression: g.node('Expression', { expression: g.js.expression }),
```
```notes
expressions: true :: Lets an attribute's value be an expression between the delimiters.
g.js.expression :: Reads JavaScript up to the closing `}}`. The field holds its ESTree node.
```

![list.tpl read with expressions added: the parser stops at the # of {{#repeat](Steps.svelte "step=2")

The parser gets one character further. `{{` now opens an expression, and `#repeat item, i in items by item.id` is not JavaScript. A block needs a mark that tells it apart from an expression.

## 3. Mark the blocks

Your blocks start with `{{#`, continue with `{{:` and end with `{{/`: the delimiter and one character. Those characters are the block's sigils. teasel also takes a fourth, for a tag such as `{{@html …}}`, so name one even though your language has no tags.

```js tpl.js
	sigils: {
		open: '#',
		branch: ':',
		close: '/',
		tag: '@',
		blocks: {},
	},
```

![list.tpl read with sigils added: the parser asks which block repeat is](Steps.svelte "step=3")

Now `{{#` starts a block, and the parser asks which one. No block is called `repeat` yet.

## 4. Describe the repeat block

Read the block's head as a sentence: `repeat item, i in items by item.id`. Start with the parts every repeat has: the item, the word `in`, the list, and the content to repeat.

`item` and `items` are different kinds of name. `items` is used: it is declared somewhere else, in your component's script or by whoever renders it. `item` is declared here, and only the block's content can see it. Read it as a pattern, so `{ name } in items` works too, and bind it, so teasel declares it in the scope the block opens.

```js tpl.js
		blocks: {
			repeat: g.block(
				g.node(
					'RepeatBlock',
					{ item: g.bind(g.js.pattern) },
					'in',
					{ list: g.js.expression },
					{ body: g.content },
				),
			),
		},
```
```notes
repeat :: The word after `{{#`. The block closes with `{{/repeat}}`.
'RepeatBlock' :: The node's type. Its fields follow, in the order the head writes them.
g.bind :: Declares the names the pattern reads in the scope the block opens.
g.js.pattern :: Reads a name or a destructuring pattern.
'in' :: A word of your syntax. It stands between the fields and ends the pattern before it.
body :: The block's content, up to `{{/repeat}}` or a branch. Content comes last.
```

![list.tpl read with the repeat block: the parser expects in where the comma is](Steps.svelte "step=4")

The parser reads `item`, then expects `in` and finds the comma before the index.

## 5. Add the index

Not every repeat writes an index, so the index goes in `g.opt`: it is read only when its first word, the comma, is there. When it is missing, `g.optional` leaves the field out of the node. An index is one name, never a pattern, so read an identifier.

```js tpl.js
					{ item: g.bind(g.js.pattern) },
					g.opt(',', { index: g.optional(g.bind(g.js.identifier)) }),
					'in',
```

![list.tpl read with the index: the parser expects }} where by is](Steps.svelte "step=5")

The head now reads up to `by`.

## 6. Add the key

`by item.id` uses `item`; it declares nothing, so it is a plain expression with no `g.bind`. It is optional, like the index. It can still use `item`, because the scope a block opens starts at the first name the block declares and covers everything read after it.

```js tpl.js
					{ list: g.js.expression },
					g.opt('by', { key: g.optional(g.js.expression) }),
					{ body: g.content },
```

![list.tpl read with the key: the parser stops at the empty branch](Steps.svelte "step=6")

The whole head reads, and so does the first `<Card>`. The parser stops at `{{:empty}}`: a branch, in a block that has none.

## 7. Add the empty branch

A branch is a word after `{{:` with fields of its own. `empty` reads no JavaScript, only content, which goes in a field of the block. A repeat without the branch has no `fallback` field.

```js tpl.js
			repeat: g.block(
				g.node(
					'RepeatBlock',
					// the fields from steps 4 to 6
				),
				{ branches: { empty: [{ fallback: g.optional(g.content) }] } },
			),
```

![list.tpl read with the whole grammar: the tree, each node tied to its text](Steps.svelte "step=7")

`list.tpl` reads. Hover a node to see its text, or the text to find its node. `item` in `item.name` and `item.id` points back to the `item` the block declares.

The same links are on the tree you get, when you ask for `scopes`:

```js parse.js
import { Source, Plan, referenceOf } from '@teasel/parser';

const { node } = new Source(list, { scopes: true }).parse(plan);
const repeat = node.children[0].children[1];
const card = repeat.body[1];
const title = card.attributes[0].value.expression;   // item.name

referenceOf(title.object).binding.node === repeat.item;   // true
referenceOf(repeat.list).binding;                         // null: declared outside
```

## The whole grammar

Each color is one part of the language. Hover a part, or a line of it, to bring it forward, and follow its step to see why it is there.

![the whole grammar, colored by part: the file and its text, elements and components, expressions, block marks, the repeat block and the empty branch, each keyed to its step](Whole.svelte "tpl.js")

In TypeScript, and in JavaScript checked with JSDoc, every node is typed from it:

```ts types.ts
import type { NodeType } from '@teasel/parser/grammar';
import { tpl } from './tpl.js';

type Repeat = NodeType<typeof tpl, 'RepeatBlock'>;
// { type: 'RepeatBlock'; start: number; end: number; loc?: SourceLocation;
//   item: Pattern; index?: Identifier; list: Expression;
//   key?: Expression; body: Content[]; fallback?: Content[] }
```

`Content` is every node your content can hold: `Element`, `Component`, `Text`, `Comment`, `Expression` and `RepeatBlock`. [Parsing with a grammar](/parsing-with-a-grammar) covers the options a document parse takes, and the [grammar reference](/reference/grammar) lists every builder.
