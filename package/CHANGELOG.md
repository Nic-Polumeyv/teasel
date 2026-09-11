# @teasel/parser

## 0.0.2

### Patch Changes

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a source is disposable: `[Symbol.dispose]()` in place of `free()`, so `using source = new Source(text)` releases what the engine holds at the end of the block

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the addon talks to Node-API itself, over a handle, as the WebAssembly module does: a source is a plain external value, and a small expression parses 15 to 22 percent faster

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the addon is built, loaded and released by the package's own scripts

- [#61](https://github.com/Nic-Polumeyv/teasel/pull/61) [`f331c0a`](https://github.com/Nic-Polumeyv/teasel/commit/f331c0a0af0f0022cde2a94fe5dec299dd91126d) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - the answer is written where its reader looks: one allocation per parse instead of fifteen, and nothing copied
