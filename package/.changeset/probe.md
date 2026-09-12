---
'@teasel/parser': patch
---

the interner keeps each string's hash beside its id, so a lookup touches one line before it reads the text
