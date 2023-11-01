---
sidebar_position: 2
---

# Migration Guide

I am a current user of antlir1, but I heard antlir2 is better in every way! How
can I get on board?

## The fastest way

The fastest way to get started is to just add this snippet to a `PACKAGE` file
nearby (in the same directory, or any parent directory) your `TARGETS` file that
has `(tw.)?image.layer` rules.

```python title="PACKAGE"
load("//antlir/bzl:antlir2_migration.bzl", "antlir2_migration")

antlir2_migration.configure_package(mode = "upgrade")
```

This will transparently upgrade all images under that directory
(`$(dirname path/to/PACKAGE)`) to antlir2 without requiring any `TARGETS`/`.bzl`
changes.

This will give you antlir2 images that you can test with, but won't let you use
any new features in antlir2, or simplify your image definitions with the new
macros.

:::tip

You _can_ stop here

If all you want is faster builds, more CI coverage and more reliability, you can
stop here and we'll migrate your image definitions to the new APIs
automatically.

:::

## The best way

When you want to use the nice new api, we have a codemod!

```
❯ buck2 run fbcode//scripts/vmagro/codemod:antlir1to2 -- --no-format $TARGETS_AND_BZL_FILES
❯ arc lint
```

This will get you most of the way, but may require some manual fixups for
complicated (aka, heavily macro-ized) image definitions.
