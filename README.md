# teasel

A JavaScript parser in Rust, built to sit under the Svelte compiler.

Named after the plant whose dried heads were used to tease apart wool fibres and raise the nap on cloth.

## Conformance

The oracle is acorn. `oracle/run.js` parses every script in a Svelte checkout with both and diffs the ESTree output, positions included.

```
cargo build --release
cd oracle && bun install && SVELTE_DIR=~/Projects/svelte bun run.js
```

Early days. Nothing to use yet.
