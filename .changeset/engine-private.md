---
"@teasel/parser": patch
---

One entry: `Source`, `parentOf`, `scopeOf`, `referenceOf` and the types come from one module, which takes the native engine where Node loads addons and the WebAssembly one everywhere else. The `engine` export and the `@teasel/parser/wasm` subpath are gone; `node --no-addons` is the way to the WebAssembly engine on Node.
