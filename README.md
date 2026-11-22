<picture>
  <source media="(prefers-color-scheme: dark)" srcset="banner-dark.svg">
  <img src="banner.svg" width="100%" alt="">
</picture>

<h1 align="center">teasel</h1>
<p align="center">A Svelte parser in Rust. A component's JavaScript and TypeScript come back as ESTree, in the same tree.</p>

<br>

```bash
npm install @teasel/parser
```

## A component

`parse` reads a Svelte component and returns its `Root`:

```js
import { parse } from '@teasel/parser';

const source = `<script>
	let count = $state(0);
</script>

<button onclick={() => count++}>{count}</button>
`;

const { node } = parse(source);
```

```text
Root 0..93
├─ instance: Script 0..42  context 'default'
│  └─ content: Program 8..33               ESTree from here down
├─ fragment: Fragment
│  ├─ Text 42..44  '\n\n'
│  └─ RegularElement 44..92  name 'button'
│     ├─ attributes: [Attribute 52..75  name 'onclick'
│     │               value: ExpressionTag 60..75 → ArrowFunctionExpression]
│     └─ fragment: Fragment
│        └─ ExpressionTag 76..83 → Identifier 'count'
├─ css: null     (the top-level <style>, its sheet read, when there is one)
├─ js: []  options: null  comments: [the script's comments]
```

The markup's nodes are Svelte's: elements (`RegularElement`, `Component`, `SvelteHead` and the other `svelte:` tags), `Text` and `Comment`, attributes and directives (`Attribute`, `SpreadAttribute`, `BindDirective`, `OnDirective`, …), blocks (`IfBlock`, `EachBlock`, `AwaitBlock`, `KeyBlock`, `SnippetBlock`) and tags (`ExpressionTag`, `HtmlTag`, `ConstTag`, `RenderTag`, …). Every JavaScript expression, pattern and script inside them is ESTree. The package's TypeScript types name every node and field: `AST.Root`, `AST.EachBlock` and so on, exported as the namespace `AST`.

A component is TypeScript when a script tag says `lang="ts"`. Its scripts are read as modules.

## JavaScript and CSS

`parseModule` reads JavaScript or TypeScript as a module, and `parseCss` reads a stylesheet:

```js
import { parseModule, parseCss } from '@teasel/parser';

parseModule('let x: number = 1', { typescript: true }).node; // an ESTree Program
parseCss('a { color: red }').node;                           // a StyleSheet
```

`children` is exported too: for every node type a parse answers with, the fields that hold a node or a list of nodes, which is what a walk over the tree follows. `extras` lists the fields TypeScript may add to a node of any type.

## The answer

Each of the three returns an object, the answer. It always has `node`, the tree. Every other key is there only when its option is on:

- `comments`: `comments` lists every comment in source order, and the same objects go in `leadingComments`, `trailingComments` and `innerComments` on the nodes. A component's `Root` lists its scripts' comments whatever the option.
- `scopes`: `scopes`, `bindings` and `references`, and for a component `roots`. See below.
- `errorRecovery`: `errors`, and a tree instead of a throw. See below.
- `typescript: true` reads TypeScript, in the TS-ESTree shape. `typescript: 'erase'` reads it and removes the types in the same pass, and adds `typescript`, the list of what could not be removed because it does something at runtime: enums, namespaces with values, parameter properties, `export =`, `import =`, decorators and accessor fields.
- `parenthesized` puts `parenthesized: true` on every node written in parentheses.

`parseCss` takes `errorRecovery` alone. An unknown option, or a value an option does not take, throws a `TypeError`.

## Positions

`start` and `end` are UTF-16 offsets into the text you passed, the way JavaScript strings count. Nodes have no `loc`; `answer.locate(offset)` gives the line, from 1, and the column, in UTF-16 units from 0:

```js
const answer = parse(source);
answer.locate(76); // { line: 5, column: 32 }
```

