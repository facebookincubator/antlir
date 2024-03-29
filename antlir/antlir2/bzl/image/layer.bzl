# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:expect.bzl", "expect")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase", "verify_build_phases")
load("//antlir/antlir2/bzl:compat.bzl", "compat")
load("//antlir/antlir2/bzl:lazy.bzl", "lazy")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "FlavorInfo", "LayerInfo")
load("//antlir/antlir2/bzl/dnf:defs.bzl", "compiler_plan_to_local_repos", "repodata_only_local_repos")
load("//antlir/antlir2/bzl/feature:feature.bzl", "feature_attrs", "feature_rule", "regroup_features", "shared_features_attrs")

load("//antlir/bzl:oss_shim.bzl", all_fbpkg_mounts = "ret_empty_list") # @oss-enable
# @oss-disable

load("//antlir/bzl:oss_shim.bzl", fb_attrs = "empty_dict", fb_defaults = "empty_dict") # @oss-enable
# @oss-disable
load(
    "//antlir/antlir2/features/mount:mount.bzl",
    "DefaultMountpointInfo",
)
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package", "should_all_images_in_package_use_default_os")
load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "RepoInfo", "RepoSetInfo")
load("//antlir/bzl:build_defs.bzl", "is_facebook")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:types.bzl", "types")

load("//antlir/bzl:oss_shim.bzl", SnapshottedFbpkgSetInfo = "none", compiler_plan_to_chef_fbpkgs = "ret_none") # @oss-enable
# @oss-disable
load("//antlir/bzl/build_defs.bzl", "config", "get_visibility")
load(":cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load(":depgraph.bzl", "build_depgraph")
load(":facts.bzl", "facts")
load(
    ":mounts.bzl",
    "all_mounts",
    "container_mount_args",
    "mount_record",  # @unused Used as type
)

def _map_image(
        ctx: AnalysisContext,
        cmd: cmd_args,
        identifier: str,
        build_appliance: LayerInfo | Provider,
        parent: Artifact | None,
        logs: Artifact,
        rootless: bool) -> (cmd_args, Artifact):
    """
    Take the 'parent' image, and run some command through 'antlir2 map' to
    produce a new image.
    In other words, this is a mapping function of 'image A -> A1'
    """
    antlir2 = ctx.attrs.antlir2[RunInfo]
    out = ctx.actions.declare_output("subvol-" + identifier)
    cmd = cmd_args(
        cmd_args("sudo") if not rootless else cmd_args(),
        antlir2,
        cmd_args(logs.as_output(), format = "--logs={}"),
        "map",
        "--working-dir=antlir2-out",
        cmd_args(build_appliance.subvol_symlink, format = "--build-appliance={}"),
        cmd_args(str(ctx.label), format = "--label={}"),
        cmd_args(identifier, format = "--identifier={}"),
        cmd_args(parent, format = "--parent={}") if parent else cmd_args(),
        cmd_args(out.as_output(), format = "--output={}"),
        cmd_args("--rootless") if rootless else cmd_args(),
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
        # the old output is used to clean up the local subvolume
        no_outputs_cleanup = True,
    )

    return cmd, out

def _container_sub_target(binary: Dependency | None, subvol: Artifact, mounts: list[mount_record]) -> list[Provider]:
    if not binary:
        return [DefaultInfo()]
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
            binary[RunInfo],
            cmd_args(subvol, format = "--subvol={}"),
            cmd_args([container_mount_args(mount) for mount in mounts]),
            dev_mode_args,
        )),
    ]

def _implicit_image_test(subvol: Artifact, implicit_image_test: ExternalRunnerTestInfo) -> ExternalRunnerTestInfo:
    implicit_image_test = ExternalRunnerTestInfo(
        type = implicit_image_test.test_type,
        command = implicit_image_test.command,
        env = (implicit_image_test.env or {}) | {"ANTLIR2_LAYER": subvol},
        labels = [],
        run_from_project_root = True,
    )
    return implicit_image_test

