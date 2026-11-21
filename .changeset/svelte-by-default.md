---
'@teasel/parser': minor
---

`parse(text, options)` reads a Svelte component and returns its `Root`, typed by the exported `AST` namespace; JavaScript and CSS are read by passing `js` or `css`, as in `parse(code, options, js)`, and `svelte` names the default. `parseAt`, its pieces and `until`, and `@teasel/parser/grammar` are removed, and the type `Root` of an answer's `roots` is now `JsRoot`
