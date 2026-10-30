---
title: Getting started
---

Install the package and call `parse` on your text.

```bash
npm install @teasel/parser
```

```js parse.js
import { parse, parseAt, js } from '@teasel/parser';

const { node } = parse('let answer = 42', js, { sourceType: 'module' });

node.type;                 // 'Program'
node.body[0].type;         // 'VariableDeclaration'
```

`node` is an ESTree `Program`, made of plain objects.

## Parse part of the text

`parseAt` reads one piece of the text, starting at an offset. Each call copies the whole text into the parser, so read a whole file with one `parse`.

```js parse.js
parse('const x = f(a, b)').node.type;                     // 'Program'
parseAt('const x = f(a, b)', js.expression, 10).node.type; // 'CallExpression', the f(a, b)
```

## In the browser

The same import works in a browser, directly or through a bundler. There it is a WebAssembly build, not the native addon, with the same API.

```js browser.js
import { parse, js } from '@teasel/parser';

const { node } = parse('export const answer = 42', js, { sourceType: 'module' });
```

The build looks for `teasel.wasm` beside the package entry, through `import.meta.url`, so a bundler includes the file without configuration. A bundle built for Node loads the native addon, from `@teasel/parser-<platform>` in `node_modules`, as Node does. Under Node, `node --no-addons` takes the WebAssembly build instead of the addon, without a warning. The Linux addons need glibc 2.28 or newer, as Node's own Linux builds do. Node also takes the WebAssembly build where no addon loads, such as on a Linux without glibc, and warns if the platform has an addon that is not installed or does not load.

Under `node --permission`, the package needs `--allow-fs-read` for its directory, and `--allow-addons` for the native addon, which also reads `@teasel/parser-<platform>`: `--allow-fs-read=node_modules/@teasel/*` covers both. Without `--allow-addons` the WebAssembly build runs, silently.

## Next

[What parse returns](/what-parse-returns) lists what a parse returns and what each option adds.
