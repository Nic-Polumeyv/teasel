---
title: The answer
---

A parse gives you the node, where it stopped, and one table for each option you turned on.

```js
const { node, end } = source.parse();
```

![the answer is an object with node and end, and with comments, errors, typescript, scopes, bindings and references tables when their option is on](Answer.svelte)

`node` is the tree. Every node carries `start` and `end`, UTF-16 offsets into the whole source exactly as acorn counts them, and they stay offsets into the whole source no matter how small a piece you parsed. `end` is where the parse stopped reading: after the node, after its closing parentheses, after any comments trailing it. For a whole program it's simply the end you gave it.

You get a key on the answer exactly when its option is on. Nothing appears that you didn't ask for.

- `comments` adds `comments`, every comment in source order, and hangs `leadingComments`, `trailingComments` and `innerComments` on the nodes.
- `locations` adds `loc` with line and column to every node.
- `scopes` adds `scopes`, `bindings` and `references`, and switches on the four questions in [Scopes](/scopes).
- `errorRecovery` adds `errors` and returns instead of throwing. See [Errors](/errors).
- `typescript: 'erase'` adds `typescript`, the list of things erasure couldn't remove. See [TypeScript](/typescript).
- `parenthesized` marks a node you wrapped in parentheses with `parenthesized: true`.
- `sourceType: 'module'` reads strict code, with `import`, `export` and top-level `await`.

Every option, with its type and default, is in the [reference](/reference/parser).
