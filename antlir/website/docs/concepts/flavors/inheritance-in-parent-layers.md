---
id: inheritance-in-parent-layers
title: Inheritance in Parent Layers
---

## Inheritance from Parent Layers

Layers with parents should have the same flavor as their parent. This is because the child layer contains the same OS and RPM installer as the parent layer.

We have a simplification in our API that lets you skip specifying the flavor for child layers. The flavor is read instead from `/.meta/flavor` in the parent.

```
image.layer(
    name = "base_layer",
    flavor = "centos8",
)

# This layer also has the flavor `centos8`.
image.layer(
    name = "child_layer",
    parent = ":base_layer",
)
```

This greatly reduces the verbosity of the flavor API as we don't have to specify the flavor everywhere.

## RPMs in inherited flavors

Due to BUCK limitations, we cannot read the flavor information from the BUCK target. We also cannot do file I/O in `.bzl` to read the flavor information. The flavor information is only available in `python`.

This creates an issue when adding installing RPMs on a layer. RPM installation requires that we add a dependency on BUCK targets in the repo snapshot to make sure we only add valid RPMs. But since we don't know what the flavor is, we don't know which dependencies we need.

The behavior we have is that we just add all dependencies for all possible flavors in [`REPO_CFG.stable_flavors`](stable-flavors.md). For rpms that only available for specific flavors like `centos8` you will have to explicitly specify the flavor on `rpms_install`

```
# This layer also has the flavor `centos8`.
image.layer(
    name = "child_layer",
    parent = ":base_layer",
    features = [
        # RPM that is available on all flavors
        feature.rpms_install([
            "gcc",
        ]),
        # RPM that is specific to `centos8`
        feature.rpms_install([
            "dnf",
        ], flavors = ["centos8"]),
    ]
)
```

We also have coverage checks to make sure that you have at least one (possibly empty) `rpms_install` for every flavor in `REPO_CFG.flavor_available`. This is to make explicit that no RPMs are being installed.

```
REPO_CFG.flavor_available = ["centos7", "centos8"]

# This layer will trigger an error due to not covering `centos7` in REPO_CFG.flavor_available
image.layer(
    name = "child_layer",
    parent = ":base_layer",
    features = [
        # RPM that is specific to `centos8`
        feature.rpms_install([
            "dnf",
        ], flavors = ["centos8"]),
    ]
)

# This layer covers all flavors and does not trigger any errors
image.layer(
    name = "child_layer",
    parent = ":base_layer",
    features = [
        # This makes it explicit that no RPMs are installed on `centos7`.
        feature.rpms_install([], flavors = ["centos7"]),
        # RPM that is specific to `centos8`
        feature.rpms_install([
            "dnf",
        ], flavors = ["centos8"]),
    ]
)
```

When attempting to install test rpms of flavor `antlir_test` we have helpers in [`antlir/bzl/test_rpms.bzl`](../../api/bzl/test_rpms.bzl.md) that wrap adding the empty `rpms_install` to simplify the api.
