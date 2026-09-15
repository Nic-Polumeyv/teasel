---
"@teasel/parser": patch
---

a binding is the reference its declaring identifier makes: `referenceOf(id)` answers with it, `referenceOf(id).binding` answers for every identifier, and `bindingOf` is gone; a binding says `write` when its declaration binds a value, and a name declared again is a reference with `declares: true`
