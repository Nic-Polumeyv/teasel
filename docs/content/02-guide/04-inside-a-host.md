---
title: Inside a host
---

A template language has JavaScript in it, but it isn't JavaScript. What it needs from a parser is different: read one expression starting here, stop when you reach one of my tokens, and tell me where you stopped.

```text
{{ items as item, index }}
   ▲    ▲
   3    8
```

```js
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

## One source, many pieces

Make one `Source` for the whole document and parse every piece out of it. The text goes in once. Every answer's positions already fit the document. [A component, piece by piece](/a-component) does this for a whole component, script block, `each` heads and all.

## Fine print

A `then` after a `.` is a property name, not your token. A TypeScript `as` is yours unless another `as` follows the type assertion, so `xs as T[] as item` ends after the type. And `typeParameters` without `typescript` is a `not_typescript` error.
