---
title: Embedded JavaScript
---

A template language has JavaScript inside its own syntax. To read it, a parser has to start at an offset, read one expression, stop at a token that belongs to the template language, and report where it stopped. These pages call the template language the host.

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

The parse started at offset 3, read an expression instead of a program, and stopped at `as` because `until` named it.

## Plans

The first argument of `parse` is the plan, which says what to read. The second is the position.

```text
source.parse(Plan.expression, 7)        the expression starting at 7
source.parse(Plan.statement, 7)         one statement
source.parse(Plan.pattern, 7)           a name or a destructuring pattern
source.parse(Plan.params, 7)            the patterns inside (a, b = 1)
source.parse(Plan.typeParameters, 7)    a <T extends U>, with typescript on
source.parse(Plan.stylesheet)           the whole source as a CSS stylesheet
source.parse(Plan.program, [12, 40])    the program between 12 and 40
```

Positions in every answer are offsets into the whole text, so nothing has to be added to them.

## until

`until` takes your own tokens, words or punctuators, and gives a plan that ends there. When the parser reads one at a point where the expression could end, outside every bracket it opened, the parse ends.

So a `,` named in `until` does not start a sequence expression, and a `/>` named in `until` is not read as a division. Inside brackets they keep their JavaScript meaning, so `f(a, b)` reads whole. A plan does not hold a source: build `Plan.expression.until('}')` once, at module level, and use it for every expression in every file.

## Names in a piece

With `scopes` on, the answer for a piece has `bindings` and `references`, as the answer for a whole program does. [Scopes](/scopes) explains both. The parser reads only the piece, and that shows in two places.

A name the piece declares itself resolves to that declaration. A name declared elsewhere in your document has `binding: null`, as a global has: the parser has not read the text that declares it. Looking those names up is your code's work, since it knows the scoping rules of your document.

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

[Svelte component](/svelte-component) puts the two together: it keeps the bindings that patterns declare, and resolves the `null` names against them.

## One Source per file

Make one `Source` for the whole file and parse every piece from it. The text is copied into the parser once, and every answer's positions are offsets into the file. [Svelte component](/svelte-component) does this for a whole component.

## Edge cases

A `then` after a `.` is a property name, not your token. A TypeScript `as` is yours unless another `as` follows the type assertion, so `xs as T[] as item` ends after the type. And `typeParameters` without `typescript` is a `not_typescript` error.

## The alternative: a grammar

With the approach on this page, your own code reads the template syntax and asks the parser for each piece of JavaScript. The alternative is to describe the template syntax to the parser once, as a grammar, and get the whole file back as one tree, with scopes that cover the template's blocks and the JavaScript in them. See [Parsing with a grammar](/parsing-with-a-grammar).
