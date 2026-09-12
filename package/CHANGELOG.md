# @teasel/parser

## 0.0.3

### Patch Changes

- [#64](https://github.com/Nic-Polumeyv/teasel/pull/64) [`de63819`](https://github.com/Nic-Polumeyv/teasel/commit/de638194cd1c7286ed4a97fe45d576c570661824) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - scope analysis keeps its working storage between parses: a request with scopes makes half the allocations it did, none per declaration

- [#69](https://github.com/Nic-Polumeyv/teasel/pull/69) [`91ff5fc`](https://github.com/Nic-Polumeyv/teasel/commit/91ff5fc2522c677ee01d26581c858d45f157b3bc) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a regular expression with a thousand groups side by side is valid, only nesting counts; a `>` inside a quoted attribute no longer ends the script tag for the TypeScript check; a parameter default's `arguments` is the function's own while the body keeps what it declares; the shortcut follows the options the source was prepared with, an inherited name is not an entry, and a cut inside an astral character goes to the engine

- [#70](https://github.com/Nic-Polumeyv/teasel/pull/70) [`0e886f3`](https://github.com/Nic-Polumeyv/teasel/commit/0e886f39371c63ae162a62fc2f6634743efa00a9) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - scope analysis declares first and resolves after: a namespace's blocks share what they export and keep the rest to themselves, a parameter default sees past what the body declares without a position check, and a type-only `import X = require()` binds nothing

- [#67](https://github.com/Nic-Polumeyv/teasel/pull/67) [`1da73a7`](https://github.com/Nic-Polumeyv/teasel/commit/1da73a763f3c6233a166b4c135e62fda4d957f49) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - scope analysis no longer lets one large scope's room spread to every small scope across parses of differing shapes

- [#68](https://github.com/Nic-Polumeyv/teasel/pull/68) [`b8e00ce`](https://github.com/Nic-Polumeyv/teasel/commit/b8e00ceadab407838ac9b033a461f901487659b4) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - unfinished input under recovery is an answer, never a panic: an attribute cut at its quote, an unterminated comment among attributes, an unclosed string where a directive could be, a handler that fails on its first character; a handler that is not one expression is read as statements without recovery standing in first; on wasm a panic no longer takes every later parse with it, the engine starts over and the sources it held say so

- [#63](https://github.com/Nic-Polumeyv/teasel/pull/63) [`349a761`](https://github.com/Nic-Polumeyv/teasel/commit/349a7615f7f218b3a1f9e6ea5216948fdbd68559) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - what a failed speculative parse built is forgotten, so a long list of TypeScript comparisons no longer trips the tree-size limit; the bare-identifier shortcut answers as the engine does for `x : T`, `eval`, `arguments`, a cut at the start, and lines ended by `\r`, U+2028 or U+2029

## 0.0.2

### Patch Changes

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a source is disposable: `[Symbol.dispose]()` in place of `free()`, so `using source = new Source(text)` releases what the engine holds at the end of the block

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the addon talks to Node-API itself, over a handle, as the WebAssembly module does: a source is a plain external value, and a small expression parses 15 to 22 percent faster

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the addon is built, loaded and released by the package's own scripts

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the answer is written where its reader looks: one allocation per parse instead of fifteen, and nothing copied
