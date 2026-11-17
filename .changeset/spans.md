---
'@teasel/parser': patch
---

Twelve errors that named a word in `args` now span it from `pos` to `end` instead: `redeclaration`, `reserved_word`, `strict_binding`, `unexpected_keyword`, `unexpected_character`, `unsyntactic`, `undeclared_private_name`, `undefined_export`, `duplicate_export`, `duplicate_label`, `type_redeclaration` and `invalid_in_static_block`