def _impl(ctx: AnalysisContext) -> Promise:
    feature_anon_kwargs = {key.removeprefix("_feature_"): getattr(ctx.attrs, key) for key in dir(ctx.attrs) if key.startswith("_feature_")}
    feature_anon_kwargs["name"] = str(ctx.label.raw_target())
    return ctx.actions.anon_target(
        feature_rule,
        feature_anon_kwargs,
    ).promise.map(partial(_impl_with_features, ctx = ctx))

def _identifier_prefix(prefix: str) -> str:
    return prefix

def _extra_repo_name_to_repo(repo_name: str, flavor_info: FlavorInfo) -> Dependency | None:
    default_repos = flavor_info.dnf_info.default_repo_set[RepoSetInfo].repos
    extra_repos = flavor_info.dnf_info.default_extra_repo_set[RepoSetInfo].repos

    for repo in extra_repos:
        if repo[RepoInfo].logical_id == repo_name:
            return repo

    for repo in default_repos:
        if repo[RepoInfo].logical_id == repo_name:
            return None

    fail("Unknown extra repo: {}. Possible choices are {}".format(
        repo_name,
        [repo[RepoInfo].logical_id for repo in extra_repos],
    ))

def _impl_with_features(features: ProviderCollection, *, ctx: AnalysisContext) -> list[Provider]:
    flavor = None
    if ctx.attrs.parent_layer and ctx.attrs.flavor:
        parent_flavor = ctx.attrs.parent_layer[LayerInfo].flavor
        if parent_flavor:
            expect(
                ctx.attrs.flavor.label.raw_target() == parent_flavor.label.raw_target(),
                "flavor ({}) was different from parent_layer's flavor ({})",
                ctx.attrs.flavor.label.raw_target(),
                parent_flavor.label.raw_target(),
            )
    if ctx.attrs.parent_layer:
        flavor = ctx.attrs.parent_layer[LayerInfo].flavor
    if not flavor:
        flavor = ctx.attrs.flavor
    if not ctx.attrs.antlir_internal_build_appliance and not flavor:
        fail("`flavor` is required")
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
        sub_targets["build_appliance"] = build_appliance.providers if hasattr(build_appliance, "providers") else [
            build_appliance[LayerInfo],
            build_appliance[DefaultInfo],
        ]

    if flavor:
        sub_targets["flavor"] = flavor.providers

    all_features = features[FeatureInfo].features
    dnf_available_repos = []
    if types.is_list(ctx.attrs.dnf_available_repos):
        dnf_available_repos = ctx.attrs.dnf_available_repos
    elif ctx.attrs.dnf_available_repos != None:
        dnf_available_repos = list(ctx.attrs.dnf_available_repos[RepoSetInfo].repos)
    else:
        dnf_available_repos = list(flavor_info.dnf_info.default_repo_set[RepoSetInfo].repos)

    dnf_additional_repos = ctx.attrs.dnf_additional_repos or []
    if not types.is_list(dnf_additional_repos):
        dnf_additional_repos = [dnf_additional_repos]

    dnf_additional_repos = dnf_additional_repos + ctx.attrs._dnf_auto_additional_repos

    for repo in dnf_additional_repos:
        if types.is_string(repo):
            extra_repo = _extra_repo_name_to_repo(repo, flavor_info)
            if extra_repo != None:
                dnf_available_repos.append(extra_repo)
        elif RepoSetInfo in repo:
            dnf_available_repos.extend(repo[RepoSetInfo].repos)
        elif RepoInfo in repo:
            dnf_available_repos.append(repo)
        else:
            fail("Unknown type for repo {} in dnf_additional_repos: ".format(repo))
    dnf_repodatas = ctx.actions.anon_target(repodata_only_local_repos, {
        "repos": dnf_available_repos,
    }).artifact("repodatas")
    dnf_versionlock = ctx.attrs.dnf_versionlock or flavor_info.dnf_info.default_versionlock

    dnf_excluded_rpms = list(ctx.attrs.dnf_excluded_rpms) if ctx.attrs.dnf_excluded_rpms != None else list(flavor_info.dnf_info.default_excluded_rpms)

    # rpmsign is missing a dependency: /usr/lib64/libtss2-rc.so.0
    # (P557719932). This failure occurss because tpm2-tss provides
    # /usr/lib64/libtss2-rc.so.0, but aziot-identity-service contains
    # /usr/lib64/aziot-identity-service/libtss2-rc.so.0 and dnf will happily
    # install that to satisfy the rpmsign dependency, even though it doesn't
    # actually do that. Since aziot-identity-service isn't actually used
    # anywhere, just exclude it
    if "aziot-identity-service" not in dnf_excluded_rpms:
        dnf_excluded_rpms.append("aziot-identity-service")

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
    final_facts_db = None
    debug_sub_targets = {}

    # Dirty hack to provide pre-computed dnf repos to multiple phases that
    # comprise chef-solo image builds. Chef is full of things that require dirty
    # hacks, and it's not worth the effort to make this gross thing be fully
    # supported in a non-hacky manner
    chef_plan_results = {}

    for phase in BuildPhase.values():
        phase = BuildPhase(phase)
        build_cmd = []
        logs = {}

        identifier_prefix = _identifier_prefix(phase.value + "_")

        # Cross-cell enum type comparisons can fail, so compare .value
        verify_build_phases([i.analysis.build_phase for i in all_features])
        features = [
            feat
            for feat in all_features
            if feat.analysis.build_phase.value == phase.value
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
                "plugin": f.plugin,
            } for f in features],
            with_inputs = True,
        )

        # deps that are needed for compiling the features, but not for depgraph
        # analysis, so are not included in `features_json`
        compile_feature_hidden_deps = [
            [feat.analysis.required_artifacts for feat in features],
            [feat.analysis.required_run_infos for feat in features],
        ]

        depgraph_input = build_depgraph(
            ctx = ctx,
            features_json = features_json,
            identifier_prefix = identifier_prefix,
            parent_depgraph = parent_depgraph,
            subvol = None,
            rootless = ctx.attrs._rootless,
        )

        target_arch = ctx.attrs._selected_target_arch

        compileish_args = cmd_args(
            cmd_args(target_arch, format = "--target-arch={}"),
            cmd_args(depgraph_input, format = "--depgraph-json={}"),
        )

        if lazy.any(lambda feat: feat.analysis.requires_planning, features):
            plan = ctx.actions.declare_output(identifier_prefix + "plan.json")
            logs["plan"] = ctx.actions.declare_output(identifier_prefix + "plan.log")
            cmd, _ = _map_image(
                build_appliance = build_appliance[LayerInfo],
                cmd = cmd_args(
                    cmd_args(dnf_repodatas, format = "--dnf-repos={}"),
                    cmd_args(dnf_versionlock, format = "--dnf-versionlock={}") if dnf_versionlock else cmd_args(),
                    cmd_args(
                        json.encode(ctx.attrs.dnf_versionlock_extend),
                        format = "--dnf-versionlock-extend={}",
                    ),
                    cmd_args(dnf_excluded_rpms, format = "--dnf-excluded-rpms={}") if dnf_excluded_rpms else cmd_args(),
                    "plan",
                    compileish_args,
                    cmd_args(plan.as_output(), format = "--plan={}"),
                ).hidden(features_json, compile_feature_hidden_deps),
                ctx = ctx,
                identifier = identifier_prefix + "plan",
                parent = parent_layer,
                logs = logs["plan"],
                rootless = ctx.attrs._rootless,
            )
            build_cmd.append(cmd)

            # Part of the compiler plan is any possible dnf transaction resolution,
            # which lets us know what rpms we will need. We can have buck download them
            # and mount in a pre-built directory of all repositories for
            # completely-offline dnf installation (which is MUCH faster and more
            # reliable)
            # TODO(T179081948): this should also be an anon_target
            dnf_repos_dir = compiler_plan_to_local_repos(
                ctx,
                identifier_prefix,
                [r[RepoInfo] for r in dnf_available_repos],
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
        elif phase == BuildPhase("chef"):
            plan = None
            dnf_repos_dir = chef_plan_results["dnf_repos_dir"]
            resolved_fbpkgs_json = chef_plan_results["resolved_fbpkgs_json"]
            resolved_fbpkgs_dir = chef_plan_results["resolved_fbpkgs_dir"]
        else:
            plan = None
            dnf_repos_dir = ctx.actions.symlinked_dir(identifier_prefix + "empty-dnf-repos", {})
            resolved_fbpkgs_json = None
            resolved_fbpkgs_dir = None

        if phase == BuildPhase("chef_package_manager"):
            chef_plan_results["dnf_repos_dir"] = dnf_repos_dir
            chef_plan_results["resolved_fbpkgs_json"] = resolved_fbpkgs_json
            chef_plan_results["resolved_fbpkgs_dir"] = resolved_fbpkgs_dir

        logs["compile"] = ctx.actions.declare_output(identifier_prefix + "compile.log")
        if resolved_fbpkgs_dir:
            compile_feature_hidden_deps.append(resolved_fbpkgs_dir)

        cmd, final_subvol = _map_image(
            build_appliance = build_appliance[LayerInfo],
            cmd = cmd_args(
                cmd_args(dnf_repos_dir, format = "--dnf-repos={}"),
                cmd_args(dnf_versionlock, format = "--dnf-versionlock={}") if dnf_versionlock else cmd_args(),
                cmd_args(
                    json.encode(ctx.attrs.dnf_versionlock_extend),
                    format = "--dnf-versionlock-extend={}",
                ),
                # @oss-disable
                "compile",
                compileish_args,
                cmd_args(plan, format = "--plan={}") if plan else cmd_args(),
            ).hidden(features_json, compile_feature_hidden_deps),
            ctx = ctx,
            identifier = identifier_prefix + "compile",
            parent = parent_layer,
            logs = logs["compile"],
            rootless = ctx.attrs._rootless,
        )
        build_cmd.append(cmd)

        final_depgraph = build_depgraph(
            ctx = ctx,
            features_json = None,
            identifier_prefix = identifier_prefix,
            parent_depgraph = depgraph_input,
            subvol = final_subvol,
            rootless = ctx.attrs._rootless,
        )

        final_facts_db = facts.new_facts_db(
            actions = ctx.actions,
            subvol_symlink = final_subvol,
            build_appliance = build_appliance[LayerInfo],
            new_facts_db = ctx.attrs._new_facts_db[RunInfo],
            phase = phase,
            rootless = ctx.attrs._rootless,
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
                    "container": _container_sub_target(ctx.attrs._run_container, final_subvol, mounts = phase_mounts),
                    "facts": [DefaultInfo(final_facts_db)],
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
    if not final_depgraph:
        final_depgraph = build_depgraph(
            ctx = ctx,
            features_json = None,
            identifier_prefix = "empty_layer_",
            parent_depgraph = parent_depgraph,
            subvol = final_subvol,
            rootless = ctx.attrs._rootless,
        )
    if not final_facts_db:
        final_facts_db = facts.new_facts_db(
            actions = ctx.actions,
            subvol_symlink = final_subvol,
            build_appliance = build_appliance[LayerInfo],
            new_facts_db = ctx.attrs._new_facts_db[RunInfo],
            phase = None,
            rootless = False,
        )

    debug_sub_targets["depgraph"] = [DefaultInfo(final_depgraph)]

    debug_sub_targets["facts"] = [DefaultInfo(final_facts_db)]

    sub_targets["subvol_symlink"] = [DefaultInfo(final_subvol)]

    parent_layer_info = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None
    mounts = all_mounts(features = all_features, parent_layer = parent_layer_info)
    # @oss-disable

    sub_targets["container"] = _container_sub_target(ctx.attrs._run_container, final_subvol, mounts)
    sub_targets["debug"] = [DefaultInfo(sub_targets = debug_sub_targets)]

    providers = [
        LayerInfo(
            build_appliance = build_appliance,
            depgraph = final_depgraph,
            facts_db = final_facts_db,
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

    if ctx.attrs.default_mountpoint:
        providers.append(DefaultMountpointInfo(default_mountpoint = ctx.attrs.default_mountpoint))

    return providers

_layer_attrs = {
    "antlir2": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2:antlir2")),
    "antlir_internal_build_appliance": attrs.bool(default = False, doc = "mark if this image is a build appliance and is allowed to not have a flavor"),
    "build_appliance": attrs.option(
        # attrs.transition_dep(providers = [LayerInfo], cfg = remove_os_constraint),
        attrs.dep(providers = [LayerInfo]),
        default = None,
    ),
    "default_mountpoint": attrs.option(attrs.string(), default = None),
    "dnf_additional_repos": attrs.option(
        attrs.one_of(
            attrs.list(attrs.dep(providers = [RepoInfo])),
            attrs.dep(providers = [RepoSetInfo]),
            attrs.list(attrs.string()),
        ),
        default = None,
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
    "dnf_versionlock_extend": attrs.dict(
        attrs.string(doc = "rpm name"),
        attrs.string(doc = "rpm evra"),
        default = {},
    ),
    "labels": attrs.list(attrs.string(), default = []),
    "parent_layer": attrs.option(
        attrs.dep(providers = [LayerInfo]),
        default = None,
    ),
    "_dnf_auto_additional_repos": attrs.list(
        attrs.one_of(
            attrs.dep(providers = [RepoInfo]),
            attrs.dep(providers = [RepoSetInfo]),
        ),
        # the true default is populated at the macro level
        default = [],
        doc = """
            Equivalent to 'dnf_additional_repos' but selected only by internal
            configurations (like systemd-cd).
        """,
    ),
    "_implicit_image_test": attrs.option(
        attrs.exec_dep(providers = [ExternalRunnerTestInfo]),
        default = None,
    ),
    "_new_facts_db": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2_facts:new-facts-db")),
    "_run_container": attrs.option(attrs.exec_dep(), default = None),
    "_selected_target_arch": attrs.default_only(attrs.string(
        default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
        doc = "CPU arch that this layer is being built for. This is always " +
              "correct, while target_arch might or might not be set",
    )),
}

_layer_attrs.update(cfg_attrs())
_layer_attrs.update(attrs_selected_by_cfg())

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
    cfg = layer_cfg,
)

def layer(
        *,
        name: str,
        # Features does not have a direct type hint, but it is still validated
        # by a type hint inside feature.bzl. Feature targets or
        # InlineFeatureInfo providers are accepted, at any level of nesting
        features = [],
        default_os: str | None = None,
        # TODO: remove this flag when all images are using this new mechanism
        use_default_os_from_package: bool | None = None,
        default_rou: str | None = None,
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
        if is_facebook:
            default_rou = compat.default_rou_from_antlir1_flavor(flavor) if flavor else None

    if use_default_os_from_package == None:
        use_default_os_from_package = should_all_images_in_package_use_default_os()
    if use_default_os_from_package:
        default_os = default_os or get_default_os_for_package()

    # TODO(vmagro): codemod existing callsites to use default_os directly
    if kwargs.get("flavor", None) and default_os:
        fail("default_os= is preferred, stop setting flavor=")
    if kwargs.get("flavor", None) and not default_rou:
        default_rou = compat.default_rou_from_antlir1_flavor(kwargs["flavor"])

    kwargs.update({"_feature_" + key: val for key, val in feature_attrs(features).items()})

    target_compatible_with = kwargs.pop("target_compatible_with", []) or []
    target_compatible_with.extend(kwargs.pop("_feature_target_compatible_with", []))
    if target_compatible_with:
        kwargs["target_compatible_with"] = target_compatible_with

    if is_facebook:
        # available_fbpkgs is logically an optional dep, but to make it truly
        # optional for using layers in anon_targets, we must just set the
        # default at the macro layer
        # TODO(vmagro): the best fix is to introduce this dep only on the
        # chef_solo feature itself, but that's trickier to accomplish
        kwargs.setdefault(
            "available_fbpkgs",
            "fbcode//bot_generated/antlir/fbpkg/db/main_db/.buck:snapshotted_fbpkgs",
        )

        # Likewise here, set it as a default in the macro layer so that it
        # doesn't need to be set for anon layers
        kwargs.setdefault(
            "_dnf_auto_additional_repos",
            fb_defaults["_dnf_auto_additional_repos"],
        )

    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    return layer_rule(
        name = name,
        default_os = default_os,
        # @oss-disable
        visibility = get_visibility(visibility),
        _implicit_image_test = antlir2_dep("//antlir/antlir2/testing/implicit_image_test:implicit_image_test"),
        _run_container = antlir2_dep("//antlir/antlir2/container_subtarget:run"),
        **kwargs
    )
