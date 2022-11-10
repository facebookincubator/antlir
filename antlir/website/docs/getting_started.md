---
id: getting_started
title: Getting Started
---

If you do not know what Antlir is, start with [Overview](../docs/).

Outside of FB, proceed to [Installation](installing.md). This section assumes that you have a working Antlir in your repo. To check, run `buck test //antlir/rpm:test-yum-dnf-from-snapshot-shadowed`.

Before you read further, review the [Buck key concepts ](https://buck.build/about/overview.html) one-pager, and get familiar with the following parts of Buck's “Concepts” section:

- [Build Rule](https://buck.build/concept/build_rule.html)
- [Build File](https://buck.build/concept/build_file.html) — for forward-compatibility, write these in Starlark (formerly Skylark); either use [Buildifier](https://github.com/bazelbuild/buildtools) to ensure that your code is compatible, or set the Buck config [`parser.default_build_file_syntax = SKYLARK` ](https://buck.build/concept/skylark.html)
- [Build Target](https://buck.build/concept/build_target.html) and [Build Target Pattern](https://buck.build/concept/build_target_pattern.html)

A common Antlir workflow is:

- Define layer, test, and package targets in a build file (`BUCK` in open-source, `TARGETS` at FB).
- `buck run :YOUR-LAYER=container` — build your image and launch a shell inside for manual inspection.
- `buck test :YOUR-TEST` — run a test inside your layer, or to get a debug shell: `buck run :YOUR-TEST--test-lay.
- Finally, `buck build :YOUR-PACKAGE` (either directly or by `buck run`ning a publisher target **TODO OSS link**), and deploy it.

To get started with building images, you may want to study the pages under Concepts & Designs, or take the plunge and try the tutorial on [Defining an Image](tutorials/defining-an-image). In both cases, you will want to refer to the [Image API](api/bzl/image.bzl.md) as you go.
