# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _impl(ctx: AnalysisContext) -> list[Provider]:
    return [DefaultInfo(
        ctx.actions.write_json("xml.json", {
            "filelists": ctx.attrs.filelists,
            "other": ctx.attrs.other,
            "primary": ctx.attrs.primary,
        }),
    )]

xml = rule(
    impl = _impl,
    attrs = {
        "filelists": attrs.string(),
        "other": attrs.string(),
        "primary": attrs.string(),
    },
)
