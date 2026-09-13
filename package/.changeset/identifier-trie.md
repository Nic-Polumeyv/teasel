---
'@teasel/parser': patch
---

non-ASCII identifier characters are looked up in a three-level bitmap trie instead of a binary search over ranges: 2.4x faster in the lexer, 1.7x in the package's own identifier check, and the tables are smaller
