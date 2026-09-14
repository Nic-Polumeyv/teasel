---
'@teasel/parser': patch
---

one plan per parse: `source.parse(plan, at)` where the built-in plans `Plan.program`, `Plan.expression`, `Plan.pattern`, `Plan.params`, `Plan.statement`, `Plan.typeParameters` are refined with `until(...tokens)` and `within(end)`, and `new Plan(text)` reads a whole document; the entry strings, `stopAt`, `end` and the `host` option are gone
