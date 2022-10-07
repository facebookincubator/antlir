# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:layer_info.bzl", "LayerInfo")
load(":feature_info.bzl", "InlineFeatureInfo")

def layer_mount(
        *,
        source: str.type,
        mountpoint: [str.type, None] = None) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "mount",
        deps = {
            "source": source,
        },
        kwargs = {
            "host_source": None,
            "is_directory": None,
            "mountpoint": mountpoint,
            "source_kind": "layer",
        },
    )

def host_mount(
        *,
        source: str.type,
        is_directory: bool.type,
        mountpoint: [str.type, None] = None) -> InlineFeatureInfo.type:
    mountpoint = mountpoint or source
    return InlineFeatureInfo(
        feature_type = "mount",
        kwargs = {
            "host_source": source,
            "is_directory": is_directory,
            "mountpoint": mountpoint,
            "source_kind": "host",
        },
    )

host_file_mount = partial(host_mount, is_directory = False)
host_dir_mount = partial(host_mount, is_directory = True)

_source_kind = enum("layer", "host")

def mount_to_json(
        mountpoint: [str.type, None],
        source_kind: _source_kind.type,
        is_directory: [bool.type, None],
        host_source: [str.type, None],
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    if source_kind == "layer":
        source = deps.pop("source")
        default_mountpoint = source[LayerInfo].default_mountpoint
        if not default_mountpoint and not mountpoint:
            fail("mountpoint is required if source does not have a default mountpoint")
        return {
            "layer": source.label.raw_target(),
            "mount_config": None,
            "mountpoint": mountpoint or default_mountpoint,
        }
    elif source_kind == "host":
        return {
            "mount_config": {
                "build_source": {
                    "source": host_source,
                    "type": "host",
                },
                "default_mountpoint": mountpoint,
                "is_directory": is_directory,
            },
        }
    else:
        fail("invalid source_kind '{}'".format(source_kind))
