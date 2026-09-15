---
title: The answer
---

A parse gives you two things: the node, and where it stopped.

```js
const { node, end } = source.parse();
```

`node` is the tree. Every node in it has `start` and `end`, offsets into the text you gave the `Source`. `end` is where the parse stopped reading, just past the last thing it read. For a whole program that's the end of the text.

## What an option adds

Turn on an option and the answer grows by one key. Nothing appears that you didn't ask for.

![the answer is an object with node and end, and with comments, errors, typescript, scopes, bindings and references tables when their option is on](Answer.svelte)

- `locations` puts `loc`, with line and column, on every node.
- `comments` adds `comments`, every comment in source order, and hangs `leadingComments`, `trailingComments` and `innerComments` on the nodes.
- `scopes` adds `scopes`, `bindings` and `references`, and lets you ask any node what it declares or refers to. That's [Scopes](/scopes).
- `errorRecovery` adds `errors` and returns a tree instead of throwing. That's [Errors](/errors).
- `typescript` reads TypeScript, and `typescript: 'erase'` also strips it, adding `typescript`, the list of what erasure had to leave behind. That's [TypeScript](/typescript).
- `parenthesized` marks anything you wrapped in parentheses with `parenthesized: true`.
- `sourceType: 'module'` reads strict code, with `import`, `export` and top-level `await`.

Every option, with its type and default, is in the [reference](/reference/parser#options).

## Fine print on positions

Offsets are UTF-16, the way JavaScript strings count. They're offsets into the whole text however small a piece you parsed, so nothing has to be added back. `end` includes closing parentheses around the node and any comment trailing it.
