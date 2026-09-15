---
title: How it works
---

A parse crosses the boundary between JavaScript and Rust exactly once in each direction, and everything between is built to make those two crossings cheap.

![JavaScript on the left, Rust on the right; the text crosses as bytes when the Source is made; a parse runs lexer, parser and scope analysis in Rust and writes one word stream; the decoder on the JavaScript side reads the stream into ESTree objects](HowItWorks.svelte)

## In

The text is encoded to UTF-8 by the engine of the JavaScript runtime, which is faster than having Rust read a JavaScript string, and copied into the engine once, when the `Source` is made. Every parse of that source reads the copy; nothing about the text crosses again. Offsets in the answer are UTF-16, as JavaScript counts them, converted by the engine from its byte positions.

## Inside

The lexer, the parser and scope analysis run in one pass over the bytes and build one tree. The tree is not thrown away after the answer: it is emptied and kept for the next parse on the same thread, so a parse allocates nothing for its own bookkeeping, and the cost of a call depends on the input alone, not on what was parsed before.

## Out

The answer is not JSON. The encoder writes the tree as a stream of 32-bit words: a numbered shape for each kind of node, then its fields as numbers, string ids and offsets. The stream lives in one buffer the engine owns and JavaScript reads directly, without a copy. The decoder walks it and builds the ESTree objects, one object literal per node with every field in place, so the runtime allocates each node in one shape. Strings arrive as one text block, decoded once and sliced.

That is why the tree is plain: nothing in it points back into the engine. Walk it, mutate it, serialize it; the engine is done with it the moment the decoder returns.

## Host languages

A template language can describe its whole syntax to the engine, which then reads a document in one pass, the host's own nodes and the JavaScript inside them in one tree, with scopes across both. That is the design behind the Svelte and Vue integrations, and the authoring API for such descriptions ships with a coming release.
