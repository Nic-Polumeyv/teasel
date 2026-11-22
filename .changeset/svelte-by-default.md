---
'@teasel/parser': minor
---

`parse` reads a Svelte component, `parseModule` a module and `parseCss` a stylesheet, each taking only the options it uses: the language argument, `parseAt` with its pieces and `until`, `@teasel/parser/grammar` and the options `sourceType` and `allow…` are removed, `parseCss` takes `errorRecovery` alone, an answer has no `end`, and the type `Root` of an answer's `roots` is now `JsRoot`
