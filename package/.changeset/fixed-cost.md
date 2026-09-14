---
'@teasel/parser': patch
---

less fixed cost per parse: the pooled tree is boxed instead of moved by value through the parser, and the interner clears the slots it used instead of its whole table; an empty parse 432 → 274 ns, and 835 → 285 ns after a large parse on the same thread
