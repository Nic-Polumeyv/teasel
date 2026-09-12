---
'@teasel/parser': patch
---

a parse allocates almost nothing after the first: list buffers, parameter names, the regular expression validator's state and the comment attachments travel with the tree, and numbers are written without a heap string
