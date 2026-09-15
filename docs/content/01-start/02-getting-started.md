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

That's an ESTree `Program`, plain objects all the way down.

## Parse it again

The `Source` keeps your text. Parse it as many times as you like: the whole thing, or one piece starting at an offset.

```js
const source = new Source('const x = f(a, b)');

source.parse().node.type;                 // 'Program'
source.parse('expression', 10).node.type; // 'CallExpression', the f(a, b)
```

When you're done with a source, `using` lets it go at the end of the block. Where `using` isn't available, it's Node 24 and later, call `source[Symbol.dispose]()` yourself, in a `finally`. Or don't bother, and the garbage collector does it, late and at a cost that shows when you parse a file at a time.

```js
using source = new Source(text);
```

## In the browser

The same import works in a browser or behind a bundler. There it's a WebAssembly build instead of a native addon, with the same API.

```js browser.js
import { Source } from '@teasel/parser';

const { node } = new Source('export const answer = 42').parse();
```

The build looks for `teasel.wasm` next to its own module, through `import.meta.url`, so a bundler copies the file along without being asked. To use that build under Node as well, import `@teasel/parser/wasm`.

## Next

[The answer](/the-answer) is what a parse gives back, and what each option adds to it.
