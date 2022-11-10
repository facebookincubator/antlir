---
id: compiler
title: compiler/
---

## Improvements to the present codebase / tech debt

- `image_layer` should document a manual test of some build errors, or better yet, there should be a Python script that attempts to `buck build` some known-broken TARGETS files. When you do this, please don't break building `antlir/...`!

- In the compiler language, consider requiring all paths to start with "/" to clarify that they are image-absolute. At present, the leading "/" is implicit.

## Ideas for the future

- Support 'image_aspect', which is an aspect of the filesystem that is specified across many features (add this user, enable this `systemd` service, etc), but is added as a single layer on top to inject things like `/etc/passwd` without creating filesystem conflicts. Upside: we get to use `useradd` and `systemctl` etc to manipulate the filesystem.

  This is roughly like `image.genrule_layer` for adding externally managed phased compiler items.

  It's probably a good idea because it would e.g. let us significantly reduce (or eliminate?) the coupling between the core and RPM support.
