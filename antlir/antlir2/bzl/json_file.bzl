# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _impl(ctx: AnalysisContext) -> list[Provider]:
    f = ctx.actions.write_json("out.json", ctx.attrs.obj, has_content_based_path = False)
    return [DefaultInfo(f)]

json_file = rule(
    impl = _impl,
    attrs = {
        "obj": attrs.any(),
    },
)
