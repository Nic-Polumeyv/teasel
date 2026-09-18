---
title: Getting started
---

Install it, wrap some text in a `Source`, parse.

```bash
npm install @teasel/parser
```

```js parse.js
import { Source, Plan } from '@teasel/parser';

const source = new Source('let answer = 42', { sourceType: 'module' });
const { node } = source.parse();

node.type;                 // 'Program'
node.body[0].type;         // 'VariableDeclaration'
```

That's an ESTree `Program`, plain objects all the way down.

## Parse it again

The `Source` keeps your text. Parse it as many times as you like: the whole thing, or one piece starting at an offset.

```js
const source = new Source('const x = f(a, b)');

source.parse().node.type;                    // 'Program'
source.parse(Plan.expression, 10).node.type; // 'CallExpression', the f(a, b)
```

Release a source when you're done with it. `using` does that at the end of the block.

```js
using source = new Source(text);
```

If you're on a Node version older than 24, `using` isn't there yet: call `source[Symbol.dispose]()` in a `finally` instead. A source left to the garbage collector is released late, and finalizing it costs more than parsing a small file did.

## In the browser

The same import works in a browser or behind a bundler. There it's a WebAssembly build instead of a native addon, with the same API.

```js browser.js
import { Source, Plan } from '@teasel/parser';

const { node } = new Source('export const answer = 42').parse();
```

The build looks for `teasel.wasm` next to its own module, through `import.meta.url`, so a bundler copies the file along without being asked. Under Node, `node --no-addons` takes the same build instead of the addon; a Linux without glibc, where there is no addon, is told so and pointed there.

## Next

[The answer](/the-answer) is what a parse gives back, and what each option adds to it.
