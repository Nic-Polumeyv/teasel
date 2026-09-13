# @teasel/parser

## 0.0.7

### Patch Changes

- [#86](https://github.com/Nic-Polumeyv/teasel/pull/86) [`c4a03e1`](https://github.com/Nic-Polumeyv/teasel/commit/c4a03e1eccf1fa4b816e69f8182bd0050b723a2c) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the stream's constant strings are numbered at compile time, so writing a node no longer hashes its type and keys

- [#88](https://github.com/Nic-Polumeyv/teasel/pull/88) [`dbb6669`](https://github.com/Nic-Polumeyv/teasel/commit/dbb6669721baa05d2f543ea64ef185d6acb08fa6) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a shape hit compares its record inline instead of through bcmp

## 0.0.6

### Patch Changes

- [#83](https://github.com/Nic-Polumeyv/teasel/pull/83) [`d2350e6`](https://github.com/Nic-Polumeyv/teasel/commit/d2350e694091fad9db88781aead746bfeb08b1df) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - named character references that stand for two code points, like `&nvlt;`, decode to both

- [#82](https://github.com/Nic-Polumeyv/teasel/pull/82) [`809f31b`](https://github.com/Nic-Polumeyv/teasel/commit/809f31bf17f02c40a1b7bb2fbc12e2fcb71558d0) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a word both the host and TypeScript read, `as` before an each block's context, is the host's at its last use, so `xs as T[] || [] as item` parses as it does upstream; a host identifier cannot be a reserved word; an identifier list cannot end in a comma; a grammar whose body declares a field no entry reads fails to load

- [#84](https://github.com/Nic-Polumeyv/teasel/pull/84) [`3830814`](https://github.com/Nic-Polumeyv/teasel/commit/3830814c3c42028c1ef1e2814b6680a3f0a8be61) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - named character references resolve through a hash index built at compile time; a `&word` with no `;` no longer tries every length

- [#80](https://github.com/Nic-Polumeyv/teasel/pull/80) [`c5aff2d`](https://github.com/Nic-Polumeyv/teasel/commit/c5aff2db0395d47e7cb71952b24dea9fc8bafca2) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the interner keeps each string's hash beside its id, so a lookup touches one line before it reads the text

## 0.0.5

### Patch Changes

- [#73](https://github.com/Nic-Polumeyv/teasel/pull/73) [`5686d36`](https://github.com/Nic-Polumeyv/teasel/commit/5686d36b7826e393e1a9c83b83f4fff490188213) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a host's own nodes carry no comments, those stay in the list; a comment between a JavaScript node and the host's next syntax trails the node only over blanks, commas and closing parens, so what follows a parenthesis leads the next node

- [#77](https://github.com/Nic-Polumeyv/teasel/pull/77) [`0f324ad`](https://github.com/Nic-Polumeyv/teasel/commit/0f324adf06d37098f2d4dee4583999b06c82d997) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a host document parses with almost no allocation: the grammar knows what follows each entry before any document is read, a host node's fields are a slice, frames and body groups borrow their buffers, attribute keys are interned, and the scope walk keeps its group stacks

- [#75](https://github.com/Nic-Polumeyv/teasel/pull/75) [`993e232`](https://github.com/Nic-Polumeyv/teasel/commit/993e2326dd63dc7b4ab12b7447de48349929cbc8) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the wasm build answers a panic with an error like the native one, instead of trapping the instance for every later parse

- [#78](https://github.com/Nic-Polumeyv/teasel/pull/78) [`5321ab8`](https://github.com/Nic-Polumeyv/teasel/commit/5321ab82367eab559c21444b825017b137f10c38) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a node is 32 bytes instead of 40 and an optional node id four: node ids keep a niche, number values sit beside the tree, and an export of a declaration is its own kind; the writer owns its constant and shape numbering instead of a thread-local, and parenthesized nodes are a bitset

- [#76](https://github.com/Nic-Polumeyv/teasel/pull/76) [`abba63c`](https://github.com/Nic-Polumeyv/teasel/commit/abba63c2015514f3305eef40c96e2c1d69aeec54) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a parse allocates almost nothing after the first: list buffers, parameter names, the regular expression validator's state and the comment attachments travel with the tree, and numbers are written without a heap string

## 0.0.4

### Patch Changes

- [#71](https://github.com/Nic-Polumeyv/teasel/pull/71) [`eccfba8`](https://github.com/Nic-Polumeyv/teasel/commit/eccfba852bfeb50d7a994a5ebdcbc1307ed00f8d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a function's own `arguments` binding is declared while its piece of the document is read, so a root's tables list it again as they did before 0.0.3

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
