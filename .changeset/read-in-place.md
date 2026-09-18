---
"@teasel/parser": patch
---

The answer is built from the parser's own tree, read in place, without a second pass that encoded it first: a parse to ESTree is 15 to 30% faster on sources of tens of kilobytes, about 10% on a host's documents, and a little on small expressions. The first parse of a process costs a few milliseconds more than before: the reader builds itself for the kinds of node it meets, once.
