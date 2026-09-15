---
title: The answer
---

A parse answers with the node and where it ended, and with a table for each option that asks for more.

```js
const { node, end } = source.parse();
```

<svg viewBox="0 0 720 200" width="100%" role="img" aria-label="the answer is an object with node and end, and with comments, errors, typescript, scopes, bindings and references tables when their option is on" fill="none" stroke="currentColor" stroke-width="1.5" font-size="14" class="my-8">
  <rect x="8" y="8" width="704" height="184" rx="10" stroke-dasharray="4 4"/>
  <text x="24" y="32" fill="currentColor" stroke="none" font-family="monospace">{ node, end, …tables }</text>
  <rect x="24" y="52" width="200" height="124" rx="8" class="fill-sky-400/15 stroke-sky-400/60"/><text x="124" y="80" text-anchor="middle" fill="currentColor" stroke="none">node</text>
  <text x="124" y="104" text-anchor="middle" fill="currentColor" stroke="none" font-size="12" class="text-muted-foreground">an ESTree tree</text>
  <text x="124" y="124" text-anchor="middle" fill="currentColor" stroke="none" font-size="12" class="text-muted-foreground">start, end on every node</text>
  <text x="124" y="144" text-anchor="middle" fill="currentColor" stroke="none" font-size="12" class="text-muted-foreground">loc with locations</text>
  <rect x="248" y="52" width="120" height="52" rx="8" class="fill-amber-400/15 stroke-amber-400/60"/><text x="308" y="83" text-anchor="middle" fill="currentColor" stroke="none">end</text>
  <rect x="392" y="52" width="140" height="36" rx="8" class="fill-emerald-400/15 stroke-emerald-400/60"/><text x="462" y="75" text-anchor="middle" fill="currentColor" stroke="none" font-size="13">comments</text>
  <rect x="392" y="96" width="140" height="36" rx="8" class="fill-emerald-400/15 stroke-emerald-400/60"/><text x="462" y="119" text-anchor="middle" fill="currentColor" stroke="none" font-size="13">errors</text>
  <rect x="392" y="140" width="140" height="36" rx="8" class="fill-emerald-400/15 stroke-emerald-400/60"/><text x="462" y="163" text-anchor="middle" fill="currentColor" stroke="none" font-size="13">typescript</text>
  <rect x="556" y="52" width="140" height="36" rx="8" class="fill-rose-400/15 stroke-rose-400/60"/><text x="626" y="75" text-anchor="middle" fill="currentColor" stroke="none" font-size="13">scopes</text>
  <rect x="556" y="96" width="140" height="36" rx="8" class="fill-rose-400/15 stroke-rose-400/60"/><text x="626" y="119" text-anchor="middle" fill="currentColor" stroke="none" font-size="13">bindings</text>
  <rect x="556" y="140" width="140" height="36" rx="8" class="fill-rose-400/15 stroke-rose-400/60"/><text x="626" y="163" text-anchor="middle" fill="currentColor" stroke="none" font-size="13">references</text>
</svg>

`node` is the tree. Every node has `start` and `end`, UTF-16 offsets into the whole source, as in acorn, and positions stay those of the whole source however small the piece parsed. `end` is the offset after everything the parse consumed: the node, its closing parentheses and the comments after it. For a program, it is the end the parse was given.

A key is on the answer exactly when its option is on:

- `comments` adds `comments`, every comment read in source order, and attaches `leadingComments`, `trailingComments` and `innerComments` to the nodes.
- `locations` adds `loc` with line and column to every node.
- `scopes` adds `scopes`, `bindings` and `references`, and turns on the four questions of [Scopes](/scopes).
- `errorRecovery` adds `errors` and returns instead of throwing; see [Errors](/errors).
- `typescript: 'erase'` adds `typescript`, what erasure had to leave in place; see [TypeScript](/typescript).
- `parenthesized` marks a node the source wraps in parentheses with `parenthesized: true`.
- `sourceType: 'module'` reads strict code with `import`, `export` and top-level `await`.

The [reference](/reference/parser) lists every option with its type and default.
