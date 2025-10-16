# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def antlir2_error_handler(ctx: ActionErrorCtx) -> list[ActionSubError]:
    errors = []

    for line in ctx.stderr.splitlines():
        if line.startswith("antlir2_error_handler: "):
            err = line.removeprefix("antlir2_error_handler: ")
            err = json.decode(err)
            locs = err.pop("locations", [])

            if len(locs) == 0:
                errors.append(ctx.new_sub_error(
                    category = err["category"],
                    message = err.get("message", None),
                ))
            else:
                for loc in locs:
                    errors.append(ctx.new_sub_error(
                        category = err["category"],
                        message = err.get("message", None),
                        file = loc["file"],
                        lnum = loc.get("line", None),
                    ))

    return errors
