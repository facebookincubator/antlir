# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

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

    # At a glance, this does not appear to do anything that interesting, but
    # buck2 will validate that the return type matches the hint here so this
    # gives us strong typing
    def _flatten(lst) -> [item_type]:
        return _flatten_any(lst)

    return _flatten

_ITEM_T = types.optional(types.union(types.str, types.list(types.str)))

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
