# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This does incorrect modifications.
# @lint-ignore-every FBCODEBZLADDLOADS

load("//antlir/bzl:build_defs.bzl", "is_buck2")
load("//antlir/bzl:types.bzl", "types")

def _flatten_any(lst):
    flat = []
    for item in lst:
        if types.is_list(item):
            flat.extend(_flatten_any(item))
        else:
            flat.append(item)

    return flat

def _typed_flattener(item_type) -> types.function:
    types.lint_noop(item_type)

    # @lint-ignore BUCKLINT
    t = native.eval_type(list[item_type]) if is_buck2() else ""

    def _flatten(lst):
        r = _flatten_any(lst)
        if is_buck2():
            t.check_matches(r)
        return r

    return _flatten

def _flatten_with_inline_hint(
        lst,
        item_type: str | type | list[str | type] | None = None):
    if item_type:
        f = _typed_flattener(item_type)
        return f(lst)
    else:
        return _flatten_any(lst)

flatten = struct(
    flatten = _flatten_with_inline_hint,
)
