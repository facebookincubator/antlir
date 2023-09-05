# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @starlark-rust: allow_string_literals_in_type_expr

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:compat.bzl", "compat")
load("//antlir/antlir2/bzl:lazy.bzl", "lazy")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "FlavorInfo", "LayerInfo")
load("//antlir/antlir2/bzl/dnf:defs.bzl", "compiler_plan_to_local_repos", "repodata_only_local_repos")
load("//antlir/antlir2/bzl/feature:feature.bzl", "feature_attrs", "feature_rule", "regroup_features", "shared_features_attrs")
# @oss-disable
# @oss-disable
load("//antlir/bzl:build_defs.bzl", "alias", "is_facebook")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:types.bzl", "types")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoInfo", "RepoSetInfo")
# @oss-disable
load("//antlir/bzl/build_defs.bzl", "config", "get_visibility")
load(":depgraph.bzl", "build_depgraph")
load(":mounts.bzl", "all_mounts", "nspawn_mount_args")

def _map_image(
        ctx: AnalysisContext,
        cmd: "cmd_args",
        identifier: str,
        build_appliance: LayerInfo.type,
        parent: Artifact | None,
        logs: Artifact) -> ("cmd_args", Artifact):
    """
    Take the 'parent' image, and run some command through 'antlir2 map' to
    produce a new image.
    In other words, this is a mapping function of 'image A -> A1'
    """
    antlir2 = ctx.attrs.antlir2[RunInfo]
    out = ctx.actions.declare_output("subvol-" + identifier)
    keepalive = ctx.actions.declare_output("keepalive-" + identifier)
    cmd = cmd_args(
        "sudo",  # this requires privileged btrfs operations
        antlir2,
        cmd_args(logs.as_output(), format = "--logs={}"),
        "map",
        "--working-dir=antlir2-out",
        cmd_args(build_appliance.subvol_symlink, format = "--build-appliance={}"),
        cmd_args(str(ctx.label), format = "--label={}"),
        cmd_args(identifier, format = "--identifier={}"),
        cmd_args(parent, format = "--parent={}") if parent else cmd_args(),
        cmd_args(out.as_output(), format = "--output={}"),
        cmd_args(keepalive.as_output(), format = "--keepalive={}"),
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
    )

    return cmd, out

def _nspawn_sub_target(nspawn_binary: Dependency, subvol: Artifact, mounts: list["mount_record"]) -> list[Provider]:
    dev_mode_args = cmd_args()
    if REPO_CFG.artifacts_require_repo:
        dev_mode_args = cmd_args(
            "--artifacts-require-repo",
            cmd_args([cmd_args("--bind-mount-ro", p, p) for p in REPO_CFG.host_mounts_for_repo_artifacts]),
        )
    return [
        DefaultInfo(),
        RunInfo(cmd_args(
            "sudo",
            nspawn_binary[RunInfo],
            cmd_args(subvol, format = "--subvol={}"),
            cmd_args([nspawn_mount_args(mount) for mount in mounts]),
            dev_mode_args,
        )),
    ]

def _implicit_image_test(subvol: Artifact, implicit_image_test: ExternalRunnerTestInfo.type) -> ExternalRunnerTestInfo.type:
    implicit_image_test = ExternalRunnerTestInfo(
        type = implicit_image_test.test_type,
        command = implicit_image_test.command,
        env = (implicit_image_test.env or {}) | {"ANTLIR2_LAYER": subvol},
        labels = [],
        run_from_project_root = True,
    )
    return implicit_image_test

def _impl(ctx: AnalysisContext) -> "promise":
    if not ctx.attrs.flavor and not ctx.attrs.parent_layer:
        fail("'flavor' must be set if there is no 'parent_layer'")

    feature_anon_kwargs = {key.removeprefix("_feature_"): getattr(ctx.attrs, key) for key in dir(ctx.attrs) if key.startswith("_feature_")}
    feature_anon_kwargs["_objcopy"] = ctx.attrs._objcopy
    feature_anon_kwargs["name"] = str(ctx.label.raw_target())
    return ctx.actions.anon_target(
        feature_rule,
        feature_anon_kwargs,
    ).map(partial(_impl_with_features, ctx = ctx))

