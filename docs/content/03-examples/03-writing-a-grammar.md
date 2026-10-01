---
title: Writing a grammar
---

This page builds a grammar for a small template language made up for it, one part at a time. Each step starts with a question about what the syntax means, and adds the part of the definition that answers it. [Parsing with a grammar](/parsing-with-a-grammar) covers using a grammar once you have one, and the [grammar reference](/reference/grammar) lists every builder.

The language renders a component once for each item of a list:

```text list.tpl
<ul>
	{{#repeat item, i in items by item.id}}
		<Card title={{ item.name }} index={{ i }} />
	{{:empty}}
		<li>No items</li>
	{{/repeat}}
</ul>
```

- `{{#repeat item, i in items by item.id}}` renders its body once per element of `items`, naming the element `item` and its position `i`. `by item.id` gives each repetition a key.
- `{{:empty}}` starts what renders when `items` is empty, and `{{/repeat}}` ends the block.
- `{{ … }}` holds a JavaScript expression, in text or in an attribute's value.
- `<Card>` is a component: an element whose name starts with a capital letter.

## 1. Elements and text

Before any syntax of its own, a file of this language is HTML: elements, text and comments. So the first questions are what each of these is called in the tree, and which fields it has.

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
		other: g.element(g.node('Element')),
	},
});
```
```notes
'tpl' :: The language's name.
document :: The node the whole file becomes, `Template`, with the file's content under `children`.
g.content :: The language's content: elements, text, and every node later steps add, as a list.
g.text.data :: The text as read, character references such as `&amp;` decoded.
delimiters :: What opens and closes an expression in text. Blocks start with them too, in step 4.
fields :: The fields every element has: its tag name, its attributes, and its children.
other :: The rule for every element no other rule matches. Here that is all of them.
```

`parse` takes the grammar and reads documents of the language:

```js parse.js
import { Source } from '@teasel/parser';
import { tpl } from './tpl.js';

new Source('<ul><li>Pears</li></ul>').parse(tpl).node;
// { type: 'Template', children: [
//   { type: 'Element', name: 'ul', attributes: [], children: [
//     { type: 'Element', name: 'li', attributes: [], children: [
//       { type: 'Text', data: 'Pears' } ] } ] } ] }
```

Every node also has `start` and `end`, offsets into the file, left out here and below.

## 2. Components

Should `<Card>` and `<li>` be the same kind of node? A tool reading the tree renders a component by calling it and an element by creating it, so they get different types. The `component` rule matches a name that starts with a capital letter or has a dot in it, as `<ui.Card>` does.

```js tpl.js
	elements: {
		fields: {
			name: g.element.tag,
			attributes: g.element.attributes,
			children: g.content,
		},
		component: g.element(g.node('Component')),
		other: g.element(g.node('Element')),
	},
```

```js parse.js
new Source('<Card />').parse(tpl).node.children[0];
// { type: 'Component', name: 'Card', attributes: [], children: [] }
```

## 3. Expressions

What does `{{ … }}` hold, and where can it appear? A JavaScript expression, in text and in an attribute's value. `delimiters` already sets how one starts and ends. `expression` names the node that holds it, and `attributes` lets an attribute's value be one.

```js tpl.js
	attributes: { expressions: true },
	expression: g.node('Expression', { expression: g.js.expression }),
