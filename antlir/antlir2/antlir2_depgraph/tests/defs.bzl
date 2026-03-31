# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/image:depgraph.bzl", "analyze_features")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo", "FeaturePluginPluginKind")
load("//antlir/bzl:build_defs.bzl", "buck_sh_test")

def _bad_impl(ctx: AnalysisContext) -> list[Provider]:
    features = ctx.attrs.features[FeatureInfo]

    analyzed_features = analyze_features(
        ctx = ctx,
        features = features.features,
        identifier = "depgraph_test",
        phase = BuildPhase("compile"),
        plugins = {str(plugin.label.raw_target()): plugin[FeaturePluginInfo] for plugin in ctx.plugins[FeaturePluginPluginKind]},
    )

    cmd = cmd_args(
        ctx.attrs.test_depgraph[RunInfo],
        cmd_args(analyzed_features, format = "--feature={}"),
        cmd_args(ctx.attrs.error_regex, format = "--error-regex={}"),
        cmd_args(ctx.attrs.parent[LayerInfo].facts_db, format = "--parent={}") if ctx.attrs.parent else cmd_args(),
    )
    return [
        DefaultInfo(),
        RunInfo(args = cmd),
    ]

_bad_depgraph_test_runner = rule(
    impl = _bad_impl,
    attrs = {
        "error_regex": attrs.string(),
        "features": attrs.dep(
            providers = [FeatureInfo],
            pulls_plugins = [FeaturePluginPluginKind],
        ),
        "parent": attrs.option(
            attrs.dep(providers = [LayerInfo]),
            default = None,
        ),
        "test_depgraph": attrs.default_only(attrs.dep(default = "//antlir/antlir2/antlir2_depgraph/tests/test_depgraph:test-depgraph")),
        "_analyze_feature": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_depgraph_if:analyze")),
    },
    uses_plugins = [FeaturePluginPluginKind],
)

def bad_depgraph(
        name: str,
        features,
        **kwargs):
    feature.new(
        name = name + "--features",
        features = features,
        visibility = [":" + name],
    )
    _bad_depgraph_test_runner(
        name = name + "--test",
        features = ":" + name + "--features",
        **(default_target_platform_kwargs() | kwargs)
    )
    buck_sh_test(
        name = name,
        test = ":" + name + "--test",
    )

def _good_impl(ctx: AnalysisContext) -> list[Provider]:
    layer_contents = ctx.attrs.layer[LayerInfo].contents
    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(
            # force the layer to be built for the test to be considered a
            # success
            command = [cmd_args("true", hidden = [layer_contents.subvol_symlink])],
            default_executor = CommandExecutorConfig(
                local_enabled = True,
                # Requires local subvolume and cannot be run on RE
                remote_enabled = False,
            ),
            type = "custom",
        ),
    ]

_good_depgraph = rule(
    impl = _good_impl,
    attrs = {
        "layer": attrs.dep(providers = [LayerInfo]),
    },
)

def good_depgraph(name, **kwargs):
    image.layer(name = name, **kwargs)
    _good_depgraph(
        name = name + "-test",
        layer = ":" + name,
        **default_target_platform_kwargs()
    )

def _deterministic_depgraph_impl(ctx: AnalysisContext) -> list[Provider]:
    features_info = ctx.attrs.features[FeatureInfo]
    plugins = {str(plugin.label.raw_target()): plugin[FeaturePluginInfo] for plugin in ctx.plugins[FeaturePluginPluginKind]}

    # Analyze features once - these are the shared inputs to both depgraph runs
    analyzed_features = analyze_features(
        ctx = ctx,
        plugins = plugins,
        features = features_info.features,
        identifier = "depgraph_determinism",
        phase = BuildPhase("compile"),
    )

    analyzed_features_json = ctx.actions.write_json(
        ctx.actions.declare_output("analyzed_features.json", has_content_based_path = False),
        analyzed_features,
        with_inputs = True,
    )

    # Run the depgraph builder twice with the same inputs but different output paths
    outputs = []
    for run in ("run1", "run2"):
        db_output = ctx.actions.declare_output(run, "depgraph", has_content_based_path = False)
        topo_features = ctx.actions.declare_output(run, "topo_features.json", has_content_based_path = False)

        ctx.actions.run(
            cmd_args(
                ctx.attrs.antlir2[RunInfo],
                "depgraph",
                cmd_args(analyzed_features_json, format = "--features={}"),
                cmd_args(db_output.as_output(), format = "--db-out={}"),
                cmd_args(topo_features.as_output(), format = "--topo-features-out={}"),
            ),
            category = "antlir2_depgraph",
            identifier = run,
            env = {
                "RUST_LOG": "antlir2=trace",
            },
        )
        outputs.append((db_output, topo_features))

    db1, topo1 = outputs[0]
    db2, topo2 = outputs[1]

    compare_script = ctx.actions.write(
        "compare.sh",
        """\
#!/bin/bash
set -euo pipefail
if ! cmp -s "$1" "$2"; then
    echo "FAIL: depgraph db outputs differ"
    exit 1
fi
if ! cmp -s "$3" "$4"; then
    echo "FAIL: topo_features outputs differ"
    exit 1
fi
echo "PASS: depgraph outputs are bitwise identical"
""",
        is_executable = True,
    )

    cmd = cmd_args(
        "/bin/bash",
        compare_script,
        db1,
        db2,
        topo1,
        topo2,
    )

    return [
        DefaultInfo(),
        RunInfo(args = cmd),
    ]

_deterministic_depgraph_test_runner = rule(
    impl = _deterministic_depgraph_impl,
    attrs = {
        "antlir2": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2")),
        "features": attrs.dep(
            providers = [FeatureInfo],
            pulls_plugins = [FeaturePluginPluginKind],
        ),
        "_analyze_feature": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_depgraph_if:analyze")),
    },
    uses_plugins = [FeaturePluginPluginKind],
)

def deterministic_depgraph(
        name: str,
        features,
        **kwargs):
    feature.new(
        name = name + "--features",
        features = features,
        visibility = [":" + name],
    )
    _deterministic_depgraph_test_runner(
        name = name + "--test",
        features = ":" + name + "--features",
        **(default_target_platform_kwargs() | kwargs)
    )
    buck_sh_test(
        name = name,
        test = ":" + name + "--test",
    )
