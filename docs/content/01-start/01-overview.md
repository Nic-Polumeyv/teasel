---
title: Overview
---

*One pass, one tree, one crossing.*

teasel is a JavaScript and TypeScript parser written in Rust and shipped to JavaScript. It reads a whole source once and answers in ESTree, the tree acorn and every tool built on acorn already understand, with the facts a compiler asks next, scopes, bindings and references, hanging beside the nodes.

<svg viewBox="0 0 720 120" width="100%" role="img" aria-label="text goes into the engine as bytes, the engine answers with one stream of words, the decoder turns the stream into an ESTree tree" fill="none" stroke="currentColor" stroke-width="1.5" font-size="14" class="my-8">
  <rect x="8" y="34" width="150" height="52" rx="8"/><text x="83" y="65" text-anchor="middle" fill="currentColor" stroke="none">your source text</text>
  <path d="M158 60 H 208" marker-end="url(#a)"/><text x="183" y="50" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">bytes</text>
  <rect x="210" y="24" width="180" height="72" rx="8"/><text x="300" y="56" text-anchor="middle" fill="currentColor" stroke="none">engine, in Rust</text><text x="300" y="76" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">lex, parse, scopes</text>
  <path d="M390 60 H 440" marker-end="url(#a)"/><text x="415" y="50" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">words</text>
  <rect x="442" y="34" width="120" height="52" rx="8"/><text x="502" y="65" text-anchor="middle" fill="currentColor" stroke="none">decoder</text>
  <path d="M562 60 H 612" marker-end="url(#a)"/>
  <rect x="614" y="34" width="98" height="52" rx="8"/><text x="663" y="65" text-anchor="middle" fill="currentColor" stroke="none">ESTree</text>
  <defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor" stroke="none"/></marker></defs>
</svg>

The boundary between JavaScript and Rust is crossed once per parse. The engine keeps nothing readable by JavaScript; it answers with a compact stream of words, and the decoder builds the tree from it on the JavaScript side, so the answer is plain objects you can walk, copy and serialize.

```bash
npm install @teasel/parser
```

Node loads a native addon; everywhere else the same import loads a WebAssembly build with the same API. [Getting started](/getting-started) walks through the first parse.

## Who it is for

- A compiler or bundler that wants acorn's tree faster, with scope analysis included, and TypeScript read or erased in the same pass.
- A template language that embeds JavaScript, which reads its expressions piece by piece from inside a larger text. See [Inside a host](/inside-a-host).
- A Rust program, through the crate itself.
