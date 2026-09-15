---
title: Overview
---

*A source, its parses, and what they answer with.*

teasel reads a source once and answers with an ESTree tree, the shape acorn produces, and with the facts a compiler asks next hanging beside the nodes. The text crosses into the engine when the `Source` is made; every parse of it after that is a call with an answer.

```text
new Source(text, options)          the text crosses, once
source.parse()                     the whole program
source.parse('expression', at)     one piece, at an offset
{ node, end, …tables }             the answer
```

## What a parse can do

- Answer in plain ESTree, every node with `start` and `end`, and `loc` with `locations` on. See [The answer](/the-answer).
- Work out scopes, bindings and references in the same pass, reached from any node with four functions. See [Scopes](/scopes).
- Keep going past a syntax error and answer with the errors beside a walkable tree, with `errorRecovery`. See [Errors](/errors).
- Read TypeScript, or read it and erase it to JavaScript in one pass, with `typescript`. See [TypeScript](/typescript).
- Read one piece of a larger text at an offset, ended by the host's own tokens, for a template language. See [Inside a host](/inside-a-host).
- Attach comments to the nodes with `comments`, and mark parenthesized expressions with `parenthesized`.

The [reference](/reference/parser) lists every option, entry and table with its type and default.

## Where it runs

Under Node, `@teasel/parser` is a native addon. In a browser or through a bundler, the same import is a WebAssembly build with the same API. From Rust, it is a crate. [Getting started](/getting-started) walks through the first parse; [How it works](/how-it-works) follows one across the boundary and back.
