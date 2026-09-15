---
title: Getting started
---

Install the package, make a `Source` of some text, and parse it.

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

`node` is an ESTree `Program`, the same shape acorn returns, so anything that walks an acorn tree walks this one.

## A source, then its parses

The text crosses into the engine when the `Source` is made, and stays there until the source is released. Parsing is a separate step, and a source can be parsed more than once: the whole program, or a piece at an offset.

```js
const source = new Source('const x = f(a, b)');

source.parse().node.type;                 // 'Program'
source.parse('expression', 10).node.type; // 'CallExpression', the f(a, b)
```

Release a source with `using` when the block ends, or leave it to the garbage collector.

```js
using source = new Source(text);
```

## Node and the browser

Under Node, `@teasel/parser` resolves to a native addon. In a browser or through a bundler, the same import resolves to a WebAssembly build. The API is the same; only the engine underneath differs.

```js browser.js
import { Source } from '@teasel/parser';

const { node } = new Source('export const answer = 42').parse();
```

To pick the WebAssembly build under Node as well, import `@teasel/parser/wasm`.

## Next

[The answer](/the-answer) explains what comes back from a parse and what each option adds to it.
