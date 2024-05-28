# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:feature.bzl", "verify_feature_records")
load("//antlir/antlir2/features:feature_info.bzl", "feature_record")
load("//antlir/bzl:types.bzl", "types")
load(":mount_types.bzl", "host_mount_record", "layer_mount_record", "mount_record")

types.lint_noop(feature_record)

def _mountpoint(mount: mount_record) -> str:
    return mount.layer.mountpoint if mount.layer else mount.host.mountpoint

def all_mounts(
        *,
        features: list[feature_record | typing.Any],
        parent_layer: LayerInfo | Provider | None) -> list[mount_record]:
    """
    Find all the mounts that would need to be directly applied to this layer
    based on these features. This expands nested layer mounts so that they can
    be easily handled without having to recursively look up mounts required by
    other mounted-in layers.
    """
    verify_feature_records(features)
    mounts = list(parent_layer.mounts) if parent_layer else []
    for feat in features:
        if feat.feature_type == "mount":
            mount = feat.analysis.data

            # Layer mounts may lead to nested mounts
            if hasattr(mount, "layer"):
                layer_mount = feat.analysis.buck_only_data
                mounts.append(mount_record(
                    layer = layer_mount_record(
                        mountpoint = mount.layer.mountpoint,
                        subvol_symlink = layer_mount.layer[LayerInfo].subvol_symlink,
                    ),
                    host = None,
                ))

                # However, we only need to propagate up a flat list of mounts,
                # since any necessary recursion will already have been expanded
                # at the previous layer
                for nested in layer_mount.layer[LayerInfo].mounts:
                    new_mountpoint = paths.join(mount.layer.mountpoint, _mountpoint(nested).lstrip("/"))
                    mounts.append(mount_record(
                        layer = layer_mount_record(
                            mountpoint = new_mountpoint,
                            subvol_symlink = nested.layer.subvol_symlink,
                        ) if nested.layer else None,
                        host = host_mount_record(
                            mountpoint = new_mountpoint,
                            src = nested.host.src,
                            is_directory = nested.host.is_directory,
                        ) if nested.host else None,
                    ))
            elif hasattr(mount, "host"):
                mounts.append(mount_record(
                    host = host_mount_record(
                        mountpoint = mount.host.mountpoint,
                        src = mount.host.src,
                        is_directory = mount.host.is_directory,
                    ),
                    layer = None,
                ))
            else:
                fail("no other mount types exist")

    return mounts

def container_mount_args(mount: mount_record) -> cmd_args:
    if mount.layer:
        return cmd_args("--bind-mount-ro", mount.layer.subvol_symlink, mount.layer.mountpoint)
    elif mount.host:
        return cmd_args("--bind-mount-ro", mount.host.src, mount.host.mountpoint)
    else:
        fail("neither host nor layer mount, what is it?!")