def _impl_with_features(features: "provider_collection", *, ctx: AnalysisContext) -> list[Provider]:
    flavor = ctx.attrs.flavor or ctx.attrs.parent_layer[LayerInfo].flavor
    if not ctx.attrs.antlir_internal_build_appliance and not flavor:
        fail("flavor= was not set, and {} does not have a flavor".format(ctx.attrs.parent_layer.label))
    flavor_info = flavor[FlavorInfo] if flavor else None
    build_appliance = ctx.attrs.build_appliance or flavor_info.default_build_appliance

    # Expose a number of things as sub-targets for both humans doing `buck
    # build` and cases where we must access a specific output from the macro
    # layer where we don't have proper rules and access to providers
    sub_targets = {
        "features": [features[FeatureInfo], features[DefaultInfo]],
    }
    if ctx.attrs.parent_layer:
        sub_targets["parent_layer"] = ctx.attrs.parent_layer.providers

    # Expose the build appliance as a subtarget so that it can be used by test
    # macros like image_rpms_test. Generally this should be accessed by the
    # provider, but that is unavailable at macro parse time.
    if build_appliance:
        sub_targets["build_appliance"] = build_appliance.providers

    if flavor:
        sub_targets["flavor"] = flavor.providers

    all_features = features[FeatureInfo].features

    dnf_available_repos = []
    if types.is_list(ctx.attrs.dnf_available_repos):
        dnf_available_repos = [r[RepoInfo] for r in ctx.attrs.dnf_available_repos]
    elif ctx.attrs.dnf_available_repos != None:
        dnf_available_repos = list(ctx.attrs.dnf_available_repos[RepoSetInfo].repo_infos)
    else:
        dnf_available_repos = list(flavor_info.dnf_info.default_repo_set[RepoSetInfo].repo_infos)
    for repo in (ctx.attrs.dnf_additional_repos or []):
        if RepoSetInfo in repo:
            dnf_available_repos.extend(repo[RepoSetInfo].repo_infos)
        else:
            dnf_available_repos.append(repo[RepoInfo])
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

        # TODO(vmagro): when we introduce other package managers, this will need
        # to be more generic
        features = regroup_features(ctx.label, features)

        # JSON file for only the features that are part of this BuildPhase
        features_json_artifact = ctx.actions.declare_output(identifier_prefix + "features.json")
        features_json = ctx.actions.write_json(
            features_json_artifact,
            [{
                "data": f.analysis.data,
                "feature_type": f.feature_type,
                "label": f.label,
                "run_info": f.run_info,
            } for f in features],
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
            [feat.run_info for feat in features],
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

            if is_facebook:
                available_fbpkgs = ctx.attrs.available_fbpkgs[SnapshottedFbpkgSetInfo]
                (resolved_fbpkgs_json, resolved_fbpkgs_dir) = compiler_plan_to_chef_fbpkgs(
                    ctx,
                    identifier_prefix,
                    available_fbpkgs,
                    plan,
                )
            else:
                resolved_fbpkgs_json = None
                resolved_fbpkgs_dir = None
        else:
            plan = None
            dnf_repos_dir = ctx.actions.symlinked_dir(identifier_prefix + "empty-dnf-repos", {})
            resolved_fbpkgs_json = None
            resolved_fbpkgs_dir = None

        logs["compile"] = ctx.actions.declare_output(identifier_prefix + "compile.log")
        if resolved_fbpkgs_dir:
            feature_hidden_deps.append(resolved_fbpkgs_dir)

        cmd, final_subvol = _map_image(
            build_appliance = build_appliance[LayerInfo],
            cmd = cmd_args(
                cmd_args(dnf_repos_dir, format = "--dnf-repos={}"),
                cmd_args(dnf_versionlock, format = "--dnf-versionlock={}") if dnf_versionlock else cmd_args(),
                # @oss-disable
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

        final_depgraph = build_depgraph(
            ctx = ctx,
            dependency_layers = dependency_layers,
            features_json = None,
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
                cmd_args(
                    [cmd_args(cmd, delimiter = " ", quote = "shell") for cmd in build_cmd],
                    delimiter = "\n\n",
                ),
                "\n",
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
                    "container": _nspawn_sub_target(ctx.attrs._run_nspawn, final_subvol, mounts = phase_mounts),
                    "features": [DefaultInfo(features_json_artifact)],
                    "logs": [DefaultInfo(all_logs, sub_targets = {
                        key: [DefaultInfo(artifact)]
                        for key, artifact in logs.items()
                    })],
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

    sub_targets["subvol_symlink"] = [DefaultInfo(final_subvol)]

    parent_layer_info = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None
    mounts = all_mounts(features = all_features, parent_layer = parent_layer_info)
    # @oss-disable

    sub_targets["container"] = _nspawn_sub_target(ctx.attrs._run_nspawn, final_subvol, mounts)
    sub_targets["debug"] = [DefaultInfo(sub_targets = debug_sub_targets)]

    providers = [
        LayerInfo(
            build_appliance = build_appliance,
            depgraph = final_depgraph,
            flavor = flavor,
            flavor_info = flavor_info,
            label = ctx.label,
            mounts = mounts,
            parent = ctx.attrs.parent_layer,
            subvol_symlink = final_subvol,
            features = all_features,
        ),
        DefaultInfo(final_subvol, sub_targets = sub_targets),
    ]

    if ctx.attrs._implicit_image_test:
        providers.append(
            _implicit_image_test(final_subvol, ctx.attrs._implicit_image_test[ExternalRunnerTestInfo]),
        )
    return providers

_layer_attrs = {
    "antlir2": attrs.exec_dep(default = "//antlir/antlir2/antlir2:antlir2"),
    "antlir_internal_build_appliance": attrs.bool(default = False, doc = "mark if this image is a build appliance and is allowed to not have a flavor"),
    "build_appliance": attrs.option(
        attrs.dep(providers = [LayerInfo]),
        default = None,
    ),
    "dnf_additional_repos": attrs.list(
        attrs.one_of(
            attrs.dep(providers = [RepoInfo]),
            attrs.dep(providers = [RepoSetInfo]),
        ),
        default = [],
        doc = """
            Make more dnf repos available while building this layer.
        """,
    ),
    "dnf_available_repos": attrs.option(
        attrs.one_of(
            attrs.list(attrs.dep(providers = [RepoInfo])),
            attrs.dep(providers = [RepoSetInfo]),
        ),
        default = None,
        doc = """
            Restrict the available dnf repos while building this layer to this
            repo_set and anything in dnf_additional_repos
        """,
    ),
    "dnf_excluded_rpms": attrs.option(
        attrs.list(attrs.string()),
        default = None,
    ),
    "dnf_versionlock": attrs.option(
        attrs.source(),
        default = None,
    ),
    "flavor": attrs.option(
        attrs.dep(providers = [FlavorInfo]),
        default = None,
    ),
    "parent_layer": attrs.option(
        attrs.dep(providers = [LayerInfo]),
        default = None,
    ),
    "target_arch": attrs.default_only(attrs.string(
        default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
    )),
    "_implicit_image_test": attrs.option(
        attrs.exec_dep(providers = [ExternalRunnerTestInfo]),
        default = None,
    ),
    "_objcopy": attrs.exec_dep(default = "fbsource//third-party/binutils:objcopy"),
    "_run_nspawn": attrs.exec_dep(default = "//antlir/antlir2/nspawn_in_subvol:nspawn"),
}

_layer_attrs.update(
    {
        "_feature_" + key: val
        for key, val in shared_features_attrs.items()
    },
)

# @oss-disable

layer_rule = rule(
    impl = _impl,
    attrs = _layer_attrs,
)

def layer(
        *,
        name: str,
        # Features does not have a direct type hint, but it is still validated
        # by a type hint inside feature.bzl. Feature targets or
        # InlineFeatureInfo providers are accepted, at any level of nesting
        features = [],
        # We'll implicitly forward some users to antlir2, so any hacks for them
        # should be confined behind this flag
        implicit_antlir2: bool = False,
        visibility: list[str] | None = None,
        **kwargs):
    """
    Create a new image layer

    Build a new image layer from the given `features` and `parent_layer`.
    """
    if implicit_antlir2:
        flavor = kwargs.pop("flavor", None)
        kwargs["flavor"] = compat.from_antlir1_flavor(flavor) if flavor else None

    kwargs.update({"_feature_" + key: val for key, val in feature_attrs(features).items()})

    if is_facebook:
        # available_fbpkgs is logically an optional dep, but to make it truly
        # optional for using layers in anon_targets, we must just set the
        # default at the macro layer
        # TODO(vmagro): the best fix is to introduce this dep only on the
        # chef_solo feature itself, but that's trickier to accomplish
        kwargs.setdefault(
            "available_fbpkgs",
            "//bot_generated/antlir/fbpkg/db/main_db/.buck:snapshotted_fbpkgs",
        )

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

    return layer_rule(
        name = name,
        visibility = get_visibility(visibility),
        _implicit_image_test = "//antlir/antlir2/testing/implicit_image_test:implicit_image_test",
        **kwargs
    )
