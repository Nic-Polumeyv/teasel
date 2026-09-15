---
title: Overview
---

*One pass, one tree, one crossing.*

teasel is a JavaScript and TypeScript parser written in Rust and shipped to JavaScript. It reads a whole source once and answers in ESTree, the tree acorn and every tool built on acorn already understand, with the facts a compiler asks next, scopes, bindings and references, hanging beside the nodes.

![the source text and the rules of what to read go into the engine, which lexes, parses and analyses scopes in one pass; the decoder turns its answer into an ESTree tree with the scope tables beside it](Overview.svelte)

The boundary between JavaScript and Rust is crossed once per parse. The engine keeps nothing readable by JavaScript; it answers with a compact stream of words, and the decoder builds the tree from it on the JavaScript side, so the answer is plain objects you can walk, copy and serialize.

```bash
npm install @teasel/parser
```

Node loads a native addon; everywhere else the same import loads a WebAssembly build with the same API. [Getting started](/getting-started) walks through the first parse.

## Who it is for

- A compiler or bundler that wants acorn's tree faster, with scope analysis included, and TypeScript read or erased in the same pass.
- A template language that embeds JavaScript, which reads its expressions piece by piece from inside a larger text. See [Inside a host](/inside-a-host).
- A Rust program, through the crate itself.
