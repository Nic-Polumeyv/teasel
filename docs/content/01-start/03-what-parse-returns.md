---
title: What parse returns
---

`parse` returns an object. It always has two keys: the node, and where the parse stopped.

```js parse.js
const { node, end } = parse(text);
```

`node` is the tree. Every node in it has `start` and `end`, offsets into the text you parsed. The `end` beside `node` is the offset where the parse stopped reading. For a whole program that is the end of the text.

These pages call that object the answer.

## Options

Each option adds one key to the answer, or one property to the nodes. A key is present only when its option is on.

![the answer is an object with node and end, and with comments, errors, typescript, scopes, bindings, references and roots tables when their option is on](Answer.svelte)

- `comments` adds `comments`, every comment in source order, and puts `leadingComments`, `trailingComments` and `innerComments` on the nodes.
- `scopes` adds `scopes`, `bindings` and `references`, and lets you ask any node what it declares or refers to. See [Scopes](/scopes).
- `errorRecovery` adds `errors` and returns a tree instead of throwing. See [Errors](/errors).
- `typescript` reads TypeScript. `typescript: 'erase'` also removes it, and adds `typescript`, the list of what could not be removed. See [TypeScript](/typescript).
- `parenthesized` puts `parenthesized: true` on every node that was written in parentheses.
- `sourceType: 'module'` reads strict code, with `import`, `export` and top-level `await`.
- `scopes`, on a file read with a grammar, also adds `roots`, one entry per piece of JavaScript in it. See [Parsing with a grammar](/parsing-with-a-grammar).

Every option, with its type and default, is in the [reference](/reference/parser#options).

## Positions

Offsets are UTF-16, the way JavaScript strings count. They are offsets into the whole text, however small a piece you parsed, so nothing has to be added to them. The answer's `end` is past any closing parentheses around the node and any comment after it.

A node has no line or column. The answer's `locate` method gives the line of an offset, counting from 1, and its column, counting UTF-16 units from 0:

```js position.js
const answer = parse(text);

answer.locate(answer.node.start); // { line: 1, column: 0 }
```

A line ends at `\n`, `\r\n`, a lone `\r`, U+2028 or U+2029. The first call finds where each line starts and keeps the list; each later call tries the line the call before found and the next one before it searches the list, so a printer asking in source order pays least. `locate` reads the answer it is called on, so call it as `answer.locate(offset)`.
