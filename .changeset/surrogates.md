---
"@teasel/parser": patch
---

A lone surrogate escape in a string or template, `'\ud83d'`, is the code unit it names in `value` and `cooked`, as JavaScript keeps it, where it was U+FFFD; an import or export name holding one is the syntax error `lone_surrogate_in_module_name`.
