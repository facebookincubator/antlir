---
id: overview
title: Overview
---

## Introduction to Flavors

A flavor is a string identifier that controls build configurations. Options contained in a flavor can include `build_appliance` as well as `rpm_installer`.

This allows us to specify compability between different images. For example, we can make sure that `centos9` images do not depend on `centos8` images, which is a breaking version.

It also allows to reuse common build opts throughout the codebase with less duplication.

Flavors are strings instead of functions because a flavor must have a stable identity as the source tree evolves. We must be able to compare flavors between old revs and new. The container runtime must also be able to rely on stable flavor IDs. Flavors names must follow two critical rules:

- Never change a flavor name
- Never reuse a flavor name

## Using Flavors

To create a flavor add a mapping to `antlir/bzl/build_defs_impl.bzl`

```
shim = struct(
    do_not_use_repo_cfg = {
        "flavor_to_config": {
            "your_flavor_here": {
                "build_appliance": "//path/to/your/build/appliance",
                "rpm_installer": "your_rpm_installer",
            },
        },
    },

)
```

Then, you can pass the flavor to images. You can also override the default value in the flavor with custom ones.

```
image.layer(
    flavor = "your_flavor_here",
    flavor_config_override = image.opts(
        build_appliance = "//your/override/build/appliance",
        rpm_version_set_overrides = [
            image.rpm.nevra(
                name = "fuse",
                epoch = "0",
                version = "2.9.7",
                release = "14.fb5",
                arch = "x86_64",
            ),
        ],
        ...
    )
)
```

The flavor of an image is written as a string to the file `/.meta/flavor` in the image. This allows you to check the compatibility of different layers. We use the flavor file to check the compatibility between cached layers built on older revisions and layers built on the current revision. We also use the flavor file to deduce the flavor of the child layer from the parent.
