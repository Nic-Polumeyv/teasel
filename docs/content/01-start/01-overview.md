---
title: Overview
---

*You make a source once. Then you ask it things.*

teasel is a parser for JavaScript and TypeScript. You give it text, it gives you a tree.

```js
import { Source, Plan } from '@teasel/parser';

const { node } = new Source('let answer = 42').parse();
```

The tree is ESTree, plain objects with a `type`, a `start` and an `end`, so anything you already have that walks an ESTree walks this one.

## Ask for more

The parse that built the tree also knows things about it. Ask, and they come back with the same answer.

```js
const { node, scopes, bindings } = new Source(text, { scopes: true }).parse();
```

Each option adds something to the answer: which `x` is which, where every comment sits, where the errors are when the file is half typed, what a TypeScript file looks like with its types stripped. [The answer](/the-answer) shows what comes back and what each option adds to it.

## Read a piece

A parse doesn't have to be the whole text. Start it at an offset, and ask for an expression, a statement, a pattern.

```js
source.parse(Plan.expression, 10);
```

That's how a template language reads the JavaScript inside its own syntax, one piece at a time, stopping at its own tokens. [Inside a host](/inside-a-host) is about that. A template language can also hand over its whole syntax and get the whole document back in one tree, scopes across both; that's [A document](/a-document).

## Where it runs

Under Node it's a native addon. In a browser, or through a bundler, the same import gives you a WebAssembly build with the same API. [Getting started](/getting-started) is the first parse; [How it works](/how-it-works) is what happens between the call and the answer.
