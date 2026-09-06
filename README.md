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

- Running out of input is `unexpected_eof`, "Unexpected end of input"; acorn reports an unexpected token at the last offset.
- A lone surrogate in a string literal becomes U+FFFD in `value`; the Node package's `raw` is a slice of the source and keeps it.
- A parameter list is read from the real source, so `(x = ")")` parses; Svelte's own paren scanner stops at the `)` inside the string.
- A pattern's type annotation extends the pattern's end, identifiers included; Svelte leaves an identifier's end before the annotation it builds by hand.

## Bindings

`json::parse` is the one entry every front end uses: a request names what to parse, where, and how, and the answer is ESTree JSON or an error with a code, a message, a span and a location. The CLI, the Node addon in `bindings/node` and the WebAssembly module in `bindings/wasm` all go through it.

`@teasel/parser` is the Node package: `parse`, `parseExpressionAt`, `parsePatternAt`, `parseParamsAt` and `parseStatementAt` take acorn's options with acorn's defaults (`sourceType: 'script'`, no `loc` without `locations`) plus `typescript`, `comments` and `scopes`, return the tree and throw a `SyntaxError` whose `code` names what went wrong (`unexpected_token`, `unexpected_eof`, `unterminated_regexp`, ...), with `pos` and `end` spanning the offending token and a `loc`; the message says it in words and carries no position. The parse-at functions return `{ node, end, comments }`: `end` is the offset after everything the parse consumed, closing parens and trailing comments included, so a host knows where its own syntax resumes, and `comments` lists every comment read when the option is on; `until: 'as'` says the host's own `as` follows the expression, as a template loop's item follows its list: the expression ends at the last top-level `as`, so `list as Type as item` keeps its assertion and ends before the second `as`; `parse` puts that list on the program. `isIdentifierStart` and `isIdentifierChar` are exported for hosts that tokenize around embedded JavaScript, and `new Source(text, options)` keeps a source with its options so the parses out of it share one copy and one set of position tables; `source.parse(start, end)` parses the program inside a range of it, a script in a template, with the positions of the whole, and `source.free()` releases what the engine holds for it before the collector would. `typescript: 'erase'` parses TypeScript and emits JavaScript: annotations, type-only declarations and imports go, assertions give way to their expression, and what erasure cannot express (enums, namespaces with values, parameter properties, `export =`, `import =`) stays in the tree and is listed as `typescript` on the answer, as are the proposals JavaScript itself has, decorators and accessor fields. Offsets are UTF-16, as in acorn; a lone surrogate inside a string literal reaches the parser as U+FFFD. The package is `package`: `bun run build` there builds the Node addon in `bindings/node` and `bun run build:wasm` the WebAssembly module in `bindings/wasm`, and one `api.js` gives both the same API over an engine of six calls. Node resolves the package to the addon and everything else to `wasm.js`, which loads the module with `init` and needs no glue and no dependency: the source is written into the module's memory once and every answer is read straight out of it.

The Node package does not go through JSON. The writer that produces acorn's JSON for the CLI writes, behind the same `Sink`, a shape-coded stream (`estree::Binary`): one buffer of 32-bit words where a node is the id of its shape, the type and the keys it holds with the kind of each value, numbered once per thread as the constants are, followed by its values with no keys and no tags between them; the tree's strings follow as UTF-8 with their UTF-16 ends, source slices are offsets. `decode.js` generates one builder per shape, an object literal V8 allocates in one hidden class, and turns the stream into ESTree objects at a twelfth of what `JSON.parse` costs, or interprets the shapes where a host forbids code generation; the addon takes the source as the UTF-8 bytes V8 encodes, one copy into Rust, and answers with a view of a buffer it keeps, so the hand-over back allocates and copies nothing. On a 66 KB file under Node, best of repeated runs on one pinned core, acorn parses in 1.3 ms with `locations` and 1.1 ms without; teasel end to end takes 0.8 ms and 0.6 ms. `check.js` in `package` parses every script under a directory three ways and diffs the trees: the addon's decoded against its JSON, and the wasm module's against the addon's. The wasm module answers the same stream, its words read in place from the module's memory and its answer buffer reused from one parse to the next.

## Scopes

`scopes: true` adds scope analysis to an answer: it lists `scopes` and `bindings`, and the package answers `scopeOf(node)`, the scope a node opens, and `bindingOf(identifier)`, what it declares or refers to (null for a global, undefined when it names no value), and `referenceOf(identifier)` for a reference's `write`, an assignment or update, and `mutate`, one to a member of its value, which a binding lists under `references` and a global's still has; the tree itself stays plain ESTree, and a copy of a node carries no facts. In JSON the facts sit on the nodes as numbers indexing the tables: `scope`, `binding`, `declares`, `write`, `mutate`; `binding.node`, `scope.declarations` and `scope.through` (the outer bindings a scope's code reaches) are linked by the package. The rules are ECMAScript's: `var` belongs to the nearest function, program or static block; `let`, `const`, classes, function declarations, catch parameters and loop heads to their block; a named function expression and a named class expression bind their own name inside; a parameter default sees past what the body declares; `arguments` is a binding of the function that reads it; references resolve when their scope closes, so use before declaration resolves. Two deliberate departures: a class declaration's body refers to the declaration's binding rather than to a second, immutable one, since that is the reference a compiler wants; and Annex B's `var`-like hoisting of block functions in sloppy scripts is not modelled. Inside `with`, references resolve statically as elsewhere. TypeScript type positions bind nothing, not even a `typeof x` query, and neither do type-only imports and exports, while enums and their members, namespaces (their blocks sharing one scope), `import =` and function signatures declare values. A pattern parsed on its own declares `pattern` bindings, for the host to say what kind they are. `oracle/scopes.js` diffs every resolution and every write against eslint-scope over acorn's tree: `SVELTE_DIR=~/Projects/svelte bun scopes.js`.

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
