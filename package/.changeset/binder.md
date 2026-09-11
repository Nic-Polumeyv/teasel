---
'@teasel/parser': patch
---

scope analysis keeps its working storage between parses: a request with scopes makes half the allocations it did, none per declaration
