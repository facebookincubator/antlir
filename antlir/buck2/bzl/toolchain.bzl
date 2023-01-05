# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

_tools = [
    "artifacts_dir",
    "builder",
    "compiler",
    "layer_mount_config",
    "subvolume_garbage_collector",
    "subvolume_version",
    "volume_for_repo",
]

AntlirToolchainInfo = provider(fields = _tools)

def _antlir_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        AntlirToolchainInfo(
            **{
                tool: getattr(ctx.attrs, tool)[RunInfo]
                for tool in _tools
            }
        ),
    ]

antlir_toolchain = rule(
    impl = _antlir_toolchain_impl,
    is_toolchain_rule = True,
    attrs = {
        tool: attrs.exec_dep()
        for tool in _tools
    },
)
