---
'@teasel/parser': patch
---

one description per parse: `source.parse(description, at)` with `program`, `expression`, `pattern`, `params`, `statement`, `typeParameters` exported, `until(...tokens)` for the host's tokens and `within(end)` for a cut, and a `Plan` passed to `parse` instead of the `host` option; the entry strings, `stopAt` and `end` are gone
