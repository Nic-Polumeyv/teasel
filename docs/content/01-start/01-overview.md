---
title: Overview
---

teasel is a parser for JavaScript and TypeScript. You give it text, it gives you a tree.

```js parse.js
import { Source, Plan } from '@teasel/parser';

const { node } = new Source('let answer = 42').parse();
```

The tree is ESTree: plain objects with a `type`, a `start` and an `end`. Code that walks an ESTree tree walks this one.

## Options

Options add to what a parse returns.

```js parse.js
const { node, scopes, bindings } = new Source(text, { scopes: true }).parse();
```

`scopes` adds what every name declares and what it refers to. Other options add the comments, the errors of a file that does not parse, and a TypeScript file with its types removed. [What parse returns](/what-parse-returns) lists every option and what it adds.

## Parse part of the text

A parse can start at an offset and read one expression, one statement or one pattern.

```js parse.js
source.parse(Plan.expression, 10);
```

A template language uses this to read the JavaScript inside its own syntax, and to stop at its own tokens: see [Embedded JavaScript](/embedded-javascript). A template language can also describe its whole syntax as a grammar, and get a whole file back as one tree: see [Parsing with a grammar](/parsing-with-a-grammar).

## Where it runs

Under Node it is a native addon. In a browser, or through a bundler, the same import is a WebAssembly build with the same API. [Getting started](/getting-started) is the first parse. [How it works](/how-it-works) explains what happens during a parse.
