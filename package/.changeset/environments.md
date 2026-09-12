---
'@teasel/parser': patch
---

scope analysis declares first and resolves after: a namespace's blocks share what they export and keep the rest to themselves, a parameter default sees past what the body declares without a position check, and a type-only `import X = require()` binds nothing
