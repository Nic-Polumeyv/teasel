<picture>
  <source media="(prefers-color-scheme: dark)" srcset="banner-dark.svg">
  <img src="banner.svg" width="100%" alt="">
</picture>

<h1 align="center">teasel</h1>
<p align="center">A JavaScript and TypeScript parser in Rust. It answers in ESTree.</p>

<br>

```js
import { Source } from '@teasel/parser';

using source = new Source(text, { typescript: true, scopes: true });
const { node } = source.parse();
```

Every parse is `source.parse(entry, offset, { end, stopAt })`. Every answer is `{ node, end }` and what the options add. A source is disposable: `using` releases what the engine holds for it at the end of the block, and the collector does otherwise.

```
source.parse()                                Program
source.parse('expression', 7)                 the expression that starts at 7
source.parse('pattern', 7)                    an assignment target: a name or a destructuring
source.parse('params', 7)                     the patterns of a (a, b = 1)
source.parse('statement', 7)                  one statement
source.parse('typeParameters', 7)             a <T extends U>
source.parse('program', 12, { end: 40 })      the program inside 12..40, positions of the whole
```

## Inside a larger syntax

A host that embeds JavaScript in its own reads one piece at a time, from an offset, and gets back where its own syntax resumes.

```
{{ items as item, index }}
   ▲    ▲
   3    8
```

```js
const { node, end } = source.parse('expression', 3, { stopAt: ['as', ','] });
// node  Identifier items
// end   8
```

`stopAt` lists the host's own tokens. One read outside every bracket the parse opened, where the expression could end, ends the parse: `,` does not start a sequence, `/>` is not a division. A `then` after `.` is a property name. A TypeScript `as` is the host's unless another `as` follows the assertion, so `xs as T[] as item` ends after the type.

## What the options add

| option | on the tree | on the answer |
| --- | --- | --- |
| `locations` | `loc` with line and column on every node | |
| `comments` | `leadingComments`, `trailingComments`, `innerComments` | `comments`, every comment read |
| `scopes` | | `scopes`, `bindings`, `references`, and the four questions below |
| `parenthesized` | `parenthesized: true` on a node the source wraps in parens | |
| `errorRecovery` | an `Identifier` named `''` of no width where something is missing | `errors`, instead of a throw |
| `typescript` | TypeScript nodes | |
| `decorators: 'legacy'` or `'proposal'` | restricts decorator syntax; unset reads both | |
| `typescript: 'erase'` | JavaScript, the types gone | `typescript`, what could not be erased: enums, namespaces with values, parameter properties, `export =`, `import =`, decorators, accessor fields |
| `sourceType: 'module'` | strict code, `import`, `export`, top-level `await` | |

A key is on the answer exactly when its option is on. `allowReturnOutsideFunction`, `allowAwaitOutsideFunction`, `allowSuperOutsideMethod` and `allowUndeclaredExports` loosen the early errors as their names say.

## Scopes

The tree stays plain ESTree. The facts hang beside it, reached from a node.

```js
import { Source, scopeOf, bindingOf, referenceOf, parentOf } from '@teasel/parser';

const { node } = new Source('let x = 1; function f(y) { x = y; }', { scopes: true }).parse();
const [declaration, fn] = node.body;
const assignment = fn.body.body[0].expression;

bindingOf(declaration.declarations[0].id)   // { name: 'x', kind: 'let', scope, node, declaration }
referenceOf(assignment.left)                // { scope, binding, write: true, read: false, mutate: false, node, writeExpr }
scopeOf(fn)                                 // { kind: 'function', parent, node, topLevelAwait: false }
parentOf(assignment.left)                   // the assignment
```

```
Program ──────────────────── scope: module
├─ let x = 1                  x  binding  let
└─ function f(y) {            scope: function
   │                          y  binding  param
   └─ x = y                   x  reference  write     y  reference  read
```

A reference to a name no scope declares has `binding: null`. A copy of a node carries no facts.

## Errors

```js
try {
	new Source('x = ;').parse();
} catch (e) {
	e.code;    // 'unexpected_token'
	e.pos;     // 4
	e.end;     // 5
	e.loc;     // { line: 1, column: 4 }
	e.message; // 'Unexpected token'
}
```

With `errorRecovery`, the parse comes back and the errors come with it.

```
{ f(a, }
       ▲
       errors  [{ code: 'unexpected_token', pos: 7, end: 7, loc }]
       node    Identifier ''  at 7..7
       end     7
```

## Rust and the command line

```rust
let (ast, roots, end) = teasel::parse_at(source, 0, None, Entry::Program, options, "")?;
```

```
teasel --typescript --scopes file.ts
```

## Build

```
cargo build --release
cd package
bun run build          # the Node addon
bun run build:wasm     # the WebAssembly module
bun test.js
```

Node resolves `@teasel/parser` to the addon and everything else to the WebAssembly module, with the same API.

<br>

<p align="center"><sub>Named after the plant whose dried heads were used to tease apart wool fibres and raise the nap on cloth.</sub></p>
