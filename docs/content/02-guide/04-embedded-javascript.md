---
title: Embedded JavaScript
---

A template language has JavaScript inside its own syntax. To read it, a parser has to start at an offset, read one expression, stop at a token that belongs to the template language, and report where it stopped. These pages call the template language the host.

> **`parseAt` copies the whole text into the parser on every call.** Reading a file piece by piece, one call per expression, copies the file once per expression, and each piece's names resolve without the rest of the file. The parser is built to answer a whole text: for a template language, describe its syntax as a [grammar](/parsing-with-a-grammar) and `parse` the file once.

```text
{{ items as item, index }}
   ▲    ▲
   3    8
```

```js template.js
const { node, end } = parseAt('{{ items as item, index }}', js.expression.until('as', ','), 3);
// node  Identifier items
// end   8
```

The parse started at offset 3, read an expression instead of a program, and stopped at `as` because `until` named it.

## Pieces

The second argument of `parseAt` is the piece to read, and the third is where: an offset, or a `[start, end]` range.

```text
parseAt(text, js.expression, 7)        the expression starting at 7
parseAt(text, js.statement, 7)         one statement
parseAt(text, js.pattern, 7)           a name or a destructuring pattern
parseAt(text, js.params, 7)            the patterns inside (a, b = 1)
parseAt(text, js.typeParameters, 7)    a <T extends U>, with typescript on
parseAt(text, js.program, [12, 40])    the program between 12 and 40
```

Positions in every answer are offsets into the whole text, so nothing has to be added to them.

## until

`until` takes your own tokens, words or punctuators, and gives a piece that ends there. When the parser reads one at a point where the expression could end, outside every bracket it opened, the parse ends.

So a `,` named in `until` does not start a sequence expression, and a `/>` named in `until` is not read as a division. Inside brackets they keep their JavaScript meaning, so `f(a, b)` reads whole. A piece holds no text: build `js.expression.until('}')` once, at module level, and use it in every parse.

## Names in a piece

With `scopes` on, the answer for a piece has `bindings` and `references`, as the answer for a whole program does. [Scopes](/scopes) explains both. The parser reads only the piece, and that shows in two places.

A name the piece declares itself resolves to that declaration. A name declared elsewhere in your document has `binding: null`, as a global has: the parser has not read the text that declares it. Looking those names up is your code's work, since it knows the scoping rules of your document.

```js template.js
const { references } = parseAt('{{ items.map((x) => x + offset) }}', js.expression.until('}'), 3, { scopes: true });
// items   binding: null        declared elsewhere, or a global
// x       binding: { name: 'x', kind: 'param', … }
// offset  binding: null
```

A `js.pattern` piece declares names for your document. Its `bindings` has one entry per name, with `kind: 'pattern'`.

```js template.js
const { bindings } = parseAt('{{ items as { id, name }, index }}', js.pattern.until(',', '}'), 12, { scopes: true });
// bindings  id and name, both kind: 'pattern'
```

[Svelte component](/svelte-component) puts the two together: it keeps the bindings that patterns declare, and resolves the `null` names against them.

## Edge cases

A `then` after a `.` is a property name, not your token. A TypeScript `as` is yours unless another `as` follows the type assertion, so `xs as T[] as item` ends after the type. `typeParameters` without `typescript` is a `not_typescript` error. And `js.program` takes no `until`: a program reads to the end of its range.

## The alternative: a grammar

With the approach on this page, your own code reads the template syntax and asks the parser for each piece of JavaScript. The alternative is to describe the template syntax to the parser once, as a grammar, and get the whole file back as one tree from one parse, with scopes that cover the template's blocks and the JavaScript in them. See [Parsing with a grammar](/parsing-with-a-grammar).
