---
title: Getting started
---

Install the package, make a `Source` from your text, and call `parse`.

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

`node` is an ESTree `Program`, made of plain objects.

## Parse the same text again

The `Source` keeps your text, so you can parse it any number of times: the whole text, or one piece that starts at an offset.

```js parse.js
const source = new Source('const x = f(a, b)');

source.parse().node.type;                    // 'Program'
source.parse(Plan.expression, 10).node.type; // 'CallExpression', the f(a, b)
```

## Release a Source

Release a `Source` when you are done with it. `using` releases it at the end of the block.

```js parse.js
using source = new Source(text);
```

Node versions before 24 do not have `using`: call `source[Symbol.dispose]()` in a `finally` there. A source left to the garbage collector is released late, and finalizing it costs more than parsing a small file did.

## In the browser

The same import works in a browser or behind a bundler. There it is a WebAssembly build, not the native addon, with the same API.

```js browser.js
import { Source, Plan } from '@teasel/parser';

const { node } = new Source('export const answer = 42').parse();
```

The build looks for `teasel.wasm` next to its own module, through `import.meta.url`, so a bundler includes the file without configuration. Under Node, `node --no-addons` takes the same build instead of the addon; on a Linux without glibc there is no addon, and the error says so and names this flag.

## Next

[What parse returns](/what-parse-returns) lists what a parse returns and what each option adds.
