# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("@fbsource//tools/build_defs/buck2:is_buck2.bzl", "is_buck2")
load("//antlir/bzl:types.bzl", "types")

def _flatten_any(lst):
    flat = []
    for item in lst if types.is_list(lst) or types.is_tuple(lst) else [lst]:
        if types.is_list(item) or types.is_tuple(item):
            flat.extend(_flatten_any(item))
        else:
            flat.append(item)

    return flat

def _typed_flattener(item_type) -> types.function:
    types.lint_noop(item_type)

    # @lint-ignore BUCKLINT
    t = native.eval_type([item_type]) if is_buck2() else ""

    def _flatten(lst):
        r = _flatten_any(lst)
        if is_buck2():
            t.check_matches(r)
        return r

    return _flatten

# TODO(nga): "function" is for example "str" which acts as type.
#   Add a type of type to starlark.
_ITEM_T = types.optional(types.union(types.str, types.function, types.list(types.union(types.str, types.function))))

types.lint_noop(_ITEM_T)

def _flatten_with_inline_hint(
        lst,
        item_type: _ITEM_T = None):
    if item_type:
        f = _typed_flattener(item_type)
        return f(lst)
    else:
        return _flatten_any(lst)

flatten = struct(
    flatten = _flatten_with_inline_hint,
    typed = _typed_flattener,
    # The most common use-case of flattening will be antlir features, so expose
    # that as its own thing
    antlir_features = _typed_flattener(types.antlir_feature),
)
