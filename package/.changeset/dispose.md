---
'@teasel/parser': patch
---

a source is disposable: `[Symbol.dispose]()` in place of `free()`, so `using source = new Source(text)` releases what the engine holds at the end of the block
