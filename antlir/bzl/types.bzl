# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Shim for type hints in buck. A lot of this can be deleted when we move to buck2
entirely, but for now this is required to keep buck1 code evaluating.
"""

load("@prelude//utils:type_defs.bzl", prelude_types = "type_utils")
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

def _dict(kt, vt):
    return dict[kt, vt]

def _enum(*values):
    # TODO(nga): `enum` can only be called from top-level statement.
    return native.enum(*values)

def _union(*types):
    if len(types) <= 1:
        fail("union must have more than 1 type")

    # TODO(nga): `eval_type` won't be needed
    #   when we get rid of string literals as types
    result = native.eval_type(types[0])
    for t in types[1:]:
        result = result | t

    return result

def _list(ty):
    return list[ty]

def _optional(ty):
    # TODO(nga): `eval_type` won't be needed
    #   when we switch all types from string literals.
    return native.eval_type(ty) | None

# In the next diff, this gets changed to strong typing for individual shapes by
# using `record`
def _shape(shape_type):
    return shape_type

# re-export the prelude types api to avoid annoying imports when both of
# these are needed
_prelude_reexport = struct(
    is_list = prelude_types.is_list,
    is_string = prelude_types.is_string,
    is_bool = prelude_types.is_bool,
    is_int = prelude_types.is_number,
    is_number = prelude_types.is_number,
    is_tuple = prelude_types.is_tuple,
    is_dict = prelude_types.is_dict,
    is_function = prelude_types.is_function,
)

def _is_none(x) -> bool:
    return x == None

def _is_autodeps_magicmock(x) -> bool:
    """
    autodeps "parsing" of buck macros is pathetically broken and does not handle
    tons and tons of completely legitimate starlark.
    For cases where we have buck macros that sadly have to run in autodeps, this
    is a nice escape hatch until autodeps gets its act together.
    """
    return repr(x).startswith("<MagicMock id=")

types = struct(
    # primitive types
    bool = bool,
    function = native.typing.Callable,
    int = int,
    # buck target label
    label = str,
    path = str,
    struct = struct,
    # either a target label or a file path
    source = str,
    # target label pointing to an executable
    exe = str,
    str = str,
    visibility = list[str],
    # more complex types
    enum = _enum,
    # TODO: can antlir features be better typed with records and unions?
    # Now a feature can be either a struct or target label
    # TODO(nga): this list also had
    #   "InlineFeatureInfo", I have not found references to it
    #   "ParseTimeFeature", which cannot be used easily because of import cycle
    antlir_feature = [struct, str, native.typing.Any],

    # TODO: when we're all buck2, this can enforce the presence of providers.
    # For now it's just a human-readable hint that only enforces on a string.
    layer_source = str,
    shape = _shape,
    # type modifiers
    dict = _dict,
    list = _list,
    optional = _optional,
    union = _union,
    # other stuff
    lint_noop = _lint_noop,
    # runtime type checking
    is_none = _is_none,
    is_autodeps_magicmock = _is_autodeps_magicmock,
    **structs.to_dict(_prelude_reexport)
)
