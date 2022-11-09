# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "use_rc_target")
# @oss-disable: load("//antlir/fbpkg:fbpkg.bzl", "fbpkg") 
load(":build_defs.bzl", "alias", "buck_genrule", "config", "get_visibility", "rust_binary", "rust_library", "rust_unittest")
load(":target_helpers.bzl", "antlir_dep", "normalize_target")

# @oss-disable: is_facebook = True 
# @oss-enable is_facebook = False

TOOLS = {
    target: target.replace("//", "/").replace(":", "/")
    for target in (
        config.get_antlir_cell_name() + "//antlir/debian:apt-proxy",
        config.get_antlir_cell_name() + "//antlir/bzl/shape2:bzl2ir",
        config.get_antlir_cell_name() + "//antlir/bzl/shape2:ir2code",
    )
}

def antlir_tool(rule, name, **kwargs):
    visibility = get_visibility(kwargs.pop("visibility", ["//antlir/..."]))
    rc_visibility = [antlir_dep("facebook:")]

    target = normalize_target(":" + name)

    if rule == rust_library or rule == rust_binary or rule == rust_unittest:
        kwargs["crate"] = kwargs.get("crate", name).replace("-", "_")

    rule(
        name = name + "-rc",
        visibility = rc_visibility,
        **kwargs
    )

    # If the target being built is in `rc_targets` build it fresh instead of
    # using the cached stable version.
    if (not is_facebook) or use_rc_target(target):
        alias(
            name = name,
            actual = ":{}-rc".format(name),
            visibility = visibility,
        )
        return

    full_label = config.get_antlir_cell_name() + "//" + native.package_name() + ":" + name
    if full_label not in TOOLS:
        fail("'{}' must be added to tool.bzl to be cacheable".format(full_label))

    buck_genrule(
        name = name + "-cached",
        out = "tool",
        cmd = "cp --reflink=auto $(location {})/{} $OUT".format(
            # @oss-disable: fbpkg.fetched_with_nondeterministic_fs_metadata("antlir.tools", repo_committed_tag = "repo_stable"), 
            # @oss-enable "oss antlir does not support cached tools",
            TOOLS[full_label],
        ),
        executable = True,
        type = "antlir_tool",
        visibility = [],
    )

    alias(
        name = name,
        actual = select({
            "DEFAULT": ":{}-rc".format(name),
            # The cached version is currently x86-only.
            "{}//cpu:x86_64".format(config.get_config_cell_name()): ":{}-cached".format(name),
        }),
        visibility = visibility,
    )
