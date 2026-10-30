---
'@teasel/parser': patch
---

Nodes no longer carry `loc`, and the `locations` option is gone: `answer.locate(offset)` gives an offset's line and column
