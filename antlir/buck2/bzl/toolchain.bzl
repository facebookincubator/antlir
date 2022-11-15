# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This needs to use native. to define a UDR.
# @lint-ignore-every BUCKLINT

load("//antlir/bzl:build_defs.bzl", "is_buck2")

AntlirToolchainInfo = provider(fields = [
    "artifacts_dir",
    "builder",
    "volume_for_repo",
])

def _antlir_toolchain_impl(ctx):
    return [
        native.DefaultInfo(),
        AntlirToolchainInfo(
            artifacts_dir = ctx.attrs.artifacts_dir[native.RunInfo],
            builder = ctx.attrs.builder[native.RunInfo],
            volume_for_repo = ctx.attrs.volume_for_repo[native.RunInfo],
        ),
    ]

antlir_toolchain = native.rule(
    impl = _antlir_toolchain_impl,
    is_toolchain_rule = True,
    attrs = {
        "artifacts_dir": native.attrs.exec_dep(),
        "builder": native.attrs.exec_dep(),
        "volume_for_repo": native.attrs.exec_dep(),
    },
) if is_buck2() else None
