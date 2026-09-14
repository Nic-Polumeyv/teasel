---
title: Getting started
---

Test. Install the package, then parse.

```bash
npm install @teasel/parser
```

## Node and the browser

Test. Node loads the native binding, everything else loads the wasm build. Same API either way.

```js browser.js
import { Source } from '@teasel/parser/wasm';

const source = new Source('export const answer = 42');
console.log(source.parse().node.body[0].type);
```
