---
title: Inside a host
---

A host language that embeds JavaScript, a template syntax say, reads one piece at a time from inside its own text: an expression at an offset, ended by the host's own tokens, and gets back where its syntax resumes.

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

The entries are what a host can ask for at an offset:

```text
source.parse('expression', 7)             the expression that starts at 7
source.parse('pattern', 7)                an assignment target: a name or a destructuring
source.parse('params', 7)                 the patterns of a (a, b = 1)
source.parse('statement', 7)              one statement
source.parse('typeParameters', 7)         a <T extends U>
source.parse('program', 12, { end: 40 })  the program inside 12..40, positions of the whole
```

A `typeParameters` entry reads TypeScript only; without `typescript` it is a `not_typescript` error.

## Stop tokens

`stopAt` lists the host's own tokens, words or punctuators. One read outside every bracket the parse opened, where the expression could end, ends the parse: `,` does not start a sequence, and `/>` is not a division. A `then` after `.` is a property name. A TypeScript `as` is the host's unless another `as` follows the assertion, so `xs as T[] as item` ends after the type.

## One source, many pieces

Make the `Source` once for the whole text and parse every piece from it. The text crosses into the engine once, and every answer's positions are offsets into that whole text, so nothing needs translating back.