```
```notes
expressions: true :: Without it an attribute's value is text, and `title={{ item.name }}` reads as an attribute `title` with the text `{{`, then two more attributes, `item.name` and `}}`.
g.js.expression :: Reads a JavaScript expression, up to the closing `}}`. The field holds its ESTree node.
```

```js parse.js
new Source('<Card title={{ item.name }} />Hi {{ user }}').parse(tpl).node;
// { type: 'Template', children: [
//   { type: 'Component', name: 'Card', attributes: [
//     { type: 'Attribute', name: 'title', value: { type: 'Expression',
//       expression: { type: 'MemberExpression', … } } } ], children: [] },
//   { type: 'Text', data: 'Hi ' },
//   { type: 'Expression', expression: { type: 'Identifier', name: 'user' } } ] }
```

## 4. The repeat block

How does the language mark where a block starts, where a branch of it starts, and where it ends? With the delimiter and one character: `{{#`, `{{:` and `{{/`. `sigils` names those characters, and a fourth, `tag`, for a tag such as `{{@html …}}`. This language has no tags, but a grammar with blocks names all four.

```js tpl.js
	sigils: {
		open: '#',
		branch: ':',
		close: '/',
		tag: '@',
		blocks: {},
	},
```

The block itself is built in five passes, one part of `{{#repeat item, i in items by item.id}}` at a time.

### The list and the body

What is the least a repeat block needs? The list to repeat over, and the content to repeat.

```js tpl.js
		blocks: {
			repeat: g.block(
				g.node('RepeatBlock', { list: g.js.expression }, { body: g.content }),
			),
		},
```
```notes
repeat :: The word after `{{#`. The block closes with `{{/repeat}}`.
'RepeatBlock' :: The node's type. After it come the block's fields, in the order the syntax writes them.
g.js.expression :: Reads JavaScript up to the `}}`, or up to the next word of the block's head once there is one.
body :: The block's content, everything up to `{{/repeat}}`. Content always comes last.
```

```js parse.js
new Source('{{#repeat items}}<Card />{{/repeat}}').parse(tpl).node.children[0];
// { type: 'RepeatBlock', list: { type: 'Identifier', name: 'items' },
//   body: [ { type: 'Component', name: 'Card', … } ] }
```

### The item

`item` names each element of the list. Is it a name the template uses, or one it declares? It declares it, for the body only. Can it be destructured, as in `{{#repeat { name } in items}}`? It should be. `g.js.pattern` reads a name or a destructuring pattern, and `g.bind` declares what it reads in the scope the block opens. The word `in` stands between the item and the list.

```js tpl.js
			repeat: g.block(
				g.node(
					'RepeatBlock',
					{ item: g.bind(g.js.pattern) },
					'in',
					{ list: g.js.expression },
					{ body: g.content },
				),
			),
```

With `scopes`, every use of `item` in the body refers to the item:

```js parse.js
import { Source, referenceOf } from '@teasel/parser';

const text = '{{#repeat item in items}}{{ item.name }}{{/repeat}}';
const { node } = new Source(text, { scopes: true }).parse(tpl);
const repeat = node.children[0];

const use = repeat.body[0].expression.object;    // the item in item.name
referenceOf(use).binding.node === repeat.item;   // true
referenceOf(repeat.list).binding;                // null: declared outside the file
```

### The index

Is the index always written? No, so it goes in `g.opt`, which reads it only when its first word, `,`, is there. When it is missing, should `index` be null or left out? `g.optional` leaves the field out. A position is one name, so it reads an identifier rather than a pattern.

```js tpl.js
					{ item: g.bind(g.js.pattern) },
					g.opt(',', { index: g.optional(g.bind(g.js.identifier)) }),
					'in',
```

```js parse.js
new Source('{{#repeat item, i in items}}…{{/repeat}}').parse(tpl).node.children[0];
// { type: 'RepeatBlock', item: { … name: 'item' }, index: { … name: 'i' },
//   list: { … name: 'items' }, body: [ … ] }

new Source('{{#repeat item in items}}…{{/repeat}}').parse(tpl).node.children[0];
// the same without index
```

### The key

Does `by item.id` declare a name? No, it uses `item`, so it reads an expression without `g.bind`. It is optional, as the index is. The key can use `item` because the scope a block opens starts at the first name the block declares, and covers everything read after it.

```js tpl.js
					{ list: g.js.expression },
					g.opt('by', { key: g.optional(g.js.expression) }),
					{ body: g.content },
```

```js parse.js
const text = '{{#repeat item in items by item.id}}…{{/repeat}}';
const repeat = new Source(text, { scopes: true }).parse(tpl).node.children[0];
referenceOf(repeat.key.object).binding.node === repeat.item;   // true
```

### The empty branch

What else can stand inside the block? `{{:empty}}` and the content after it. A branch is a word after `{{:`, with fields of its own; this one reads no JavaScript, only content. A repeat block without the branch has no `fallback` field.

```js tpl.js
			repeat: g.block(
				g.node(
					'RepeatBlock',
					// the fields from the steps above
				),
				{ branches: { empty: [{ fallback: g.optional(g.content) }] } },
			),
```
```notes
empty :: The word after `{{:`.
fallback :: The field the branch's content goes in.
```

## 5. The whole grammar

```js tpl.js
import * as g from '@teasel/parser/grammar';

export const tpl = g.grammar('tpl', {
	document: g.node('Template', { children: g.content }),
	text: g.node('Text', { data: g.text.data }),
	comment: g.node('Comment', { data: g.text.data }),
	delimiters: ['{{', '}}'],
	attributes: { expressions: true },
	elements: {
		fields: {
			name: g.element.tag,
			attributes: g.element.attributes,
			children: g.content,
		},
		component: g.element(g.node('Component')),
		other: g.element(g.node('Element')),
	},
	sigils: {
		open: '#',
		branch: ':',
		close: '/',
		tag: '@',
		blocks: {
			repeat: g.block(
				g.node(
					'RepeatBlock',
					{ item: g.bind(g.js.pattern) },
					g.opt(',', { index: g.optional(g.bind(g.js.identifier)) }),
					'in',
					{ list: g.js.expression },
					g.opt('by', { key: g.optional(g.js.expression) }),
					{ body: g.content },
				),
				{ branches: { empty: [{ fallback: g.optional(g.content) }] } },
			),
		},
	},
	expression: g.node('Expression', { expression: g.js.expression }),
});
```

Read `list.tpl` from the top of the page with it:

```js parse.js
const { node, bindings } = new Source(list, { scopes: true }).parse(tpl);
const repeat = node.children[0].children[1];   // after the text '\n\t'

repeat.fallback[1].name;                        // 'li'
bindings.map((b) => `${b.name}: ${b.kind}`);    // ['item: pattern', 'i: pattern']
```

A document the grammar does not describe throws a `ParseError` naming what it expected:

```js parse.js
new Source('{{#repeat item items}}…{{/repeat}}').parse(tpl);
// ParseError: Expected in, code 'expected', pos 15

new Source('{{#repeat item in items}}…').parse(tpl);
// ParseError: repeat is not closed, code 'unclosed', pos 0
```

## 6. Types

In TypeScript, and in JavaScript checked with JSDoc, every node is typed from the definition:

```ts types.ts
import type { NodeType } from '@teasel/parser/grammar';
import { tpl } from './tpl.js';

type Repeat = NodeType<typeof tpl, 'RepeatBlock'>;
// { type: 'RepeatBlock'; start: number; end: number; loc?: SourceLocation;
//   item: Pattern; index?: Identifier; list: Expression;
//   key?: Expression; body: Content[]; fallback?: Content[] }
```

`Content` is every node the language's content can hold: `Element`, `Component`, `Text`, `Comment`, `Expression` and `RepeatBlock`. A field left out by `g.optional` is optional in the type, and a field read by `g.js.pattern` is a `Pattern`.
