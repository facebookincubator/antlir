---
id: stable-flavors
title: Stable Flavors
---

## Stable Flavors

`REPO_CFG.stable_flavors` defines which flavors of rpms are included by default into layers that determine their flavor from the parent

```
REPO_CFG = {
    "stable_flavors": [
        "centos7",
        "centos8",
    ]
}

image.layer(
    name = "test-layer",
    parent = "parent-layer",
    features = [
        feature.rpms_install([
            # Includes `centos7/gtk`, `centos8/gtk`
            # but not `centos7-untested/gtk` or `centos8-untested/gtk`
            "gtk",
        ])
    ]
)
```

Flavors not included in `REPO_CFG.stable_flavors` are considered unstable, and can have incorrect rpm snapshots that do not include rpms.

We cannot include rpms from unstable flavors by default into layers with inherited flavors. This is because when an rpm snapshot fails for unstable flavor it will have no available rpms. Then all layers using inherited flavors will have a missing dependency.

Instead, if a user wants to install an rpm onto a layer with an unstable flavor, they have to explicitly mark the flavor with

```
image.layer(
    name = "test-layer",
    features = [
        # Correct installation of flavors with error annotation.
        feature.rpms_install([
            "gtk",
        ], flavors = [
            "centos8-untested",
        ]),
    ]
    flavor = "centos8-untested",
)
```

This ensures when an rpm snapshot fails, we only break layers of that specific flavor.

## Restrictions on `rpms_install`

We also have a restriction that when using an unstable flavor, the rpm action must include the flavor.

This is to make it clear to the user that unflavored rpm actions only affect layers with a stable flavor. They do not affect layers with an unstable flavor.

```
image.layer(
    name = "test-layer",
    features = [
        # Throws an error because it does not contain
        # the rpm for the untested flavor `centos8-untested`
        feature.rpms_install([
            "gtk",
        ]),
    ]
    flavor = "centos8-untested",
)
```
