<picture>
  <source media="(prefers-color-scheme: dark)" srcset="banner-dark.svg">
  <img src="banner.svg" width="100%" alt="">
</picture>

<h1 align="center">teasel</h1>
<p align="center">A JavaScript and TypeScript parser in Rust. It answers in ESTree.</p>

<br>

```bash
npm install @teasel/parser
```

```js
import { Source, js } from '@teasel/parser';

const source = new Source(text, { typescript: true, scopes: true });
const { node } = source.parse();            // the whole source, as an ESTree Program
const { node: expression } = source.parse(js.expression, 7);   // the expression at offset 7
```

The tree is plain ESTree. The options add to the answer: [what parse returns](https://teasel.dev/what-parse-returns) lists them. The scopes, bindings and references hang beside the tree, reached from a node: [scopes](https://teasel.dev/scopes). A parse of one piece stops at the host's own tokens, so a template language reads the JavaScript inside its syntax: [embedded JavaScript](https://teasel.dev/embedded-javascript). A template language can also hand over its whole syntax as a grammar and get the whole document back as one tree: [parsing with a grammar](https://teasel.dev/parsing-with-a-grammar). Everything else is at [teasel.dev](https://teasel.dev).

## Rust and the command line

```rust
let (ast, parsed) = teasel::parse_at(source, 0, None, Entry::Program, Options::default(), "");
```

```
teasel --typescript --scopes file.ts
```

## Build

```
cargo build --release
cd npm
npm ci                 # the pinned TypeScript
npm run build          # the Node addon, and the JavaScript and declarations from the TypeScript
npm run build:wasm     # the WebAssembly module; wasm-opt (binaryen) on the PATH; everything lands in dist/
npm test
```

Node, and a bundle built for Node, resolve `@teasel/parser` to the addon, and everything else to the WebAssembly module, with the same API. Where the addon does not load, Node takes the WebAssembly module too.

<br>

<p align="center"><sub>Named after the plant whose dried heads were used to tease apart wool fibres and raise the nap on cloth.</sub></p>
