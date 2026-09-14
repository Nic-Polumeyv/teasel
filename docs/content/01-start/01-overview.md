---
title: Overview
---

*One pass, one tree, one crossing.*

A JavaScript and template parser written in Rust, shipped to JavaScript.

```bash
npm install @teasel/parser
```

## A first parse

Test. The whole source goes in once, and one tree comes out.

```js parse.js
import { Source } from '@teasel/parser';

using source = new Source('let x = 1', { sourceType: 'module' });
const { node } = source.parse();
```

### What comes back

- Test: an ESTree program.
- Test: comments and locations when asked.
- Test: scopes, bindings and references when asked.

Test paragraph with `inline code` and a [link to the next page](/getting-started).
