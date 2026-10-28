---
title: Getting started
---

Install the package, make a `Source` from your text, and call `parse`.

```bash
npm install @teasel/parser
```

```js parse.js
import { Source, js } from '@teasel/parser';

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

source.parse().node.type;                     // 'Program'
source.parse(js.expression, 10).node.type; // 'CallExpression', the f(a, b)
```

## In the browser

The same import works in a browser, directly or through a bundler. There it is a WebAssembly build, not the native addon, with the same API.

```js browser.js
import { Source, js } from '@teasel/parser';

const { node } = new Source('export const answer = 42').parse();
```

The build looks for `teasel.wasm` beside the package entry, through `import.meta.url`, so a bundler includes the file without configuration. A bundle built for Node loads the native addon, from `@teasel/parser-<platform>` in `node_modules`, as Node does. Under Node, `node --no-addons` takes the WebAssembly build instead of the addon, without a warning. Node also takes it where no addon loads, such as on a Linux without glibc, and warns if the platform has an addon that is not installed or does not load.

Under `node --permission`, the package needs `--allow-fs-read` for its directory, and `--allow-addons` for the native addon, which also reads `@teasel/parser-<platform>`: `--allow-fs-read=node_modules/@teasel/*` covers both. Without `--allow-addons` the WebAssembly build runs, silently.

## Next

[What parse returns](/what-parse-returns) lists what a parse returns and what each option adds.
