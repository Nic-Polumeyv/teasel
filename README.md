# teasel

A JavaScript parser in Rust, built to sit under the Svelte compiler. The npm name `@teasel/parser` is reserved for it.

Named after the plant whose dried heads were used to tease apart wool fibres and raise the nap on cloth.

## Conformance

The oracle is acorn. `oracle/run.js` parses every script in a Svelte checkout with both, as a module and as a script, and diffs the ESTree output, positions included. `oracle/expressions.js` does the same for every template expression, from the offset the Svelte compiler hands to `parseExpressionAt`, and `oracle/svelte.js` for the each-block patterns, snippet parameters and `@const` declarations Svelte parses through wrappers.

```
cargo build --release
cd oracle && bun install
SVELTE_DIR=~/Projects/svelte bun run.js
SVELTE_DIR=~/Projects/svelte bun expressions.js
SVELTE_DIR=~/Projects/svelte bun svelte.js
git clone --depth 1 https://github.com/tc39/test262-parser-tests.git && bun test262.js
bun regexp.js
```

Known divergences:

- A lone surrogate in a string literal becomes U+FFFD.
- Snippet parameters are read from the real source, so `{#snippet s(x = ")")}` parses; Svelte's own paren scanner stops at the `)` inside the string.

Early days. Nothing to use yet.
