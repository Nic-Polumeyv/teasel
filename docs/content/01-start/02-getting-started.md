---
title: Getting started
---

Install it, wrap some text in a `Source`, parse.

```bash
npm install @teasel/parser
```

```js parse.js
import { Source } from '@teasel/parser';

const source = new Source('let answer = 42', { sourceType: 'module' });
const { node } = source.parse();

node.type;                 // 'Program'
node.body[0].type;         // 'VariableDeclaration'
```

That's an ESTree `Program`, plain objects all the way down. Any walker or visitor written for ESTree takes it as it is.

## Why a source and then a parse

The text is copied into the engine when you make the `Source`, and that's the last time it crosses. Every parse after that works on the copy, which is why parsing is its own step: you can parse the whole thing, or one piece at an offset, as many times as you like, and the text never moves again.

```js
const source = new Source('const x = f(a, b)');

source.parse().node.type;                 // 'Program'
source.parse('expression', 10).node.type; // 'CallExpression', the f(a, b)
```

When you're done, `using` releases the engine's copy at the end of the block. Or don't bother, and the garbage collector does it.

```js
using source = new Source(text);
```

## Node and the browser

In Node, `@teasel/parser` is a native addon. In a browser or behind a bundler, the same import is a WebAssembly build. Same API, different engine underneath.

```js browser.js
import { Source } from '@teasel/parser';

const { node } = new Source('export const answer = 42').parse();
```

The WebAssembly build finds `teasel.wasm` next to its own module with `new URL('./teasel.wasm', import.meta.url)`, so a bundler copies it along without being asked. If you want that build under Node too, import `@teasel/parser/wasm`.

## Next

[The answer](/the-answer) is what a parse gives back, and what each option adds to it.
