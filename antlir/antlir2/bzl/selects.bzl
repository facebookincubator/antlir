# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:selects.bzl", _prelude_selects = "selects")

# Copied from tools/build_defs/selects.bzl but needs to be copied to be public for OSS
def _tie_n_impl_inner(objs, pvals, val):
    return _tie_n_impl(objs[1:], pvals + [val])

def _tie_n_impl(objs, pvals):
    if not objs:
        return tuple(pvals)

    return _prelude_selects.apply(
        objs[0],
        native.partial(_tie_n_impl_inner, objs, pvals),
    )

def _tie_n(*objs):
    return _tie_n_impl(objs, [])

# End copied section

def _join(**selects):
    """
    Join many selects together into one select that resolves to a struct with
    keys being the kwargs passed into this function.
    """
    lst = list(selects.items())

    def _map_to_struct(resolved):
        return struct(**{
            lst_item[0]: res
            for lst_item, res in zip(lst, resolved)
        })

    return _prelude_selects.apply(_tie_n(*[item[1] for item in lst]), _map_to_struct)

selects = struct(
    apply = _prelude_selects.apply,
    join = _join,
)
