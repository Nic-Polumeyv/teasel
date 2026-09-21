---
"@teasel/parser": patch
---

Under `errorRecovery`, a statement that is skipped no longer leaves its declarations behind: in `let x = (1; let x = 2;` the second statement is kept, where it was reported as a redeclaration and dropped.
