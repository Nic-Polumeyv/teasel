---
title: How it works
---

A parse crosses from JavaScript into Rust once, and comes back once. Everything in between is arranged to make those two crossings cheap.

![JavaScript on the left, Rust on the right; the text crosses as bytes when the Source is made; a parse runs lexer, parser and scope analysis in Rust and writes one word stream; the decoder on the JavaScript side reads the stream into ESTree objects](HowItWorks.svelte)

## In

The JavaScript engine encodes your text to UTF-8, which it does faster than Rust could read a JavaScript string, and the bytes are copied into the engine once, when the `Source` is made. Every parse of that source reads the copy. Nothing about the text crosses again. Offsets in the answer are UTF-16, the way JavaScript counts, converted from byte positions on the Rust side so you never see a byte offset.

## Inside

Lexer, parser and scope analysis run as one pass over the bytes and build one tree. That tree isn't thrown away after the answer. It's emptied and kept for the next parse on the same thread, so a parse allocates nothing for its own bookkeeping, and how long a call takes depends on the input alone, not on what you parsed before it.

## Out

The answer is not JSON. The encoder writes the tree as a stream of 32-bit words: a numbered shape for each kind of node, then its fields as numbers, string ids and offsets. The stream sits in one buffer the engine owns and JavaScript reads in place, without a copy. The decoder walks it and builds the ESTree objects, one object literal per node with every field present, so the runtime gives each node one hidden class and keeps it. Strings come across as a single block, decoded once and sliced.

That's why the tree is plain. Nothing in it points back into the engine. Walk it, mutate it, serialize it; the engine was done with it the moment the decoder returned.

## Host languages

A template language can describe its whole syntax to the engine, and the engine will then read a document in one pass, the host's own nodes and the JavaScript inside them in one tree, scopes across both. That's how the Svelte and Vue integrations work. The API for writing such a description ships with a coming release.
