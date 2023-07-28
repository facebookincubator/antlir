# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:feature.bzl", "feature_record")
load("//antlir/antlir2/bzl/feature:mount.bzl", "host_mount_record", "layer_mount_record", "mount_record")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop(feature_record)

def _mountpoint(mount: mount_record.type) -> str:
    return mount.layer.mountpoint if mount.layer else mount.host.mountpoint

def all_mounts(
        *,
        features: list[feature_record.type],
        parent_layer: ["LayerInfo", None]) -> list[mount_record.type]:
    """
    Find all the mounts that would need to be directly applied to this layer
    based on these features. This expands nested layer mounts so that they can
    be easily handled without having to recursively look up mounts required by
    other mounted-in layers.
    """
    mounts = list(parent_layer.mounts) if parent_layer else []
    for feat in features:
        if feat.feature_type == "mount":
            mount = feat.analysis.data
            mounts.append(mount)

            # Layer mounts may lead to nested mounts
            if mount.layer:
                # However, we only need to propagate up a flat list of mounts,
                # since any necessary recursion will already have been expanded
                # at the previous layer
                for nested in mount.layer.src.mounts:
                    new_mountpoint = paths.join(mount.layer.mountpoint, _mountpoint(nested).lstrip("/"))
                    mounts.append(mount_record(
                        layer = layer_mount_record(
                            mountpoint = new_mountpoint,
                            src = nested.layer.src,
                        ) if nested.layer else None,
                        host = host_mount_record(
                            mountpoint = new_mountpoint,
                            src = nested.host.src,
                            is_directory = nested.host.is_directory,
                        ) if nested.host else None,
                    ))

    return mounts

def nspawn_mount_args(mount: mount_record.type) -> cmd_args.type:
    if mount.layer:
        return cmd_args("--bind-mount-ro", mount.layer.src.subvol_symlink, mount.layer.mountpoint)
    elif mount.host:
        return cmd_args("--bind-mount-ro", mount.host.src, mount.host.mountpoint)
    else:
        fail("neither host nor layer mount, what is it?!")
