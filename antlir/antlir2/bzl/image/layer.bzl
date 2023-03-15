# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:collections.bzl", "collections")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "FlavorInfo", "LayerInfo")
load("//antlir/antlir2/bzl/dnf:defs.bzl", "compiler_plan_to_local_repos", "repodata_only_local_repos")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:flatten.bzl", "flatten")
load("//antlir/bzl:types.bzl", "types")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoSetInfo")
load("//antlir/bzl/build_defs.bzl", "config", "get_visibility")
load(":depgraph.bzl", "build_depgraph")

def _map_image(
        ctx: "context",
        cmd: "cmd_args",
        identifier: str.type,
        flavor_info: FlavorInfo.type,
        parent: ["artifact", None]) -> ("cmd_args", "artifact"):
    """
    Take the 'parent' image, and run some command through 'antlir2 map' to
    produce a new image.
    In other words, this is a mapping function of 'image A -> A1'
    """
    out = ctx.actions.declare_output("subvol-" + identifier)
    build_appliance = (ctx.attrs.build_appliance or flavor_info.default_build_appliance)[LayerInfo]
    cmd = cmd_args(
        "sudo",  # this requires privileged btrfs operations
        ctx.attrs.antlir2[RunInfo],
        "map",
        "--working-dir=antlir2-out",
        cmd_args(build_appliance.subvol_symlink, format = "--build-appliance={}"),
        cmd_args(str(ctx.label), format = "--label={}"),
        cmd_args(identifier, format = "--identifier={}"),
        cmd_args(parent, format = "--parent={}") if parent else cmd_args(),
        cmd_args(out.as_output(), format = "--output={}"),
        cmd,
    )
    ctx.actions.run(
        cmd,
        category = "antlir2_map",
        identifier = identifier,
        # needs local subvolumes
        local_only = True,
        # 'antlir2 isolate' will clean up an old image if it exists
        no_outputs_cleanup = True,
        env = {
            "RUST_LOG": "antlir2=trace",
        },
    )
    return cmd, out