A line ends at `\n`, `\r\n`, a lone `\r`, U+2028 or U+2029. Calls in source order cost least.

## Scopes

With `scopes` on, an identifier can tell you what it refers to. The tree stays plain: the facts are reached through three functions.

```js
import { parse, referenceOf, scopeOf, parentOf } from '@teasel/parser';

const answer = parse(source, { scopes: true });
const [, button] = answer.node.fragment.nodes;
const tag = button.fragment.nodes[0];

referenceOf(tag.expression).binding; // { name: 'count', kind: 'let', scope, node, declaration, … }
scopeOf(answer.node.fragment).kind;  // 'fragment'
parentOf(tag);                       // the Fragment that lists it
```

- `referenceOf(identifier)`: the reference it makes, with the `binding` it resolves to, `null` for a global; for the identifier that declares a name, the binding itself. A reference says whether it `read`s, `write`s or `mutate`s, and `writeExpr` is what a write assigns.
- `scopeOf(node)`: the scope the node opens, when it opens one. A component's `Root` opens the module scope, each script its own, and every fragment a scope of its own, so a block's body sees what the block declares.
- `parentOf(node)`: the node it hangs from; it works without `scopes`.

What the markup declares, an `{#each}` context or a snippet's parameters, is a binding of kind `pattern`. The answer's `roots` lists each piece of JavaScript in the component, the scripts and every expression of the markup, with the scopes, bindings and references made inside it. The facts sit on the nodes under symbol keys: a spread copies them, `structuredClone` and JSON drop them, and they belong to the answer they came with.

## Errors

A syntax error throws a `SyntaxError` with a `code` to branch on, `args` naming what it is about, `pos` and `end` spanning the token, and `line` and `column`. Its `message` is the code and its arguments:

```js
try {
	parse('<div>{#if a}</div>');
} catch (e) {
	e.code;    // 'unexpected_close'
	e.args;    // ['div']
	e.pos;     // 12
	e.message; // "unexpected_close 'div'"
}
```

With `errorRecovery` the parse does not throw: the answer lists the errors and the tree holds what could be read. Something missing is an `Identifier` named `''` of no width where it was expected, so every node keeps the fields of its type. Every code is in the type `Code`; `unexpected_eof` means the text ended while more was expected.

## Rust and the command line

The crate has no dependencies.

```rust
use teasel::{Entry, Options};

let (ast, parsed) = teasel::parse_at(source, 0, Entry::Svelte, Options::default(), "");
let json = teasel::estree::parse(source, Entry::Svelte, 0, Options(Options::SCOPES));
```

```
teasel --svelte --scopes App.svelte
teasel --typescript --module file.ts
teasel --stylesheet style.css
```

The binary prints the answer as JSON. `teasel --batch` reads jobs from stdin, each a header line `LANGUAGE FLAGS LENGTH` (0 JavaScript, 1 CSS, 2 Svelte) followed by the source.

## Build and test

```
cargo build --release
cargo test --release
cd npm
npm ci                 # the pinned TypeScript
npm run build          # the Node addon, and the JavaScript and declarations from the TypeScript
npm run build:wasm     # the WebAssembly module; wasm-opt (binaryen) on the PATH; everything lands in dist/
npm test
```

Node, and a bundle built for Node, resolve `@teasel/parser` to the addon, and everything else to the WebAssembly module, with the same API. Where the addon does not load, Node takes the WebAssembly module too.

The Rust tests pin an answer beside every component under `crates/teasel/tests/hosts/svelte`, and a hash of the answers to Svelte's own test components in `crates/teasel/tests/svelte.hashes` when Svelte is cloned into `crates/teasel/tests/svelte` as CI does it. `UPDATE=1` rewrites the pins once a change is meant.

<br>

<p align="center"><sub>Named after the plant whose dried heads were used to tease apart wool fibres and raise the nap on cloth.</sub></p>
