---
id: inheritance-in-parent-layers
title: Inheritance in Parent Layers
---

## Inheritance from Parent Layers

Layers with parents must specify a flavor, and they should have the same flavor as their parent. This is because the child layer contains the same OS and RPM installer as the parent layer.

```
image.layer(
    name = "base_layer",
    flavor = "centos8",
)

# This layer also has the flavor `centos8`.
image.layer(
    name = "child_layer",
    parent = ":base_layer",
    flavor = "centos8",
)
```

## RPMs in inherited flavors

Due to Buck1 limitations, we cannot read the flavor information from the BUCK target. We also cannot do file I/O in `.bzl` to read the flavor information.

This means that when defining rpms_install() features outside of an image (which may be used in multiple images) you must specify all the flavors of images in which that feature may be used.

```
# rpm install feature
dnf_feature = feature.rpms_install(
    ["dnf"],
    flavors = ["centos8"],
)

# This layer also has the flavor `centos8`.
image.layer(
    name = "child_layer",
    parent = ":base_layer",
    flavor = "centos8",
    features = [
        # RPM that is available on all flavors
        feature.rpms_install([
            "gcc",
        ]),
        # RPM that is specific to `centos8`
        dnf_feature,
    ]
)
```
