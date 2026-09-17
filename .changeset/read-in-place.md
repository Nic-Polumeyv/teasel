---
"@teasel/parser": patch
---

The answer is built from the parser's own tree, read in place, without a second pass that encoded it first: a parse to ESTree is 15 to 30% faster on sources of tens of kilobytes, and the same on small expressions.
