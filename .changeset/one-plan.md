---
"@teasel/parser": patch
---

One rule for every parse: `source.parse(plan, at)`. `Plan.program`, `Plan.expression`, `Plan.pattern`, `Plan.params`, `Plan.statement` and `Plan.typeParameters` are what to read, `until(...tokens)` ends one where the host's own tokens follow, and `new Plan(grammar)` reads the whole source as a document of a host language. `at` is an offset or `[start, end]`. The entry strings, `stopAt`, `end` and the `host` option are gone; `Source` has no type parameter, a plan carries what its parse answers with.
