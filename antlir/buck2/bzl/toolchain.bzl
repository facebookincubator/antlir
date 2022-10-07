# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

AntlirToolchainInfo = provider(fields = [
    "artifacts_dir",
    "compiler",
    "layer_mount_config",
    "subvolume_garbage_collector",
    "subvolume_version",
    "volume_for_repo",
])

def _antlir_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        AntlirToolchainInfo(
            artifacts_dir = ctx.attrs.artifacts_dir[RunInfo],
            compiler = ctx.attrs.compiler[RunInfo],
            layer_mount_config = ctx.attrs.layer_mount_config[RunInfo],
            subvolume_garbage_collector = ctx.attrs.subvolume_version[RunInfo],
            subvolume_version = ctx.attrs.subvolume_version[RunInfo],
            volume_for_repo = ctx.attrs.volume_for_repo[RunInfo],
        ),
    ]

antlir_toolchain = rule(
    impl = _antlir_toolchain_impl,
    is_toolchain_rule = True,
    attrs = {
        "artifacts_dir": attrs.exec_dep(),
        "compiler": attrs.exec_dep(),
        "layer_mount_config": attrs.exec_dep(),
        "subvolume_garbage_collector": attrs.exec_dep(),
        "subvolume_version": attrs.exec_dep(),
        "volume_for_repo": attrs.exec_dep(),
    },
)
