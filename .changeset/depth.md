---
"@teasel/parser": patch
---

Input nested deeper than a walk of its tree can follow is a `nesting_depth` error everywhere: a host document's elements and blocks have the same limit as the script's nesting, a chain of `new` counts toward it, and a tree the decoder cannot descend is that error instead of a `RangeError`. Before, 32,000 nested elements with `scopes` ended the process, and under WebAssembly deep type arguments or a long chain with `scopes` lost the engine and every source it held.
