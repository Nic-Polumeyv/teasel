---
title: Overview
---

*One pass, one tree, one crossing.*

teasel is a JavaScript and TypeScript parser written in Rust for JavaScript to call. It answers in ESTree, the tree acorn produces and every tool built on acorn already reads, with scopes, bindings and references worked out in the same pass and hanging beside the nodes.

## The boundary

A parser written in Rust is fast. What costs is getting its tree into JavaScript, and today that is done one of two ways.

![on the left, a tree in Rust is written out as JSON text, which crosses to JavaScript, where JSON.parse builds the tree again; on the right, the tree stays in Rust and JavaScript code sends a question across the boundary and gets an answer back for every node it looks at](Boundary.svelte)

Serialize it, as [swc](https://github.com/swc-project/swc/blob/366817647cf0da46cc296a77b2feca6a1882d733/packages/core/src/index.ts#L98) and [oxc](https://github.com/oxc-project/oxc/blob/827fdbdf52558db556f2a00da44e02864f416c0a/napi/parser/src-js/wrap.js#L35) do by default: Rust writes the tree out as JSON text, JavaScript calls `JSON.parse`, and the tree is built twice, with a text between the two that grows with the file. Or keep it in Rust, as [tree-sitter's bindings](https://github.com/tree-sitter/node-tree-sitter/blob/d9c53278dd6d95db507772cef7e62b46888d8744/src/node.cc#L322) do: JavaScript holds handles, and every question about a node, its type, its children, where it starts, is a call across the boundary and back.

teasel does neither.

![the source text crosses into the engine once, read through the piece that says what to read, which fits the engine like a puzzle piece; the engine lexes, parses and analyses scopes in one pass and answers with one stream, which the decoder turns into the tree and the scope tables on the JavaScript side](Overview.svelte)

The text crosses once, when the `Source` is made. What to read, a whole program, an expression at an offset, a piece of a template that ends at the host's own token, is the piece the engine reads it through. The engine lexes, parses and analyses scopes in one pass and answers with a compact stream of words. The decoder builds the ESTree tree from that stream on the JavaScript side, one object per node, and nothing in it points back into the engine: walk it, change it, serialize it. oxc's experimental raw transfer takes the same road, reading its arena from JavaScript.

```bash
npm install @teasel/parser
```

Node loads a native addon; everywhere else the same import loads a WebAssembly build with the same API. [Getting started](/getting-started) walks through the first parse.

## Who it is for

- A compiler or bundler that wants acorn's tree faster, with scope analysis included, and TypeScript read or erased in the same pass.
- A template language that embeds JavaScript, which reads its expressions piece by piece from inside a larger text. See [Inside a host](/inside-a-host).
- A Rust program, through the crate itself.