def _impl(ctx: "context") -> ["provider"]:
    if not ctx.attrs.flavor and not ctx.attrs.parent_layer:
        fail("'flavor' must be set if there is no 'parent_layer'")

    flavor_info = ctx.attrs.flavor[FlavorInfo] if ctx.attrs.flavor else ctx.attrs.parent_layer[LayerInfo].flavor_info

    # traverse the features to find dependencies this image build has on other
    # image layers
    dependency_layers = []
    feature_hidden_deps = []
    for dep in flatten.flatten(ctx.attrs.features[FeatureInfo].deps.traverse()):
        if type(dep) == "dependency" and LayerInfo in dep:
            dependency_layers.append(dep[LayerInfo])
        elif type(dep) == "dependency":
            feature_hidden_deps.append(ensure_single_output(dep))

            # TODO(vmagro): features should be able to provide better dep info
            # instead of doing things like this...
            if RunInfo in dep:
                feature_hidden_deps.append(dep[RunInfo])

        if type(dep) == "artifact":
            feature_hidden_deps.append(dep)

    depgraph_input = build_depgraph(ctx, "json", None, dependency_layers)

    available_rpm_repos = (ctx.attrs.available_rpm_repos or flavor_info.default_rpm_repo_set)[RepoSetInfo]
    dnf_repodatas = repodata_only_local_repos(ctx, available_rpm_repos)

    mounts = ctx.actions.declare_output("mounts.json")
    ctx.actions.run(cmd_args(
        ctx.attrs.antlir2[RunInfo],
        "serialize-mounts",
        ctx.attrs.features[FeatureInfo].json_files.project_as_args("feature_json"),
        cmd_args(ctx.attrs.parent_layer[LayerInfo].mounts, format = "--parent={}") if ctx.attrs.parent_layer else cmd_args(),
        cmd_args(mounts.as_output(), format = "--out={}"),
    ).hidden([dep.mounts for dep in dependency_layers]), category = "antlir2", identifier = "serialize_mounts")

    plan = ctx.actions.declare_output("plan")
    plan_cmd, _ = _map_image(
        ctx = ctx,
        cmd = cmd_args(
            cmd_args(dnf_repodatas, format = "--dnf-repos={}"),
            "plan",
            cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
            cmd_args(depgraph_input, format = "--depgraph-json={}"),
            cmd_args(collections.uniq([li.subvol_symlink for li in dependency_layers]), format = "--image-dependency={}"),
            cmd_args(plan.as_output(), format = "--plan={}"),
        ).hidden(feature_hidden_deps),
        identifier = "plan",
        parent = ctx.attrs.parent_layer[LayerInfo].subvol_symlink if ctx.attrs.parent_layer else None,
        flavor_info = flavor_info,
    )

    # Part of the compiler plan is any possible dnf transaction resolution,
    # which lets us know what rpms we will need. We can have buck download them
    # and mount in a pre-built directory of all repositories for
    # completely-offline dnf installation (which is MUCH faster and more
    # reliable)
    # TODO: this is basically a no-op if there are no features that require
    # pre-planning, but it does still invoke antlir2 inside of the container so
    # takes ~1-2s even if it does nothing. The performance benefit for the
    # extremely common use case of installing rpms absolutely dwarfs this cost,
    # but it would be nice to only run this if there are relevant features
    dnf_repos_dir = compiler_plan_to_local_repos(ctx, available_rpm_repos, plan)

    compile_cmd, final_subvol = _map_image(
        ctx = ctx,
        cmd = cmd_args(
            cmd_args(dnf_repos_dir, format = "--dnf-repos={}"),
            "compile",
            cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
            cmd_args(depgraph_input, format = "--depgraph-json={}"),
            cmd_args(collections.uniq([li.subvol_symlink for li in dependency_layers]), format = "--image-dependency={}"),
        ).hidden(feature_hidden_deps),
        identifier = "compile",
        parent = ctx.attrs.parent_layer[LayerInfo].subvol_symlink if ctx.attrs.parent_layer else None,
        flavor_info = flavor_info,
    )

    depgraph_output = build_depgraph(ctx, "json", final_subvol, dependency_layers)

    # This script is provided solely for developer convenience. It would
    # actually be a large regression to run this to produce the final image
    # during normal buck operation, as it would prevent buck from caching
    # individual actions when possible (for example, if rpm features do not
    # change, the transaction plan might be cached)
    debug_sequence = [plan_cmd, compile_cmd]
    build_script = ctx.actions.write(
        "build.sh",
        cmd_args(
            "#!/bin/bash -e",
            "export RUST_LOG=warn,antlir2=trace",
            [cmd_args(
                c,
                delimiter = " \\\n  ",
                quote = "shell",
            ) for c in debug_sequence],
            "\n",
        ),
        is_executable = True,
    )

    return [
        LayerInfo(
            label = ctx.label,
            flavor_info = flavor_info,
            depgraph = depgraph_output,
            subvol_symlink = final_subvol,
            parent = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None,
            mounts = mounts,
        ),
        DefaultInfo(
            default_outputs = [final_subvol],
            sub_targets = {
                "build.sh": [
                    DefaultInfo(build_script),
                    RunInfo(args = cmd_args("/bin/bash", "-e", build_script).hidden(debug_sequence)),
                ],
                "depgraph": [DefaultInfo(
                    default_outputs = [],
                    sub_targets = {
                        "input.dot": [DefaultInfo(default_outputs = [build_depgraph(ctx, "dot", None, dependency_layers)])],
                        "input.json": [DefaultInfo(default_outputs = [depgraph_input])],
                        "output.dot": [DefaultInfo(default_outputs = [build_depgraph(ctx, "dot", final_subvol, dependency_layers)])],
                        "output.json": [DefaultInfo(default_outputs = [depgraph_output])],
                    },
                )],
                "plan": [
                    DefaultInfo(default_outputs = [plan]),
                ],
            },
        ),
    ]

_layer = rule(
    impl = _impl,
    attrs = {
        "antlir2": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2:antlir2")),
        "available_rpm_repos": attrs.option(attrs.dep(providers = [RepoSetInfo]), default = None),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "features": attrs.dep(providers = [FeatureInfo]),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo]), default = None),
        "parent_layer": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        # TODO: this can (and should) be be attrs.default_only once antlir2 is
        # fully self hosting (almost done)
        "target_arch": attrs.default_only(attrs.string(
            default =
                select({
                    "ovr_config//cpu:arm64": "aarch64",
                    "ovr_config//cpu:x86_64": "x86_64",
                }),
        )),
    },
)

def layer(
        *,
        name: str.type,
        # Features does not have a direct type hint, but it is still validated
        # by a type hint inside feature.bzl. Feature targets or
        # InlineFeatureInfo providers are accepted, at any level of nesting
        features = [],
        # We'll implicitly forward some users to antlir2, so any hacks for them
        # should be confined behind this flag
        implicit_antlir2: bool.type = False,
        visibility: [[str.type], None] = None,
        **kwargs):
    """
    Create a new image layer

    Build a new image layer from the given `features` and `parent_layer`.
    """
    features = flatten.flatten(features, item_type = ["InlineFeatureInfo", str.type])

    if implicit_antlir2:
        flavor = kwargs.pop("flavor", None)
        if flavor:
            if not types.is_string(flavor):
                flavor = flavor.unaliased_name
            if ":" not in flavor:
                flavor = "//antlir/antlir2/facebook/flavor:" + flavor
        kwargs["flavor"] = flavor

    feature_target = name + "--features"
    feature.new(
        name = feature_target,
        visibility = [":" + name],
        features = features,
    )
    feature_target = ":" + feature_target

    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    return _layer(
        name = name,
        features = feature_target,
        visibility = get_visibility(visibility),
        **kwargs
    )
