# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

AntlirToolchainInfo = provider(fields = [
    "artifacts_dir",
    "volume_for_repo",
])

def _antlir_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        AntlirToolchainInfo(
            artifacts_dir = ctx.attrs.artifacts_dir[RunInfo],
            volume_for_repo = ctx.attrs.volume_for_repo[RunInfo],
        ),
    ]

antlir_toolchain = rule(
    impl = _antlir_toolchain_impl,
    is_toolchain_rule = True,
    attrs = {
        "artifacts_dir": attrs.exec_dep(),
        "volume_for_repo": attrs.exec_dep(),
    },
)
