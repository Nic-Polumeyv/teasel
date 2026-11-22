---
'@teasel/parser': minor
---

`parse(text, options)` reads a Svelte component and returns its `Root`, typed by the exported `AST` namespace; `parseModule` reads JavaScript or TypeScript as a module and `parseCss` reads a stylesheet. `parseAt`, its pieces and `until`, `@teasel/parser/grammar`, and the options `sourceType`, `allowReturnOutsideFunction`, `allowAwaitOutsideFunction`, `allowSuperOutsideMethod` and `allowUndeclaredExports` are removed, and the type `Root` of an answer's `roots` is now `JsRoot`
