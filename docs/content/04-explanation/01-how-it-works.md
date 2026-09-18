---
title: How it works
---

A parse crosses from JavaScript into Rust once, and back once. This page follows the text in, through the parse, and back out.

![JavaScript on the left, Rust on the right; the text crosses as bytes when the Source is made; a parse runs lexer, parser and scope analysis in Rust and leaves one tree; the reader on the JavaScript side builds ESTree objects from that tree in place](HowItWorks.svelte)

## The text goes in

The JavaScript engine encodes your text to UTF-8, which it does faster than Rust could read a JavaScript string, and the bytes are copied into the engine once, when the `Source` is made. Every parse of that source reads the copy. Nothing about the text crosses again. Offsets in the answer are UTF-16, the way JavaScript counts, converted from byte positions on the Rust side so you never see a byte offset.

## The parse

Lexer, parser and scope analysis run as one pass over the bytes and build one tree. After the answer is read, that tree is emptied and kept for the next parse on the same thread, so a parse allocates nothing for its own bookkeeping, and how long a call takes depends on the input alone, not on what you parsed before it.

## The tree comes back

The answer is not JSON, and nothing encodes the tree. The parser's own tree is a few flat buffers, a record per node, and JavaScript reads those buffers in place as typed arrays, without a copy. The engine describes the layout once, which kind has which fields at which offset and how each kind is spelled in ESTree, and the reader generates a builder per kind from it: one object literal per node with every field present, so the runtime gives each node one hidden class and keeps it. Strings come across as a single block, decoded once and sliced.

Nothing in the tree points back into the engine. You can walk it, change it and serialize it, and the engine is not involved.

## Template languages

A template language can describe its whole syntax to the engine, as a [grammar](/host-grammar), and the engine then reads a document in one pass: the host's own nodes and the JavaScript inside them in one tree, scopes across both, one crossing for the whole file. [Parsing with a grammar](/parsing-with-a-grammar) is the guide to it.
