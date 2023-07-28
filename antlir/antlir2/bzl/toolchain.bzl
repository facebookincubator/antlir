# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

Antlir2ToolchainInfo = provider(fields = [
    "antlir2",
])

def _impl(ctx: "context") -> list["provider"]:
    return [
        DefaultInfo(),
        Antlir2ToolchainInfo(
            antlir2 = ctx.attrs.antlir2,
        ),
    ]

antlir2_toolchain = rule(
    impl = _impl,
    attrs = {
        "antlir2": attrs.exec_dep(),
    },
    is_toolchain_rule = True,
)
