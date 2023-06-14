# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase", "build_phase")
load("//antlir/antlir2/bzl:compat.bzl", "compat")
load("//antlir/antlir2/bzl:lazy.bzl", "lazy")
load("//antlir/antlir2/bzl:toolchain.bzl", "Antlir2ToolchainInfo")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "FlavorInfo", "LayerInfo")
load("//antlir/antlir2/bzl/dnf:defs.bzl", "compiler_plan_to_local_repos", "repodata_only_local_repos")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/bzl:build_defs.bzl", "alias")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoSetInfo")
load("//antlir/bzl/build_defs.bzl", "config", "get_visibility")
load(":depgraph.bzl", "build_depgraph")
load(":mounts.bzl", "all_mounts", "nspawn_mount_args")

def _map_image(
        ctx: "context",
        cmd: "cmd_args",
        identifier: str.type,
        build_appliance: LayerInfo.type,
        parent: ["artifact", None],
        logs: "artifact") -> ("cmd_args", "artifact"):
    """
    Take the 'parent' image, and run some command through 'antlir2 map' to
    produce a new image.
    In other words, this is a mapping function of 'image A -> A1'
    """
    toolchain = ctx.attrs.toolchain[Antlir2ToolchainInfo]
    out = ctx.actions.declare_output("subvol-" + identifier)
    cmd = cmd_args(
        "sudo",  # this requires privileged btrfs operations
        toolchain.antlir2[RunInfo],
        cmd_args(logs.as_output(), format = "--logs={}"),
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
        env = {
            "RUST_LOG": "antlir2=trace",
        },
        identifier = identifier,
        # needs local subvolumes
        local_only = True,
        # 'antlir2 isolate' will clean up an old image if it exists
        no_outputs_cleanup = True,
    )

    return cmd, out

def _nspawn_sub_target(subvol: "artifact", mounts: ["mount_record"]) -> ["provider"]:
    return [
        DefaultInfo(),
        RunInfo(cmd_args(
            "sudo",
            "systemd-nspawn",
            "--ephemeral",
            "--directory",
            subvol,
            cmd_args([nspawn_mount_args(mount) for mount in mounts]),
        )),
    ]

