---
title: Inside a host
---

A template language has JavaScript in it, but it isn't JavaScript. What it needs from a parser is different: read one expression starting here, stop when you reach one of my tokens, and tell me where you stopped.

```text
{{ items as item, index }}
   ▲    ▲
   3    8
```

```js template.js
const source = new Source('{{ items as item, index }}');
const { node, end } = source.parse(Plan.expression.until('as', ','), 3);
// node  Identifier items
// end   8
```

Three things happened there. The parse started at an offset. It was asked for an expression, not a program. And it stopped at `as`, because you said so.

## What to ask for

The first argument is the plan, what you want read; the second is where.

```text
source.parse(Plan.expression, 7)          the expression starting at 7
source.parse(Plan.statement, 7)           one statement
source.parse(Plan.pattern, 7)             an assignment target: a name or a destructuring
source.parse(Plan.params, 7)              the patterns inside (a, b = 1)
source.parse(Plan.typeParameters, 7)      a <T extends U>, with typescript on
source.parse(Plan.program, [12, 40])      the program between 12 and 40
```

Positions in every answer are offsets into the whole text, so there's nothing to add back afterwards.

## Where to stop

`until` takes your own tokens, words or punctuators, and gives a plan that ends there. When the parser reads one at a point where the expression could end, outside every bracket it opened, the parse ends.

That's why `,` in `until` doesn't start a sequence expression, and `/>` isn't a division: they're yours. Inside brackets they're still JavaScript, so `f(a, b)` reads whole. A plan has no source in it: build `Plan.expression.until('}')` once, at module level, and use it for every expression in every document.

## Names in a piece

With `scopes` on, the answer for a piece has `bindings` and `references`, as the answer for a whole program does. [Scopes](/scopes) explains both. The parser reads only the piece, and that shows in two places.

A name the piece declares itself resolves as usual. A name declared elsewhere in your document has `binding: null`, as a global has: the parser has not read the text that declares it. Looking those names up is your code's work, since it knows the scoping rules of your document.

```js template.js
const source = new Source('{{ items.map((x) => x + offset) }}', { scopes: true });
const { references } = source.parse(Plan.expression.until('}'), 3);
// items   binding: null        declared elsewhere, or a global
// x       binding: { name: 'x', kind: 'param', … }
// offset  binding: null
```

A `Plan.pattern` piece declares names for your document. Its `bindings` has one entry per name, with `kind: 'pattern'`.

```js template.js
const source = new Source('{{ items as { id, name }, index }}', { scopes: true });
const { bindings } = source.parse(Plan.pattern.until(',', '}'), 12);
// bindings  id and name, both kind: 'pattern'
```

[A component, piece by piece](/a-component) puts the two together: it keeps the bindings that patterns declare, and resolves the `null` names against them.

## One source, many pieces

Make one `Source` for the whole document and parse every piece out of it. The text goes in once. Every answer's positions already fit the document. [A component, piece by piece](/a-component) does this for a whole component, script block, `each` heads and all.

## Fine print

A `then` after a `.` is a property name, not your token. A TypeScript `as` is yours unless another `as` follows the type assertion, so `xs as T[] as item` ends after the type. And `typeParameters` without `typescript` is a `not_typescript` error.

## Or hand over the syntax

Reading piece by piece keeps the host's own parser in charge. The other way is to describe the host's syntax to the parser once and get the whole document back in one tree, scopes across the host's blocks and the JavaScript in them. That's [A document](/a-document).
