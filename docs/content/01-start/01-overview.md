---
title: Overview
---

teasel is a parser for JavaScript and TypeScript. You give it text, it gives you a tree.

```js parse.js
import { parse, parseAt, js } from '@teasel/parser';

const { node } = parse('let answer = 42');
```

The tree is ESTree: plain objects with a `type`, a `start` and an `end`. Code that walks an ESTree tree walks this one.

## Options

Options add to what a parse returns.

```js parse.js
const { node, scopes, bindings } = parse(text, { scopes: true });
```

`scopes` adds what every name declares and what it refers to. Other options add the comments, the errors of a file that does not parse, and a TypeScript file with its types removed. [What parse returns](/what-parse-returns) lists every option and what it adds.

## Parse part of the text

`parseAt` starts at an offset and reads one expression, one statement or one pattern.

```js parse.js
parseAt(text, js.expression, 10);
```

A template language describes its whole syntax as a grammar and gets the whole file back as one tree, in one parse: see [Parsing with a grammar](/parsing-with-a-grammar). `parseAt` reads it piece by piece instead: see [Embedded JavaScript](/embedded-javascript).

## Where it runs

Under Node it is a native addon, and the WebAssembly build where the addon does not load. In a browser, directly or through a bundler, the same import is a WebAssembly build with the same API. [Getting started](/getting-started) is the first parse. [How it works](/how-it-works) explains what happens during a parse.
