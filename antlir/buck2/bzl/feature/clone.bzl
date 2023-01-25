# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def clone(
        *,
        src_layer: str.type,
        src_path: str.type,
        dst_path: str.type) -> InlineFeatureInfo.type:
    omit_outer_dir = src_path.endswith("/")
    pre_existing_dest = dst_path.endswith("/")
    if omit_outer_dir and not pre_existing_dest:
        fail(
            "Your `src_path` {} ends in /, which means only the contents " +
            "of the directory will be cloned. Therefore, you must also add a " +
            "trailing / to `dst_path` to signal that clone will write " +
            "inside that pre-existing directory dst_path".format(src_path),
        )
    return InlineFeatureInfo(
        feature_type = "clone",
        deps = {
            "src_layer": src_layer,
        },
        kwargs = {
            "dst_path": dst_path,
            "omit_outer_dir": omit_outer_dir,
            "pre_existing_dest": pre_existing_dest,
            "src_path": src_path,
        },
    )

def clone_to_json(
        src_path: str.type,
        dst_path: str.type,
        omit_outer_dir: bool.type,
        pre_existing_dest: bool.type,
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "dst_path": dst_path,
        "omit_outer_dir": omit_outer_dir,
        "pre_existing_dest": pre_existing_dest,
        "src_layer": deps.pop("src_layer").label.raw_target(),
        "src_path": src_path,
    }
