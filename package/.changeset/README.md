# Changesets

Every user-visible change lands with a file in this folder, named anything, saying the bump and
one line for the changelog:

```md
---
"@teasel/parser": patch
---

what changed, as the changelog should say it
```

Merging to main runs `.github/version.js`, which opens or updates a Version Packages PR;
merging that PR releases.
