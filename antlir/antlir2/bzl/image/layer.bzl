# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase", "build_phase")
load("//antlir/antlir2/bzl:lazy.bzl", "lazy")
load("//antlir/antlir2/bzl:toolchain.bzl", "Antlir2ToolchainInfo")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "FlavorInfo", "LayerInfo")
load("//antlir/antlir2/bzl/dnf:defs.bzl", "compiler_plan_to_local_repos", "repodata_only_local_repos")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
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
    toolchain = ctx.attrs.toolchain[Antlir2ToolchainInfo]
    out = ctx.actions.declare_output("subvol-" + identifier)
    build_appliance = (ctx.attrs.build_appliance or flavor_info.default_build_appliance)[LayerInfo]
    cmd = cmd_args(
        "sudo",  # this requires privileged btrfs operations
        toolchain.antlir2[RunInfo],
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

    toolchain = ctx.attrs.toolchain[Antlir2ToolchainInfo]

    flavor_info = ctx.attrs.flavor[FlavorInfo] if ctx.attrs.flavor else ctx.attrs.parent_layer[LayerInfo].flavor_info

    all_features = ctx.attrs.features[FeatureInfo]
    all_features_json = ctx.actions.write_json("all_features.json", all_features.features.project_as_json("features_json"), with_inputs = True)

    # traverse the features to find dependencies this image build has on other
    # image layers
    # TODO(vmagro): these should be broken out by build_phase as well
    dependency_layers = flatten.flatten(list(ctx.attrs.features[FeatureInfo].required_layers.traverse()))
    feature_hidden_deps = [
        all_features.required_artifacts.project_as_args("hidden_artifacts"),
        all_features.required_run_infos.project_as_args("hidden_run_infos"),
    ]

    mounts = ctx.actions.declare_output("mounts.json")
    ctx.actions.run(cmd_args(
        toolchain.antlir2[RunInfo],
        "serialize-mounts",
        cmd_args(all_features_json, format = "--feature-json={}"),
        cmd_args(ctx.attrs.parent_layer[LayerInfo].mounts, format = "--parent={}") if ctx.attrs.parent_layer else cmd_args(),
        cmd_args(mounts.as_output(), format = "--out={}"),
    ).hidden([dep.mounts for dep in dependency_layers]), category = "antlir2", identifier = "serialize_mounts")

    dnf_available_repos = (ctx.attrs.dnf_available_repos or flavor_info.dnf_info.default_repo_set)[RepoSetInfo]
    dnf_repodatas = repodata_only_local_repos(ctx, dnf_available_repos)
    dnf_versionlock = ctx.attrs.dnf_versionlock or flavor_info.dnf_info.default_versionlock

    # The image build is split into phases based on features' `build_phase`
    # property.
    # This gets us some caching benefits (for example, if a feature in a layer
    # changed but does not change the rpm installations, that intermediate layer
    # can still be cached and not have to re-install rpms).
    #
    # Equally importantly, this enables more correctness in the dependency
    # graph, since the depgraph will immediately recognize any rpm-installed
    # files in the layer, users created by package installation, etc.
    #
    # Effectively, this is the same as if image authors separated their layer
    # rules into a layer that installs rpms, then an immediate child layer that
    # contains all the other features. In practice that's really hard and
    # inconvenient for image authors, but is incredibly useful for everyone
    # involved, so we can do it for them implicitly.

    parent_layer = ctx.attrs.parent_layer[LayerInfo].subvol_symlink if ctx.attrs.parent_layer else None
    parent_depgraph = ctx.attrs.parent_layer[LayerInfo].depgraph if ctx.attrs.parent_layer else None
    final_subvol = None
    final_depgraph = None

    for phase in BuildPhase.values():
        phase = BuildPhase(phase)
        identifier_prefix = phase.value + "_" if phase.value else ""
        features = [
            feat
            for feat in all_features.features.traverse()
            if feat.analysis.build_phase == phase
        ]

        # Build phase can be skipped if it doesn't contain any features, but if
        # this is the final phase and nothing has been built yet, we need to
        # fall through and produce an empty subvolume so it can still be used as
        # a parent_layer and/or snapshot its own parent's contents
        if not features and not (phase == BuildPhase(None) and parent_layer == None):
            continue

        features_json = ctx.actions.write_json(
            identifier_prefix + "features.json",
            [struct(feature_type = f.feature_type, label = f.label, data = f.analysis.data) for f in features],
            with_inputs = True,
        )
        depgraph_input = build_depgraph(
            ctx = ctx,
            parent_depgraph = parent_depgraph,
            features = all_features,
            features_json = features_json,
            format = "json",
            subvol = None,
            dependency_layers = dependency_layers,
            identifier_prefix = identifier_prefix,
        )

        if lazy.any(lambda feat: feat.analysis.requires_planning, features):
            plan = ctx.actions.declare_output("plan")
            _map_image(
                ctx = ctx,
                cmd = cmd_args(
                    cmd_args(dnf_repodatas, format = "--dnf-repos={}"),
                    cmd_args(dnf_versionlock, format = "--dnf-versionlock={}") if dnf_versionlock else cmd_args(),
                    "plan",
                    cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
                    cmd_args(depgraph_input, format = "--depgraph-json={}"),
                    cmd_args([li.depgraph for li in all_features.required_layers.reduce("unique")], format = "--image-dependency={}") if features else cmd_args(),
                    cmd_args(plan.as_output(), format = "--plan={}"),
                ).hidden(feature_hidden_deps),
                identifier = identifier_prefix + "plan",
                parent = parent_layer,
                flavor_info = flavor_info,
            )

            # Part of the compiler plan is any possible dnf transaction resolution,
            # which lets us know what rpms we will need. We can have buck download them
            # and mount in a pre-built directory of all repositories for
            # completely-offline dnf installation (which is MUCH faster and more
            # reliable)
            dnf_repos_dir = compiler_plan_to_local_repos(ctx, dnf_available_repos, plan)
        else:
            plan = None
            dnf_repos_dir = ctx.actions.symlinked_dir(identifier_prefix + "empty-dnf-repos", {})

        _, final_subvol = _map_image(
            ctx = ctx,
            cmd = cmd_args(
                cmd_args(dnf_repos_dir, format = "--dnf-repos={}"),
                cmd_args(dnf_versionlock, format = "--dnf-versionlock={}") if dnf_versionlock else cmd_args(),
                "compile",
                cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
                cmd_args(depgraph_input, format = "--depgraph-json={}"),
                cmd_args([li.depgraph for li in all_features.required_layers.reduce("unique")], format = "--image-dependency={}") if features else cmd_args(),
            ).hidden(feature_hidden_deps),
            identifier = identifier_prefix + "compile",
            parent = parent_layer,
            flavor_info = flavor_info,
        )

        if build_phase.is_predictable(phase):
            final_depgraph = depgraph_input
        else:
            # If this phase was not predictable, we need to walk the fs to make
            # sure we're not missing any files/users/groups/whatever
            final_depgraph = build_depgraph(
                ctx = ctx,
                parent_depgraph = parent_depgraph,
                features = all_features,
                features_json = features_json,
                format = "json",
                subvol = final_subvol,
                dependency_layers = dependency_layers,
                identifier_prefix = identifier_prefix,
            )

        parent_layer = final_subvol
        parent_depgraph = final_depgraph

    # If final_subvol was not produced, that means that this layer is devoid of
    # features, so just present the parent artifacts as our own. This is a weird
    # use case, but sometimes creating layers with no features makes life easier
    # for macro authors, so antlir2 should allow it.
    if not final_subvol:
        return [
            ctx.attrs.parent_layer[LayerInfo],
            DefaultInfo(ctx.attrs.parent_layer[LayerInfo].subvol_symlink),
        ]

    return [
        LayerInfo(
            label = ctx.label,
            flavor_info = flavor_info,
            depgraph = final_depgraph,
            subvol_symlink = final_subvol,
            parent = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None,
            mounts = mounts,
        ),
        DefaultInfo(final_subvol),
    ]

_layer = rule(
    impl = _impl,
    attrs = {
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "dnf_available_repos": attrs.option(attrs.dep(providers = [RepoSetInfo]), default = None),
        "dnf_versionlock": attrs.option(attrs.source(), default = None),
        "features": attrs.dep(providers = [FeatureInfo]),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo]), default = None),
        "parent_layer": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "target_arch": attrs.default_only(attrs.string(
            default =
                select({
                    "ovr_config//cpu:arm64": "aarch64",
                    "ovr_config//cpu:x86_64": "x86_64",
                }),
        )),
        "toolchain": attrs.toolchain_dep(
            providers = [Antlir2ToolchainInfo],
            default = "//antlir/antlir2:toolchain",
        ),
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
