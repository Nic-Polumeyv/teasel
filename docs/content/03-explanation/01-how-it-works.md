---
title: How it works
---

A parse crosses the boundary between JavaScript and Rust exactly once in each direction, and everything between is built to make those two crossings cheap.

<svg viewBox="0 0 720 260" width="100%" role="img" aria-label="JavaScript on the left, Rust on the right; the text crosses as bytes when the Source is made; a parse runs lexer, parser and scope analysis in Rust and writes one word stream; the decoder on the JavaScript side reads the stream into ESTree objects" fill="none" stroke="currentColor" stroke-width="1.5" font-size="14" class="my-8">
  <text x="120" y="24" text-anchor="middle" fill="currentColor" stroke="none" class="text-muted-foreground" font-size="12">JavaScript</text>
  <text x="560" y="24" text-anchor="middle" fill="currentColor" stroke="none" class="text-muted-foreground" font-size="12">Rust</text>
  <path d="M360 8 V 252" stroke-dasharray="4 4" class="text-muted-foreground"/>
  <rect x="20" y="44" width="200" height="44" rx="8" class="fill-sky-400/15 stroke-sky-400/60"/><text x="120" y="71" text-anchor="middle" fill="currentColor" stroke="none">new Source(text)</text>
  <path d="M220 66 H 440" marker-end="url(#c)"/>
  <rect x="442" y="44" width="240" height="44" rx="8" class="fill-violet-400/15 stroke-violet-400/60"/><text x="562" y="71" text-anchor="middle" fill="currentColor" stroke="none">the source, kept</text>
  <rect x="20" y="120" width="200" height="44" rx="8" class="fill-amber-400/15 stroke-amber-400/60"/><text x="120" y="147" text-anchor="middle" fill="currentColor" stroke="none">source.parse(…)</text>
  <path d="M220 142 H 440" marker-end="url(#c)"/>
  <rect x="442" y="108" width="240" height="68" rx="8" class="fill-violet-400/15 stroke-violet-400/60"/><text x="562" y="132" text-anchor="middle" fill="currentColor" stroke="none">lex → parse → scopes</text><text x="562" y="156" text-anchor="middle" fill="currentColor" stroke="none" font-size="11" class="text-muted-foreground">one tree, pooled between parses</text>
  <rect x="442" y="196" width="240" height="44" rx="8" class="fill-violet-400/15 stroke-violet-400/60"/><text x="562" y="223" text-anchor="middle" fill="currentColor" stroke="none">encoder → word stream</text>
  <path d="M440 218 H 222" marker-end="url(#c)"/>
  <rect x="20" y="196" width="200" height="44" rx="8" class="fill-emerald-400/15 stroke-emerald-400/60"/><text x="120" y="223" text-anchor="middle" fill="currentColor" stroke="none">decoder → ESTree</text>
  <defs><marker id="c" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor" stroke="none"/></marker></defs>
</svg>

## In

The text is encoded to UTF-8 by the engine of the JavaScript runtime, which is faster than having Rust read a JavaScript string, and copied into the engine once, when the `Source` is made. Every parse of that source reads the copy; nothing about the text crosses again. Offsets in the answer are UTF-16, as JavaScript counts them, converted by the engine from its byte positions.

## Inside

The lexer, the parser and scope analysis run in one pass over the bytes and build one tree. The tree is not thrown away after the answer: it is emptied and kept for the next parse on the same thread, so a parse allocates nothing for its own bookkeeping, and the cost of a call depends on the input alone, not on what was parsed before.

## Out

The answer is not JSON. The encoder writes the tree as a stream of 32-bit words: a numbered shape for each kind of node, then its fields as numbers, string ids and offsets. The stream lives in one buffer the engine owns and JavaScript reads directly, without a copy. The decoder walks it and builds the ESTree objects, one object literal per node with every field in place, so the runtime allocates each node in one shape. Strings arrive as one text block, decoded once and sliced.

That is why the tree is plain: nothing in it points back into the engine. Walk it, mutate it, serialize it; the engine is done with it the moment the decoder returns.

## Host languages

A template language can describe its whole syntax to the engine, which then reads a document in one pass, the host's own nodes and the JavaScript inside them in one tree, with scopes across both. That is the design behind the Svelte and Vue integrations, and the authoring API for such descriptions ships with a coming release.
