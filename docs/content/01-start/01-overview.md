---
title: Overview
---

*One pass, one tree, one crossing.*

teasel is a JavaScript and TypeScript parser written in Rust and shipped to JavaScript. It reads a whole source once and answers in ESTree, the tree acorn and every tool built on acorn already understand, with the facts a compiler asks next, scopes, bindings and references, hanging beside the nodes.

<svg viewBox="0 0 720 200" width="100%" role="img" aria-label="the source text and the rules of what to read go into the engine, which lexes, parses and analyses scopes in one pass; the decoder turns its answer into an ESTree tree with the scope tables beside it" fill="none" stroke="currentColor" stroke-width="1.5" font-size="14" class="my-8">
  <rect x="8" y="24" width="160" height="56" rx="8" class="fill-sky-400/15 stroke-sky-400/60"/><text x="88" y="48" text-anchor="middle" fill="currentColor" stroke="none">source text</text><text x="88" y="68" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">JavaScript, TypeScript, a template</text>
  <rect x="8" y="120" width="160" height="56" rx="8" class="fill-amber-400/15 stroke-amber-400/60"/><text x="88" y="144" text-anchor="middle" fill="currentColor" stroke="none">what to read</text><text x="88" y="164" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">a program, a piece, a plan; options</text>
  <path d="M168 52 C 200 52, 200 100, 232 100" marker-end="url(#a)"/>
  <path d="M168 148 C 200 148, 200 100, 232 100" marker-end="url(#a)"/>
  <rect x="234" y="60" width="200" height="80" rx="8" class="fill-violet-400/15 stroke-violet-400/60"/><text x="334" y="90" text-anchor="middle" fill="currentColor" stroke="none">engine</text><text x="334" y="112" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">lex · parse · scopes, one pass in Rust</text>
  <path d="M434 100 H 484" marker-end="url(#a)"/>
  <rect x="486" y="72" width="100" height="56" rx="8" class="fill-emerald-400/15 stroke-emerald-400/60"/><text x="536" y="104" text-anchor="middle" fill="currentColor" stroke="none">decoder</text>
  <path d="M586 100 C 610 100, 610 52, 636 52" marker-end="url(#a)"/>
  <path d="M586 100 C 610 100, 610 148, 636 148" marker-end="url(#a)"/>
  <rect x="638" y="24" width="74" height="56" rx="8" class="fill-rose-400/15 stroke-rose-400/60"/><text x="675" y="57" text-anchor="middle" fill="currentColor" stroke="none">tree</text>
  <rect x="638" y="120" width="74" height="56" rx="8" class="fill-rose-400/15 stroke-rose-400/60"/><text x="675" y="148" text-anchor="middle" fill="currentColor" stroke="none">scopes</text><text x="675" y="166" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">beside it</text>
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