def _impl(ctx: "context") -> ["provider"]:
    if not ctx.attrs.flavor and not ctx.attrs.parent_layer:
        fail("'flavor' must be set if there is no 'parent_layer'")

    flavor = ctx.attrs.flavor or ctx.attrs.parent_layer[LayerInfo].flavor
    if not ctx.attrs.antlir_internal_build_appliance and not flavor:
        fail("flavor= was not set, and {} does not have a flavor".format(ctx.attrs.parent_layer.label))
    flavor_info = flavor[FlavorInfo] if flavor else None
    build_appliance = ctx.attrs.build_appliance or flavor_info.default_build_appliance

    # Yeah this is against the spirit of Transitive Sets, but we can save an
    # insane amount of actual image building work if we do the "wrong thing" and
    # expect it in starlark instead of a cli/json projection.
    all_features = list(ctx.attrs.features[FeatureInfo].features.traverse())

    dnf_available_repos = (ctx.attrs.dnf_available_repos or flavor_info.dnf_info.default_repo_set)[RepoSetInfo]
    dnf_repodatas = repodata_only_local_repos(ctx, dnf_available_repos)
    dnf_versionlock = ctx.attrs.dnf_versionlock or flavor_info.dnf_info.default_versionlock
    dnf_excluded_rpms = ctx.attrs.dnf_excluded_rpms if ctx.attrs.dnf_excluded_rpms != None else flavor_info.dnf_info.default_excluded_rpms
    if dnf_excluded_rpms:
        dnf_excluded_rpms = ctx.actions.write_json("excluded_rpms.json", dnf_excluded_rpms)
    else:
        dnf_excluded_rpms = None

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
    debug_sub_targets = {}

    for phase in BuildPhase.values():
        phase = BuildPhase(phase)
        build_cmd = []
        logs = {}

        identifier_prefix = phase.value + "_"
        features = [
            feat
            for feat in all_features
            if feat.analysis.build_phase == phase
        ]

        # Build phase can be skipped if it doesn't contain any features, but if
        # this is the final phase and nothing has been built yet, we need to
        # fall through and produce an empty subvolume so it can still be used as
        # a parent_layer and/or snapshot its own parent's contents
        if not features and not (phase == BuildPhase("compile") and parent_layer == None):
            continue

        # JSON file for only the features that are part of this BuildPhase
        features_json = ctx.actions.write_json(
            identifier_prefix + "features.json",
            [{"data": f.analysis.data, "feature_type": f.feature_type, "label": f.label} for f in features],
            with_inputs = True,
        )

        # Features in this phase may depend on other image layers, or may
        # require artifacts to be materialized on disk.
        # Layers are deduped because it can accidentaly trigger some expensive
        # work if the same layer is passed many times as cli args
        dependency_layers = []
        for feat in features:
            for layer in feat.analysis.required_layers:
                if layer not in dependency_layers:
                    dependency_layers.append(layer)
        feature_hidden_deps = [
            [feat.analysis.required_artifacts for feat in features],
            [feat.analysis.required_run_infos for feat in features],
        ]

        depgraph_input = build_depgraph(
            ctx = ctx,
            dependency_layers = dependency_layers,
            features_json = features_json,
            format = "json",
            identifier_prefix = identifier_prefix,
            parent_depgraph = parent_depgraph,
            subvol = None,
        )

        compileish_args = cmd_args(
            cmd_args(ctx.attrs.target_arch, format = "--target-arch={}"),
            cmd_args(depgraph_input, format = "--depgraph-json={}"),
            cmd_args([li.depgraph for li in dependency_layers], format = "--image-dependency={}"),
        )

        if lazy.any(lambda feat: feat.analysis.requires_planning, features):
            plan = ctx.actions.declare_output(identifier_prefix + "plan.json")
            logs["plan"] = ctx.actions.declare_output(identifier_prefix + "plan.log")
            cmd, _ = _map_image(
                build_appliance = build_appliance[LayerInfo],
                cmd = cmd_args(
                    cmd_args(dnf_repodatas, format = "--dnf-repos={}"),
                    cmd_args(dnf_versionlock, format = "--dnf-versionlock={}") if dnf_versionlock else cmd_args(),
                    cmd_args(dnf_excluded_rpms, format = "--dnf-excluded-rpms={}") if dnf_excluded_rpms else cmd_args(),
                    "plan",
                    compileish_args,
                    cmd_args(plan.as_output(), format = "--plan={}"),
                ).hidden(feature_hidden_deps),
                ctx = ctx,
                identifier = identifier_prefix + "plan",
                parent = parent_layer,
                logs = logs["plan"],
            )
            build_cmd.append(cmd)

            # Part of the compiler plan is any possible dnf transaction resolution,
            # which lets us know what rpms we will need. We can have buck download them
            # and mount in a pre-built directory of all repositories for
            # completely-offline dnf installation (which is MUCH faster and more
            # reliable)
            dnf_repos_dir = compiler_plan_to_local_repos(
                ctx,
                identifier_prefix,
                dnf_available_repos,
                plan,
                flavor_info.dnf_info.reflink_flavor,
            )
        else:
            plan = None
            dnf_repos_dir = ctx.actions.symlinked_dir(identifier_prefix + "empty-dnf-repos", {})

        logs["compile"] = ctx.actions.declare_output(identifier_prefix + "compile.log")
        cmd, final_subvol = _map_image(
            build_appliance = build_appliance[LayerInfo],
            cmd = cmd_args(
                cmd_args(dnf_repos_dir, format = "--dnf-repos={}"),
                cmd_args(dnf_versionlock, format = "--dnf-versionlock={}") if dnf_versionlock else cmd_args(),
                "compile",
                compileish_args,
                cmd_args(plan, format = "--plan={}") if plan else cmd_args(),
            ).hidden(feature_hidden_deps),
            ctx = ctx,
            identifier = identifier_prefix + "compile",
            parent = parent_layer,
            logs = logs["compile"],
        )
        build_cmd.append(cmd)

        if build_phase.is_predictable(phase):
            final_depgraph = depgraph_input
        else:
            # If this phase was not predictable, we need to walk the fs to make
            # sure we're not missing any files/users/groups/whatever
            final_depgraph = build_depgraph(
                ctx = ctx,
                dependency_layers = dependency_layers,
                features_json = features_json,
                format = "json",
                identifier_prefix = identifier_prefix,
                parent_depgraph = parent_depgraph,
                subvol = final_subvol,
            )

        build_script = ctx.actions.write(
            "{}_build.sh".format(identifier_prefix),
            cmd_args(
                "#!/bin/bash",
                "set -e",
                "export RUST_LOG=\"antlir2=trace\"",
                cmd_args(build_cmd, delimiter = "\n", quote = "shell"),
                delimiter = "\n",
            ),
            is_executable = True,
        )

        phase_mounts = all_mounts(
            features = features,
            parent_layer = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None,
        )
        all_logs = ctx.actions.declare_output(identifier_prefix + "logs", dir = True)
        ctx.actions.symlinked_dir(all_logs, {key + ".log": artifact for key, artifact in logs.items()})
        debug_sub_targets[phase.value] = [
            DefaultInfo(
                sub_targets = {
                    "build": [DefaultInfo(build_script), RunInfo(cmd_args(build_script))],
                    "logs": [DefaultInfo(all_logs)],
                    "nspawn": _nspawn_sub_target(final_subvol, mounts = phase_mounts),
                    "subvol": [DefaultInfo(final_subvol)],
                },
            ),
        ]

        parent_layer = final_subvol
        parent_depgraph = final_depgraph

    # If final_subvol was not produced, that means that this layer is devoid of
    # features, so just present the parent artifacts as our own. This is a weird
    # use case, but sometimes creating layers with no features makes life easier
    # for macro authors, so antlir2 should allow it.
    if not final_subvol:
        final_subvol = parent_layer

    sub_targets = {}

    # Expose the build appliance as a subtarget so that it can be used by test
    # macros like image_rpms_test. Generally this should be accessed by the
    # provider, but that is unavailable at macro parse time.
    if build_appliance:
        sub_targets["build_appliance"] = build_appliance.providers

    if flavor:
        sub_targets["flavor"] = flavor.providers

    mounts = all_mounts(
        features = all_features,
        parent_layer = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None,
    )

    sub_targets["nspawn"] = _nspawn_sub_target(final_subvol, mounts)
    sub_targets["debug"] = [DefaultInfo(sub_targets = debug_sub_targets)]
    if ctx.attrs.parent_layer:
        sub_targets["parent_layer"] = ctx.attrs.parent_layer.providers

    sub_targets["features"] = ctx.attrs.features.providers

    return [
        LayerInfo(
            build_appliance = build_appliance,
            depgraph = final_depgraph,
            flavor = flavor,
            flavor_info = flavor_info,
            label = ctx.label,
            mounts = mounts,
            parent = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None,
            subvol_symlink = final_subvol,
        ),
        DefaultInfo(final_subvol, sub_targets = sub_targets),
    ]

