---
sidebar_position: 2
---

# Migration Guide

All antlir1 macros are now transparently using antlir2. antlir1 is no more.
There's nothing you need to do to migrate.

## New API Codemod

When you want to use the nice new api, we have a codemod!

```
❯ buck2 run fbcode//scripts/vmagro/codemod:antlir1to2 -- --no-format $TARGETS_AND_BZL_FILES
❯ arc lint
```

This will get you most of the way, but may require some manual fixups for
complicated (aka, heavily macro-ized) image definitions.

:::note

This codemod is running against fbsource, so you'll get these changes
automatically, but feel free to run the codemod if you're impatient!

:::
