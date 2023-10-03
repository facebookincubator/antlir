# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//linking:shared_libraries.bzl", "SharedLibraryInfo")
load("@prelude//rust:link_info.bzl", "RustLinkInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:build_defs.bzl", "rust_library")

FeaturePluginInfo = provider(fields = [
    "plugin",
    "libs",
])

def _impl(ctx: AnalysisContext) -> list[Provider]:
    # copy plugin so that it's RPATH configured below works
    plugin_src = ensure_single_output(ctx.attrs.lib[DefaultInfo].sub_targets["shared"])
    plugin = ctx.actions.declare_output("{}.so".format(ctx.label.name))
    ctx.actions.copy_file(plugin, plugin_src)

    lib_dir = ctx.actions.declare_output("lib", dir = True)

    # Copy all the .so's that this plugin links against (at the time of writing,
    # this is exclusively libbtrfsutil.so). This shouldn't really be necessary,
    # but when building in @mode/opt and on RE, the dependencies get dropped and
    # we get left with only the plugin .so and none of its dependencies
    link_info = ctx.attrs.lib[RustLinkInfo]
    lib_dir_map = {}
    for dep in link_info.non_rust_exported_link_deps:
        lib_dir_map.update({
            soname: lib.lib.output
            for soname, lib in dep[SharedLibraryInfo].set.value.libraries.items()
        })

    ctx.actions.copied_dir(lib_dir, lib_dir_map)

    return [
        FeaturePluginInfo(plugin = plugin, libs = lib_dir),
        DefaultInfo(plugin, sub_targets = {"libs": [DefaultInfo(lib_dir)]}),
    ]

feature_plugin = rule(
    impl = _impl,
    attrs = {
        "lib": attrs.dep(providers = [RustLinkInfo]),
    },
)

def feature_impl(
        *,
        name: str,
        src: str | None = None,
        extra_srcs: list[str] = [],
        deps: list[str] = [],
        unstable_features: list[str] = [],
        allow_unused_crate_dependencies: bool = False,
        lib_visibility: list[str] | None = None,
        plugin_visibility: list[str] | None = None,
        visibility: list[str] | None = None,
        **kwargs):
    lib_visibility = lib_visibility or visibility or [
        "//antlir/antlir2/...",
        "//tupperware/cm/antlir2/...",
    ]
    rust_library(
        name = name + ".lib",
        srcs = [src or name + ".rs"] + extra_srcs,
        crate = name,
        crate_root = src or name + ".rs",
        rustc_flags = list(kwargs.pop("rustc_flags", [])) + [
            "-Zcrate-attr=feature({})".format(feat)
            for feat in unstable_features
        ],
        allow_unused_crate_dependencies = allow_unused_crate_dependencies,
        visibility = lib_visibility,
        deps = deps + [
            "anyhow",
            "serde",
            "tracing",
            "//antlir/antlir2/antlir2_compile:antlir2_compile",
            "//antlir/antlir2/antlir2_depgraph:antlir2_depgraph",
            "//antlir/antlir2/antlir2_features:antlir2_features",
            "//antlir/antlir2/features/antlir2_feature_impl:antlir2_feature_impl",
        ],
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
        visibility = [":" + name],
        deps = [
            "anyhow",
            "serde_json",
            "tracing",
            "tracing-core",
            "//antlir/antlir2/antlir2_compile:antlir2_compile",
            "//antlir/antlir2/antlir2_depgraph:antlir2_depgraph",
            "//antlir/antlir2/antlir2_features:antlir2_features",
            "//antlir/antlir2/features/antlir2_feature_impl:antlir2_feature_impl",
        ],
    )

    feature_plugin(
        name = name,
        lib = ":{}.linked".format(name),
        visibility = plugin_visibility or visibility or ["PUBLIC"],
    )
