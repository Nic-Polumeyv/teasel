---
"@teasel/parser": patch
---

In a host grammar, the optional groups after an `expression` are tried in the order the form writes them: with `[ as context=pattern ] [ , index?=identifier ]`, `{#each a, b as x}` reads the sequence `a, b`, where the comma ended the expression before and the document was refused.
