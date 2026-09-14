---
title: API
---

## Source

Test. A source kept with its options; the parses out of it share the source copy and the position tables.

```ts index.d.ts
export class Source<Root = Program> {
  constructor(source: string, options?: Options);
  parse(entry?: 'program', offset?: number, at?: At): Parsed<Root>;
  parse(entry: 'expression', offset: number, at?: At): Parsed<Expression>;
  [Symbol.dispose](): void;
}
```

## Options

- `sourceType`: test.
- `typescript`: test.
- `scopes`: test.
