# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Shim for type hints in buck. A lot of this can be deleted when we move to buck2
entirely, but for now this is required to keep buck1 code evaluating.
"""

# We use recursion and `native.` so ignore those lints
# @lint-ignore-every BUCKLINT

load("@bazel_skylib//lib:types.bzl", skylib_types = "types")
load(":build_defs.bzl", "is_buck2")
load(":structs.bzl", "structs")

def _lint_noop(*_args):
    """
    This function intentionally does nothing, but allows the user to provide
    arbitrary arguments to trick the buck linter into not removing any loads
    that are used only for type hints.

    Most bzl files that use type hints will end up calling this function, to
    prevent `types.bzl` or any shape file loads from being removed.
    """
    pass

_bool = bool if is_buck2() else "bool"
_function = native.typing.Callable if is_buck2() else "function"
_int = int if is_buck2() else "int"
_str = str if is_buck2() else "str"
_struct = struct if is_buck2() else "struct"

def _dict(kt, vt):
    return dict[kt, vt] if is_buck2() else ""

def _enum(*values):
    if is_buck2():
        # TODO(nga): `enum` can only be called from top-level statement.
        return native.enum(*values)

    values = list(values)

    # TODO(T139523690)
    def _buck1_enum(arg):
        if arg not in values:
            fail("'{}' not in '{}'".format(arg, values))
        return arg

    return _buck1_enum

def _union(*types):
    if not is_buck2():
        return ""

    if len(types) <= 1:
        fail("union must have more than 1 type")

    # TODO(nga): `eval_type` won't be needed
    #   when we get rid of string literals as types
    result = native.eval_type(types[0])
    for t in types[1:]:
        result = result | t

    return result

def _list(ty):
    return list[ty] if is_buck2() else ""

def _optional(ty):
    if not is_buck2():
        return ""

    # TODO(nga): `eval_type` won't be needed
    #   when we switch all types from string literals.
    return native.eval_type(ty) | None

def _record_ctor(**kwargs):
    return struct(**kwargs)

def _record(**kwargs):
    if is_buck2():
        # TODO(nga): record can only be called from top-level statement.
        return native.record(**kwargs)
    else:
        return _record_ctor

# This is really for human-readability, since we can't guarantee that the result
# of the `select` will be of the correct inner type until analysis time, but
# code that's using this should have a function with the concrete resolved type
# for later type checking after this frontend interface allows either the
# concrete type or a (possibly incorrect) selector
def _or_selector(ty):
    return ty | native.Select if is_buck2() else ""

# In the next diff, this gets changed to strong typing for individual shapes by
# using `record`
def _shape(_shape_type):
    return native.struct if is_buck2() else "struct"

# re-export the bazel_skylib types api to avoid annoying imports when both of
# these are needed
_skylib_reexport = struct(
    is_list = skylib_types.is_list,
    is_string = skylib_types.is_string,
    is_bool = skylib_types.is_bool,
    is_none = skylib_types.is_none,
    is_int = skylib_types.is_int,
    is_tuple = skylib_types.is_tuple,
    is_dict = skylib_types.is_dict,
    is_function = skylib_types.is_function,
)

types = struct(
    # primitive types
    bool = _bool,
    function = _function,
    int = _int,
    # buck target label
    label = _str,
    path = _str,
    record = _record,
    struct = _struct,
    # either a target label or a file path
    source = _str,
    # target label pointing to an executable
    exe = _str,
    str = _str,
    visibility = _list(_str),
    # more complex types
    enum = _enum,
    # TODO: can antlir features be better typed with records and unions?
    # Now a feature can be either a struct or target label
    antlir_feature = [_struct, _str, "InlineFeatureInfo", "record", "ParseTimeFeature"],
    antlir_rule = _enum("antlir-private", "user-facing", "user-internal"),
    # TODO: when we're all buck2, this can enforce the presence of providers.
    # For now it's just a human-readable hint that only enforces on a string.
    layer_source = _str,
    shape = _shape,
    # type modifiers
    dict = _dict,
    list = _list,
    optional = _optional,
    union = _union,
    or_selector = _or_selector,
    # other stuff
    lint_noop = _lint_noop,
    **structs.to_dict(_skylib_reexport)
)
