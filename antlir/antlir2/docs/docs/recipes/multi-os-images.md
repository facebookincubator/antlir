# Multi-OS (aka Base) Images

A common requirement of complex base images is the support of multiple OS
versions (think: CentOS 8 and 9) for customers that have varied needs
(especially during migration periods between major OS versions).

### A bit of history

Historically, images were configured by their `parent_layer`, all the way up to
the first root image which decided the OS version of the entire child image
chain.

This required base image owners to maintain separate target graphs for every OS
version that they supported. These were often 99% the same, with the exception
of a few very minor differences (eg: CentOS 8 -> 9 going from `dbus` ->
`dbus-broker`).

These minor differences required very complex `TARGETS`/`.bzl` setups, because
the entire target graph needed to be declared for each supported OS, even if the
individual `image.layer` had the _exact same_ features (because it's
`parent_layer` would need to be different).

## A better way (`default_os`)

Nowadays, `antlir2` allows the leaf image owner to directly decide what OS they
want to use, without requiring the base image author to explicitly create
targets for that OS. This is accomplished with a new attribute `default_os`
supported on `image.layer` and a few other "leaf" rules (such as `package.*` and
`image_*_test`).

Setting `default_os` reconfigures the entire `parent_layer` chain to build for
whatever OS the end user wanted - of course, provided that all images along the
way are compatible (no `compatible_with` that excludes the OS or any features
with a `select` that fails to cover the requested OS).

:::note

`default_os` is applied from the bottom-up

The leaf image being built takes over the configuration of the entire chain. In
other words, the `default_os` attribute of any `parent_layer`s is ignored.

:::

## Base Image Recommendations

### Target Hierarchy

Base image authors should define a single image hierarchy that covers all the
OSes that they support. The OS should never appear in the target name.

Any differences should be covered by `select`s based on the OS being used.

For example, supporting a base image across C8 and C9 might use a select like:

```python
feature.new(
    name = "dbus",
    features = [
        features.rpms_install(subjects = select({
            "//antlir/antlir2/os:centos8": ["dbus"],
            "//antlir/antlir2/os:centos9": ["dbus-broker"],
        }))
    ]
)
```

### Building base image itself

The base image author is free to set `default_os` on their own layers so that
`buck build` will build whatever OS they consider the default.

#### `PACKAGE` files

A chosen `default_os` value can also be applied to all image targets within a
subdirectory of the repo by using `PACKAGE` files.

```python
load("//antlir/antlir2/os:defs.bzl", "set_default_os_for_package")

set_default_os_for_package(
    default_os = "centos9"
)
```

### (In)Compatibility

If a base image is only compatible with some OSes, the author should add a
`compatible_with` to their `image.layer` so that `buck2` provides a better error
message.

For example, if an image is only compatible with CentOS 9, the following might
be used.

```python
image.layer(
    name = "my-base-image",
    compatible_with = ["//antlir/antlir2/os:centos9"],
)
```

:::tip

Prefer not to set `compatible_with`

Unless you know that your image is only compatible with certain OSes, it is
preferred to not specify `compatible_with` in order to ease migration to new OS
versions that are expected to be broadly compatible `feature`-wise.

:::

:::caution

`compatible_with` can give better error messages

If you forget `compatible_with` but do have a `select` (that does not cover any
incompatible OSes), the build will still fail if a child image uses an
incompatible OS, but `compatible_with` will give the end user an
easier-to-understand error.

:::

### CI for packages

See the [internal page](fb/multi-os-images-ci-recommendations.md) for CI
structure recommendations.
