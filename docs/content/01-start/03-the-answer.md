---
title: The answer
---

A parse answers with the node and where it ended, and with a table for each option that asks for more.

```js
const { node, end } = source.parse();
```

![the answer is an object with node and end, and with comments, errors, typescript, scopes, bindings and references tables when their option is on](Answer.svelte)

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
