# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "@prelude//linking:shared_libraries.bzl",
    "SharedLibraryInfo",
    "create_shlib_symlink_tree",
    "traverse_shared_library_info",
)
load("@prelude//rust:link_info.bzl", "RustLinkInfo")
load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
# @oss-disable[end= ]: load("//antlir/antlir2/facebook:deps_test.bzl", "blocklist_deps_test")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:build_defs.bzl", "rust_library")
load("//antlir/bzl:oss_shim.bzl", blocklist_deps_test = "ret_none") # @oss-enable

load("//antlir/bzl:target_helpers.bzl", "normalize_target")

FeaturePluginPluginKind = plugins.kind()

FeaturePluginInfo = provider(fields = [
    "plugin",
    "libs",
])

def _impl(ctx: AnalysisContext) -> list[Provider]:
    # copy plugin so that it's RPATH configured below works
    plugin_src = ensure_single_output(ctx.attrs.lib[DefaultInfo].sub_targets["shared"])
    plugin = ctx.actions.declare_output("{}.so".format(ctx.label.name))
    ctx.actions.copy_file(plugin, plugin_src)

    # Copy all the .so's that this plugin links against (at the time of writing,
    # this is exclusively libbtrfsutil.so). This shouldn't really be necessary,
    # but when building in @mode/opt and on RE, the dependencies get dropped and
    # we get left with only the plugin .so and none of its dependencies
    shlib_info = ctx.attrs.lib[SharedLibraryInfo]
    shared_libs = traverse_shared_library_info(shlib_info, transformation_provider = None)
    lib_dir = create_shlib_symlink_tree(
        actions = ctx.actions,
        out = "lib",
        shared_libs = shared_libs,
    )

    return [
        FeaturePluginInfo(plugin = plugin, libs = lib_dir),
        DefaultInfo(plugin, sub_targets = {"libs": [DefaultInfo(lib_dir)]}),
    ]

_feature_plugin = rule(
    impl = _impl,
    attrs = {
        "lib": attrs.dep(providers = [RustLinkInfo]),
    },
)

feature_plugin = rule_with_default_target_platform(_feature_plugin)

def feature_impl(
        *,
        name: str,
        src: str | None = None,
        extra_srcs: list[str] = [],
        deps: list[str] | Select = [],
        resources: dict[str, str | Select] | Select | None = None,
        unstable_features: list[str] = [],
        allow_unused_crate_dependencies: bool = False,
        lib_visibility: list[str] | None = None,
        plugin_visibility: list[str] | None = None,
        visibility: list[str] | None = None,
        rustc_flags: list[str] | Select | None = [],
        features: list[str] | Select | None = [],
        test_srcs: list[str] | Select | None = [],
        test_deps: list[str] | Select | None = []):
    lib_visibility = lib_visibility or visibility or [
        "//antlir/antlir2/...",
        "//tupperware/cm/antlir2/...",
    ]
    rust_library(
        name = name + ".lib",
        srcs = [src or name + ".rs"] + extra_srcs,
        crate = name,
        crate_root = src or name + ".rs",
        rustc_flags = selects.apply(rustc_flags, lambda flags: flags + [
            "-Zcrate-attr=feature({})".format(feat)
            for feat in unstable_features
        ]),
        link_style = "static_pic",
        allow_unused_crate_dependencies = allow_unused_crate_dependencies,
        visibility = lib_visibility,
        deps = selects.apply(
            deps or [],
            lambda deps: deps + [
                "serde",
                "tracing",
                "//antlir/antlir2/antlir2_compile:antlir2_compile",
                "//antlir/antlir2/antlir2_depgraph_if:antlir2_depgraph_if",
                "//antlir/antlir2/antlir2_features:antlir2_features",
            ],
        ),
        features = features,
        test_deps = test_deps,
        test_srcs = test_srcs,
    )
    rust_library(
        name = name + ".linked",
        crate = name,
        crate_root = "src/plugin_entrypoint.rs",
        mapped_srcs = {
            "//antlir/antlir2/features:plugin_entrypoint.rs": "src/plugin_entrypoint.rs",
        },
        named_deps = {
            "impl": ":{}.lib".format(name),
        },
        rustc_flags = [
            # statically link rust's libstd
            "-Cprefer-dynamic=no",
            # Set an RPATH using $ORIGIN so that we can make the feature plugins
            # more or less self-contained.
            # See feature_plugin impl above for more details.
            "-Clink-arg=-Wl,-rpath=$ORIGIN/lib",
        ],
        env = {
            "LABEL": normalize_target(":" + name),
        },
        visibility = [":" + name],
        deps = [
            "serde_json",
            "static_assertions",
            "tracing",
            "tracing-core",
            "//antlir/antlir2/antlir2_compile:antlir2_compile",
            "//antlir/antlir2/antlir2_depgraph_if:antlir2_depgraph_if",
            "//antlir/antlir2/antlir2_features:antlir2_features",
        ],
        resources = resources,
    )

    feature_plugin(
        name = name,
        lib = ":{}.linked".format(name),
        visibility = plugin_visibility or visibility or ["PUBLIC"],
    )

    blocklist_deps_test(
        name = name + "--test-deps",
        target = ":" + name,
    )
