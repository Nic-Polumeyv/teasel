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

## TypeScript

TypeScript is an extension of the JavaScript grammar: the JavaScript parser calls into a fixed set of hook points and knows nothing else about it. `src/typescript/` holds the types, declarations and expressions, its own nodes, and the keys it adds to JavaScript nodes. The oracle is `@sveltejs/acorn-typescript`, the fork Svelte compiles with, quirks included. `oracle/typescript.js` diffs every `.ts` file in the Svelte and SvelteKit checkouts and every `lang="ts"` script in the Svelte test fixtures, and with `--dts` every `.d.ts` under the Svelte checkout's node_modules; the template oracles above cover TypeScript components too.

```
SVELTE_DIR=~/Projects/svelte KIT_DIR=~/Projects/kit bun typescript.js --dts
```

Where acorn-typescript leaves out keys acorn sets (`attributes`, `optional` on a call with type arguments, `options` of `import()`, `id` of a class expression), teasel follows acorn and the oracle brings the expected side in line before comparing. `TSTypeParameterDeclaration` does not carry the plugin's `extra.trailingComma`.

Known divergences from the plugin:

- `class type {}`, `function declare() {}` and labels named after TypeScript's contextual keywords parse; the plugin rejects them.
- `async <T>(x: T) => x` keeps its parameters; the plugin drops them.
- `for (a as b of c)` parses; the plugin reports a type cast in parameter position.
- Modifiers on type parameters report a formatted message; the plugin prints the source of its message function.

Early days. Nothing to use yet.
