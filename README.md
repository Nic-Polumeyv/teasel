# teasel

A JavaScript and TypeScript parser in Rust. It produces acorn's ESTree, byte for byte, and fixes what makes acorn awkward under a compiler that embeds JavaScript in a larger syntax: parsing an expression, pattern, parameter list or statement at an offset in a bigger source, attaching comments, and TypeScript without a plugin. Svelte is the first consumer; nothing in it is specific to Svelte. The npm name `@teasel/parser` is reserved for it.

Named after the plant whose dried heads were used to tease apart wool fibres and raise the nap on cloth.

## Conformance

The oracle is acorn, and the corpus is a Svelte checkout, which has thousands of small scripts and template expressions. `oracle/run.js` parses every script with both, as a module and as a script, and diffs the ESTree output, positions included. `oracle/expressions.js` does the same for every template expression, from its offset in the whole file, and `oracle/svelte.js` for the patterns, parameter lists and declarations Svelte reads through wrappers around acorn, compared with what those wrappers produce.

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
- A parameter list is read from the real source, so `(x = ")")` parses; Svelte's own paren scanner stops at the `)` inside the string.
- A pattern's type annotation extends the pattern's end, identifiers included; Svelte leaves an identifier's end before the annotation it builds by hand.

## Comments

The lexer keeps every comment with its range. `comments::attach` hangs them on nodes as `leadingComments` and `trailingComments`, for tools that read directives from comments or print with them. The rule: a comment before a node leads the first node that starts after it; a comment after a node, separated from it by nothing but spaces, commas and closing parens, trails it; the last node of a block, program, array or object takes everything up to the closing bracket, and an empty one keeps what is inside it as `innerComments`; what is left trails the root. Children are visited in source order. `oracle/comments.js` diffs the scripts and template expressions of every component in a Svelte checkout against what Svelte's own attachment produces. Svelte follows the same rule but visits children in the order acorn creates properties, so a comment between `case` and its test, inside a template literal, after a label, before an arrow's return type, before a decorated key or inside a call's type arguments can land elsewhere there; acorn-typescript also reports a comment twice when it backtracks over it, and Svelte keeps both.

```
SVELTE_DIR=~/Projects/svelte bun comments.js
```

Comment values are the text between the delimiters; Svelte strips a block comment's indentation, which the oracle applies before comparing, and keeps one comment list per file so a script inherits the comments of what came before it, which the oracle drops.

## TypeScript

TypeScript is an extension of the JavaScript grammar: the JavaScript parser calls into a fixed set of hook points and knows nothing else about it. `src/typescript/` holds the types, declarations and expressions, its own nodes, and the keys it adds to JavaScript nodes. The oracle is `@sveltejs/acorn-typescript`, the most used acorn TypeScript plugin. `oracle/typescript.js` diffs every `.ts` file in the Svelte and SvelteKit checkouts and every `lang="ts"` script in the Svelte test fixtures, and with `--dts` every `.d.ts` under the Svelte checkout's node_modules; the template oracles above cover TypeScript components too.

```
SVELTE_DIR=~/Projects/svelte KIT_DIR=~/Projects/kit bun typescript.js --dts
```

Where acorn-typescript leaves out keys acorn sets (`attributes`, `optional` on a call with type arguments or a decorator's member expression, `options` of `import()`, `id` of a class expression), teasel follows acorn and the oracle brings the expected side in line before comparing. `TSTypeParameterDeclaration` does not carry the plugin's `extra.trailingComma`.

Where acorn-typescript gets TypeScript wrong, teasel follows TypeScript and the oracle counts the case as an oracle bug:

- `declare const x = 1` is valid; the plugin rejects every ambient initializer because it checks Babel's node names.
- A type, interface, enum, namespace, overload or `declare function` anywhere at the top level satisfies `export { T }`, and exports inside a `declare module` or namespace block name that block; the plugin reports them undefined.
- Modifier order and conflict errors point at the modifier; the plugin reports its column as the offset.
- `export declare const x` is a value export; the plugin marks every `export declare` type-only.
- `f(a: T)` and `[a: T]` are errors, as in TypeScript; the plugin keeps a type cast node there.
- `a?.<T>()` leaves the callee alone; the plugin marks the callee itself optional.
- `class type {}`, `function declare() {}` and labels named after TypeScript's contextual keywords parse; the plugin rejects them.
- `async <T>(x: T) => x` keeps its parameters; the plugin drops them.
- `for (a as b of c)` parses; the plugin reports a type cast in parameter position.
- Modifiers on type parameters report a formatted message; the plugin prints the source of its message function.
- `type T = intrinsic` is the intrinsic keyword; the plugin tests for the word `interface` instead.
- `a?.<b>c` reports a position; the plugin reports `NaN`.

Early days. Nothing to use yet.