_layer = rule(
    impl = _impl,
    attrs = {
        "antlir_internal_build_appliance": attrs.bool(default = False, doc = "mark if this image is a build appliance and is allowed to not have a flavor"),
        "build_appliance": attrs.option(
            attrs.dep(providers = [LayerInfo]),
            default = None,
        ),
        "dnf_available_repos": attrs.option(
            attrs.dep(providers = [RepoSetInfo]),
            default = None,
        ),
        "dnf_excluded_rpms": attrs.option(
            attrs.list(attrs.string()),
            default = None,
        ),
        "dnf_versionlock": attrs.option(
            attrs.source(),
            default = None,
        ),
        "features": attrs.dep(providers = [FeatureInfo]),
        "flavor": attrs.option(
            attrs.dep(providers = [FlavorInfo]),
            default = None,
        ),
        "parent_layer": attrs.option(
            attrs.dep(providers = [LayerInfo]),
            default = None,
        ),
        "target_arch": attrs.default_only(attrs.string(
            default =
                select({
                    "ovr_config//cpu:arm64": "aarch64",
                    "ovr_config//cpu:x86_64": "x86_64",
                }),
        )),
        "toolchain": attrs.toolchain_dep(
            default = "//antlir/antlir2:toolchain",
            providers = [Antlir2ToolchainInfo],
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
        visibility: [
            [str.type],
            None,
        ] = None,
        **kwargs):
    """
    Create a new image layer

    Build a new image layer from the given `features` and `parent_layer`.
    """
    if implicit_antlir2:
        flavor = kwargs.pop("flavor", None)
        kwargs["flavor"] = compat.from_antlir1_flavor(flavor) if flavor else None

    feature_target = name + "--features"
    feature.new(
        name = feature_target,
        features = features,
        toolchain = kwargs.get("toolchain"),
        visibility = [":" + name],
        compatible_with = kwargs.get("compatible_with"),
    )
    feature_target = ":" + feature_target

    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    # TODO(vmagro): remove this when antlir1 compat is no longer needed
    # This exists only because the implicit antlir2 conversion rules append a
    # '.antlir2' suffix wherever a layer is involved. When the source layer is
    # antlir2, this suffixed layer will not exist so just make it an alias
    alias(
        name = name + ".antlir2",
        actual = ":" + name,
        antlir_rule = "user-internal",
        visibility = get_visibility(visibility),
    )

    return _layer(
        name = name,
        features = feature_target,
        visibility = get_visibility(visibility),
        **kwargs
    )
