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
        deps = {},
    )

host_file_mount = partial(host_mount, is_directory = False)
host_dir_mount = partial(host_mount, is_directory = True)

_source_kind = enum("layer", "host")

def mount_to_json(
        mountpoint: [str.type, None],
        source_kind: _source_kind.type,
        is_directory: [bool.type, None],
        host_source: [str.type, None],
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    if source_kind == "layer":
        source = deps.pop("source")
        if not mountpoint:
            default_mountpoint = source[LayerInfo].default_mountpoint
            if not default_mountpoint:
                fail("mountpoint is required if source does not have a default mountpoint")
            mountpoint = default_mountpoint
        return {
            "layer": {
                "mountpoint": mountpoint,
                "src": source.label.raw_target(),
            },
        }
    elif source_kind == "host":
        return {
            "host": {
                "is_directory": is_directory,
                "mountpoint": mountpoint,
                "src": host_source,
            },
        }
    else:
        fail("invalid source_kind '{}'".format(source_kind))
