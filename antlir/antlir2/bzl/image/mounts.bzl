# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/features:feature_info.bzl", "feature_record")
load("//antlir/bzl:types.bzl", "types")
load(":mount_types.bzl", "host_mount_record", "layer_mount_record", "mount_record")

types.lint_noop(feature_record)

def _mountpoint(mount: mount_record) -> str:
    return mount.layer.mountpoint if mount.layer else mount.host.mountpoint

def all_mounts(*, features: list[feature_record | typing.Any] | None, parent_layer: LayerInfo | Provider | None) -> list[mount_record]:
    """
    Find all the mounts that would need to be directly applied to this layer
    based on these features. This expands nested layer mounts so that they can
    be easily handled without having to recursively look up mounts required by
    other mounted-in layers.
    """
    if not features:
        return []
    mounts = list(parent_layer.mounts) if parent_layer else []
    for feat in features:
        if feat.feature_type == "mount":
            mount = feat.analysis.data

            # Layer mounts may lead to nested mounts
            if hasattr(mount, "layer"):
                layer_mount = feat.analysis.buck_only_data
                mounts.append(
                    mount_record(
                        layer = layer_mount_record(
                            mountpoint = mount.layer.mountpoint,
                            contents = layer_mount.layer[LayerInfo].contents,
                        ),
                        host = None,
                    )
                )

                # However, we only need to propagate up a flat list of mounts,
                # since any necessary recursion will already have been expanded
                # at the previous layer
                for nested in layer_mount.layer[LayerInfo].mounts:
                    new_mountpoint = paths.join(mount.layer.mountpoint, _mountpoint(nested).lstrip("/"))
                    mounts.append(
                        mount_record(
                            layer = layer_mount_record(
                                mountpoint = new_mountpoint,
                                contents = nested.layer.contents,
                            )
                            if nested.layer
                            else None,
                            host = host_mount_record(
                                mountpoint = new_mountpoint,
                                src = nested.host.src,
                                is_directory = nested.host.is_directory,
                            )
                            if nested.host
                            else None,
                        )
                    )
            elif hasattr(mount, "host"):
                mounts.append(
                    mount_record(
                        host = host_mount_record(
                            mountpoint = mount.host.mountpoint,
                            src = mount.host.src,
                            is_directory = mount.host.is_directory,
                        ),
                        layer = None,
                    )
                )
            else:
                fail("no other mount types exist")

    return mounts

def container_mount_args(mount: mount_record) -> cmd_args:
    if mount.layer:
        if not mount.layer.contents.subvol_symlink:
            fail("the layer mounted at {} has no local subvolume (force-remote cad-stack layer)".format(mount.layer.mountpoint))
        return cmd_args("--bind-mount-ro", mount.layer.contents.subvol_symlink, mount.layer.mountpoint)
    elif mount.host:
        return cmd_args("--bind-mount-ro", mount.host.src, mount.host.mountpoint)
    else:
        fail("neither host nor layer mount, what is it?!")

def genrule_mount_args(mount: mount_record) -> cmd_args:
    """
    Mount args for genrule_in_image, which materializes a cad-stack layer
    itself, so the mounted layer needs no local subvolume and the genrule can
    run remotely
    """
    # TODO: re-unify with container_mount_args once container_subtarget
    # supports a cad-stack mount src
    if mount.layer and mount.layer.contents.cad_stack:
        return cmd_args([cmd_args("--mount-cad-stack", mount.layer.mountpoint, layer) for layer in mount.layer.contents.cad_stack])
    return container_mount_args(mount)
