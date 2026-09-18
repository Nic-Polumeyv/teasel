---
"@teasel/parser": patch
---

TypeScript that the compiler accepts and the parser refused: `var` inside a namespace, `declare global` or `declare module` body no longer clashes with the name outside it; `const` on a type parameter of a class, a function type or a call, construct or method signature; an optional binding pattern in a signature without a body; a value declared with the name of a type-only import; an export of what a `declare global` block declares; `arguments` as a parameter name in a type inside a namespace; `declare function eval`.
