---
title: Overview
---

*You make a source once. Then you ask it things.*

teasel reads your JavaScript or TypeScript and hands back the tree acorn would have given you, an ESTree `Program` with `start` and `end` on every node. If you already have code that walks an acorn tree, it walks this one. What you don't get from acorn is everything else a compiler needs next: which `x` is which, what each function closes over, where the errors are when the file is half typed. teasel works that out while it parses and leaves it on the nodes for you to pick up.

```text
new Source(text, options)          the text goes to the engine, once
source.parse()                     the whole program
source.parse('expression', at)     one piece, starting at an offset
{ node, end, …tables }             what comes back
```

## What you can ask for

- A plain ESTree tree with offsets, and `loc` with lines and columns if you say `locations`. See [The answer](/the-answer).
- Scopes, bindings and references, resolved during the parse and reachable from any node. See [Scopes](/scopes).
- A tree even when the file is broken, with the errors listed beside it, if you say `errorRecovery`. See [Errors](/errors).
- TypeScript read as TS-ESTree, or read and stripped to JavaScript on the way out. See [TypeScript](/typescript).
- A single expression out of the middle of a bigger text, stopped at a token you choose. This is how a template language uses it. See [Inside a host](/inside-a-host).
- Comments attached to nodes with `comments`, and `parenthesized: true` on anything you wrapped in parentheses.

The [reference](/reference/parser) has every option, entry and table with its type and default. This section has the story.

## Where it runs

Under Node it's a native addon. In a browser, or through a bundler, the same import gives you a WebAssembly build with the same API. In Rust it's a crate. [Getting started](/getting-started) is the first parse; [How it works](/how-it-works) is what happens between the call and the answer.
