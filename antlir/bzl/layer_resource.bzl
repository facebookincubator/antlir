# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":build_defs.bzl", "buck_genrule")
load(":target_helpers.bzl", "wrap_target")

# This needs to be kept in sync with
# `antlir.tests.layer_resource.LAYER_SLASH_ENCODE`
LAYER_SLASH_ENCODE = "%2F"

def layer_resource(target):
    """
    Wrap an `image.layer()` target with this function before including it
    in a `resources` field of your `python_unittest`.

    DO NOT USE this in `python_binary` or anything that can go in production,
    because layer references are only valid on the machine that built them,
    which makes the including artifact (i.e. your binary) uncacheable too.

    Luckily, CI runs tests in in-place @mode/dev, and in this mode,
    `python_unittest`s are link-trees, which avoids caching the below
    `cacheable = False` target as a part of the `python_unittest` output.
    This happy accident is likely only one reason that this does not cause
    constant failures on CI due to the wrong filesystem paths (i.e. cached
    on the build host, not on the test host) existing in the
    `python_unittest` build artifact.

    If we need CI tests to work in @mode/opt, I don't know of a good mitigation.
    """
    exists, wrapped_target = wrap_target(target, "wrapped_layer_resource")

    if not exists:
        buck_genrule(
            name = wrapped_target,
            bash = 'echo -n "$(location {})" > "$OUT"'.format(target),
            cacheable = False,
            type = "wrapped_layer_resource",
            visibility = [],
        )

    return ":" + wrapped_target
