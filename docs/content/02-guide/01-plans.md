---
title: Plans
---

Test. A plan describes a host language, a template language with JavaScript inside it, and crosses into the engine once.

```js plan.js
import { Plan, Source } from '@teasel/parser';

const plan = new Plan(grammar);
const { node, roots } = new Source(text).parse(plan);
```

## Rules

Test. Each rule names a node type and the fields it reads.

```ts grammar.ts
export const grammar = {
  Text: rule.token(TEXT),
  Mustache: rule.seq('{', expr(), '}'),
};
```
