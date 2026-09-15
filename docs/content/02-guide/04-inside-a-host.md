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

The first argument is the entry, what you're asking for at that offset:

```text
source.parse('expression', 7)             the expression starting at 7
source.parse('pattern', 7)                an assignment target: a name or a destructuring
source.parse('params', 7)                 the patterns inside (a, b = 1)
source.parse('statement', 7)              one statement
source.parse('typeParameters', 7)         a <T extends U>
source.parse('program', 12, { end: 40 })  the program between 12 and 40, positions still of the whole
```

`typeParameters` only makes sense with `typescript` on; without it you get a `not_typescript` error.

## Stop tokens

`stopAt` is the list of your own tokens, words or punctuators. When the parser reads one outside every bracket it opened, at a point where the expression could end, the parse ends. So a `,` in `stopAt` doesn't start a sequence expression, and `/>` isn't a division. A `then` after a `.` is still a property name. And a TypeScript `as` is yours unless another `as` follows the type assertion, so `xs as T[] as item` ends after the type, the way you'd want.

## One source, many pieces

Make one `Source` for the whole document and parse every piece out of it. The text crosses once, and every answer's positions are offsets into that same text, so there's nothing to add back or translate.
