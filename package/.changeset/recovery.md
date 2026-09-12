---
'@teasel/parser': patch
---

unfinished input under recovery is an answer, never a panic: an attribute cut at its quote, an unterminated comment among attributes, an unclosed string where a directive could be, a handler that fails on its first character; a handler that is not one expression is read as statements without recovery standing in first; on wasm a panic no longer takes every later parse with it, the engine starts over and the sources it held say so
