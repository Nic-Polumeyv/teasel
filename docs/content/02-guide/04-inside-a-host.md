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
const { node, end } = source.parse('expression', 3, { stopAt: ['as', ','] });
// node  Identifier items
// end   8
```

Three things happened there. The parse started at an offset. It was asked for an expression, not a program. And it stopped at `as`, because you said so.

## What to ask for

The first argument is the entry, what you want read at that offset.

```text
source.parse('expression', 7)             the expression starting at 7
source.parse('statement', 7)              one statement
source.parse('pattern', 7)                an assignment target: a name or a destructuring
source.parse('params', 7)                 the patterns inside (a, b = 1)
source.parse('typeParameters', 7)         a <T extends U>, with typescript on
source.parse('program', 12, { end: 40 })  the program between 12 and 40
```

Positions in every answer are offsets into the whole text, so there's nothing to add back afterwards.

## Where to stop

`stopAt` is the list of your own tokens, words or punctuators. When the parser reads one at a point where the expression could end, outside every bracket it opened, the parse ends there.

That's why `,` in `stopAt` doesn't start a sequence expression, and `/>` isn't a division: they're yours. Inside brackets they're still JavaScript, so `f(a, b)` reads whole.

## One source, many pieces

Make one `Source` for the whole document and parse every piece out of it. The text goes in once. Every answer's positions already fit the document.

## Fine print

A `then` after a `.` is a property name, not your token. A TypeScript `as` is yours unless another `as` follows the type assertion, so `xs as T[] as item` ends after the type. And `typeParameters` without `typescript` is a `not_typescript` error.
