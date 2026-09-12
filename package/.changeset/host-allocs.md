---
'@teasel/parser': patch
---

a host document parses with almost no allocation: the grammar knows what follows each entry before any document is read, a host node's fields are a slice, frames and body groups borrow their buffers, attribute keys are interned, and the scope walk keeps its group stacks
