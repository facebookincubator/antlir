---
title: Build something else using an image
---

When dealing with upstream-adjacent software, it can be very useful to build a
`buck2` target using some system-installed tools (likely from `dnf`
repositories).

For example, if you want to build a
[UKI](https://wiki.archlinux.org/title/Unified_kernel_image), you can either
hope and pray that `systemd-ukify` is installed on your build host and that the
version is roughly what you have tested, or you can use an `antlir2` image to
get a well-known version every single time.

## `genrule_in_image`

`antlir2` offers a rule that behaves similarly to `buck2`'s default
[`genrule`](https://buck2.build/docs/prelude/globals/#genrule) named
`genrule_in_image`.

Just like a `buck_genrule`, this accepts a `bash` string attribute that holds a
script that will be run to produce the rule output. The same macro expansions
(like `$(location)`, `$(exe)`, etc) all work.

The main addition to the `buck_genrule` api is that `genrule_in_image` accepts
an attribute that controls the environment in which the script is run
(`exe_layer` or `layer`).

## Cross-compilation

In most cases, you should use `exe_layer=` instead of `layer=` to choose what
image layer your command will run in. This will give you a layer optimized for
your build host and should generally be more performant, since you'll be running
binaries built for your actual host architecture.

In the rarer occasions where you need to run tools built for the target platform
(eg, `aarch64` while your build host is `x86_64`), you can use `layer=`, which
will cause the `genrule_in_image` to emulate your target architecture (including
installing rpms built for the target architecture).

## Examples

### Produce a single file / directory

Produce a single output file (or directory) by running a command inside of an
image, using inputs from other rules.

```python title="my/team/BUCK"
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/genrule_in_image:genrule_in_image.bzl", "genrule_in_image")

image.layer(
    name = "layer",
    features = [
        feature.rpms_install(rpms = ["systemd-ukify"]),
        "//antlir/antlir2/genrule_in_image:prep",
    ]
)

export_file(
    name = "vmlinuz",
)
export_file(
    name = "initrd.cpio",
)

genrule_in_image(
    name = "uki",
    out = "uki",
    exe_layer = ":layer",
    bash = """
        systemd-ukify build \
            --linux $(location :vmlinuz) \
            --initrd $(location :initrd.cpio) \
            --output $OUT
    """,
)
```

### Produce a set of named outputs

Produce a set of named output files / directories by running a script inside of
an image.

```python title="my/team/BUCK"
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/genrule_in_image:genrule_in_image.bzl", "genrule_in_image")

image.layer(
    name = "layer",
    features = [
        feature.rpms_install(rpms = ["systemd-ukify"]),
        "//antlir/antlir2/genrule_in_image:prep",
    ]
)

export_file(
    name = "vmlinuz",
)
export_file(
    name = "initrd.cpio",
)

genrule_in_image(
    name = "out",
    outs = {
        "uki": "uki",
        "key": "key",
    },
    exe_layer = ":layer",
    bash = """
        systemd-ukify genkey --output $OUT/key

        systemd-ukify sign \
            --linux $(location :vmlinuz) \
            --initrd $(location :initrd.cpio) \
            --output $OUT/uki
    """,
)

# The individual outputs are available as buck subtargets
export_file(
    name = "uki",
    src = ":out[uki]",
)
```
