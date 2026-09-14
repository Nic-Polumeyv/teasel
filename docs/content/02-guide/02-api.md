---
title: API
---

## Source

Test. A source kept with its options; the parses out of it share the source copy and the position tables.

```ts index.d.ts
export class Source {
  constructor(source: string, options?: Options);
  parse<T>(plan?: Plan<T>, at?: number): Parsed<T>;
  [Symbol.dispose](): void;
}
```

## Options

- `sourceType`: test.
- `typescript`: test.
- `scopes`: test.
