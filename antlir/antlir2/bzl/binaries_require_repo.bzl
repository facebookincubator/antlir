# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//dist:dist_info.bzl", "DistInfo")

_select_value = select({
    # For unknown build modes, we don't know if the repo is
    # required, so err on the side of caution and include it.
    "DEFAULT": True,
    # @oss-disable[end= ]: "ovr_config//build_mode:dbg": True,
    # @oss-disable[end= ]: "ovr_config//build_mode:dbgo": False,
    # @oss-disable[end= ]: "ovr_config//build_mode:dev": True,
    # @oss-disable[end= ]: "ovr_config//build_mode:opt": False,
})

def _binary_is_standalone(dep: Dependency) -> bool:
    dev = (
        (DistInfo in dep) and (dep[DistInfo].shared_libs or dep[DistInfo].nondebug_runtime_files) or  # Rust/C++
        ("standalone" in dep[DefaultInfo].sub_targets and dep[DefaultInfo].default_outputs != dep[DefaultInfo].sub_targets["standalone"][DefaultInfo].default_outputs)  # Python
    )
    return not dev

binaries_require_repo = struct(
    select_value = _select_value,
    attr = attrs.default_only(attrs.bool(
        default = _select_value,
        doc = "buck2-built binaries require the repo to run (are not relocatable)",
    )),
    optional_attr = attrs.option(attrs.bool(
        doc = "buck2-built binaries require the repo to run (are not relocatable)",
    ), default = None),
    is_standalone = _binary_is_standalone,
)
