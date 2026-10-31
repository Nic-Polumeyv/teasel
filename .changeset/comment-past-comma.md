---
'@teasel/parser': patch
---

A comment past a comma leads the code after it on its line: in `f(a, /* @__PURE__ */ g())` it is `g()`'s, not `a`'s
