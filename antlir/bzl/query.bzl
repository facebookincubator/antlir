# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This API provides the ability to construct query strings suitable for use
with `buck query` style parameter macros.  This API currently implements a
subset of the available query expressions implemented in Buck.  See the
`Query functions` section of
https://buck.build/function/string_parameter_macros.html for more detail
about the specific expressions.
"""

load(":target_helpers.bzl", "normalize_target")
load(":types.bzl", "types")

_UNBOUNDED = -1

def _deps(expr, depth):
    """
    Build a `deps(...)` type query expression.

    `epxr` is a query expression of any supported type.

    `depth` is how deep the query should go.  This should be a positive number.
    And if you know what you are doing you can set this to `query.UNBOUNDED` to
    query without a depth limit.
    """
    if depth != _UNBOUNDED and depth < 0:
        fail("buck queries cannot have a negative depth: {depth}".format(depth))

    return "deps({expr}{maybe_depth})".format(
        expr = expr,
        maybe_depth = ", {}".format(depth) if depth != _UNBOUNDED else "",
    )

def _attrfilter(label, value, expr):
    """
    Build an `attrfilter(<label>, <value>, <expr>)` type query expression.

    `expr` is a query expression of any supported type.
    """
    return "attrfilter({label}, {value}, {expr})".format(
        label = label,
        value = value,
        expr = expr,
    )

def _attrregexfilter(label, pattern, expr):
    """
    Build an `attrregexfilter(<label>, <pattern>, <expr>)` type query
    expression.

    `pattern` is a regular expression for matching against the provided
    `label`.

    `expr` is a query expression of any supported type.
    """
    return 'attrregexfilter({label}, "{pattern}", {expr})'.format(
        label = label,
        pattern = pattern,
        expr = expr,
    )

def _set(targets):
    """
    Builds a `set("//foo:target1" "//bar:target2")` query expression.
    """

    if not targets:
        return "set()"

    if types.is_string(targets):
        fail("`query.set()` expects a list")

    # This does not currently escape double-quotes since Buck docs say they
    # cannot occur: https://buck.build/concept/build_target.html
    return 'set("' + '" "'.join([
        normalize_target(target)
        for target in targets
    ]) + '")'

def _union(queries):
    """
    Create a union of multiple query expressions.
    """

    return "(" + " union ".join(queries) + ")"

def _diff(queries):
    """
    Builds an expression using the - operator
    """
    return "(" + " - ".join(queries) + ")"

def _intersect(queries):
    """
    Builds an expression using the ^ operator
    """
    return "(" + " ^ ".join(queries) + ")"

def _kind(kind, expr):
    """
    Build a `kind(...)` type query expression.

    `kind` is a regex that matches on the rule type.
    `epxr` is a query expression of any supported type.
    """
    return "kind('{}', {})".format(kind, expr)

def _filter(regex, expr):
    """
    Build an `filter(<regex>, <expr>)` type query expression.

    `regex` is a regex matched on the rule names from `expr`
    `expr` is a query expression of any supported type.
    """
    return "filter('{}', {})".format(regex, expr)

# The API for constructing buck queries
query = struct(
    attrfilter = _attrfilter,
    attrregexfilter = _attrregexfilter,
    deps = _deps,
    diff = _diff,
    filter = _filter,
    intersect = _intersect,
    kind = _kind,
    set = _set,
    union = _union,
    UNBOUNDED = -1,
)

def layer_deps_query(layer):
    """
    Build and return a query to get all of the image_layer and variant deps
    of the supplied layer.
    """

    return query.attrregexfilter(
        expr = query.deps(
            expr = query.set([
                layer,
            ]),
            depth = query.UNBOUNDED,
        ),
        label = "type",
        pattern = "image_layer*",
    )
