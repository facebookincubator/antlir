"""
This API provides the ability to construct query strings suitable for use
with `buck query` style parameter macros.  This API currently implements a
subset of the available query expressions implemented in Buck.  See the
`Query functions` section of
https://buck.build/function/string_parameter_macros.html for more detail
about the specific expressions.
"""

load(":target_helpers.bzl", "normalize_target")

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

    `epxr` is a query expression of any supported type.
    """
    return "attrfilter({label}, {value}, {expr})".format(
        label = label,
        value = value,
        expr = expr,
    )

def _set(targets):
    """
    Builds a `set("//foo:target1" "//bar:target2")` query expression.
    """

    if not targets:
        return "set()"

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

# The API for constructing buck queries
query = struct(
    attrfilter = _attrfilter,
    deps = _deps,
    set = _set,
    union = _union,
    UNBOUNDED = -1,
)

def layer_deps_query(layer):
    """
    Build and return a query to get all of the image_layer deps of the supplied
    layer.
    """

    return query.attrfilter(
        expr = query.deps(
            expr = query.set([
                layer,
            ]),
            depth = query.UNBOUNDED,
        ),
        label = "type",
        value = "image_layer",
    )
