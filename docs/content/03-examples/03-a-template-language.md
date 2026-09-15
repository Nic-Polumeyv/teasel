---
title: A template language
---

Say you're writing a template language: text with expressions in `{{ }}`. You don't want to write an expression parser, and you don't want to cut the expressions out and parse them as separate strings either, because then every position you get back is wrong.

Make one `Source` of the whole document, and parse each expression in place.

```js
import { Source } from '@teasel/parser';

const text = 'Hello {{ user.name }}, you have {{ count }} new {{ count === 1 ? "message" : "messages" }}.';
const source = new Source(text);

const pieces = [];
for (let at = text.indexOf('{{'); at !== -1; at = text.indexOf('{{', at)) {
	const { node, end } = source.parse('expression', at + 2, { stopAt: ['}}'] });
	pieces.push(node);
	at = end;
}

pieces.map((n) => n.type);                          // ['MemberExpression', 'Identifier', 'ConditionalExpression']
pieces.map((n) => text.slice(n.start, n.end));      // ['user.name', 'count', 'count === 1 ? "message" : "messages"']
```

Your scanner finds the opening braces. For each one, the parser reads an expression starting just after them, and `}}` in `stopAt` tells it where your syntax takes over again. It returns `end`, and you continue from there.

![the document as one strip of text; three expressions are cut from it by parses that start at an offset and stop at the closing braces; each answer's node carries offsets into the whole strip](Template.svelte)

Nothing was copied out. The third expression has a `?`, a `:` and two strings in it, and the parser didn't stop at any of them, because it stops only where an expression could end and a `}}` is what it sees. The `start` and `end` on every node are offsets into the document, so an error message, a source map or a highlight needs no translation.

The [entries](/inside-a-host#what-to-ask-for) go further than expressions. A `pattern` reads the `item` in `{{ each items as item }}`, a `params` reads a snippet's parameters, and `program` reads a whole script block between two offsets.
