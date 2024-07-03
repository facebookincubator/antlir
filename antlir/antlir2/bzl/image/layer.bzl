# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:expect.bzl", "expect")
load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/antlir2_error_handler:handler.bzl", "antlir2_error_handler")
load("//antlir/antlir2/antlir2_overlayfs:overlayfs.bzl", "OverlayFs", "OverlayLayer", "get_antlir2_use_overlayfs")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase", "verify_build_phases")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "FeatureInfo", "FlavorInfo", "LayerContents", "LayerInfo")
load("//antlir/antlir2/bzl/feature:feature.bzl", "feature_attrs", "feature_rule", "reduce_features", "shared_features_attrs")

load("//antlir/bzl:oss_shim.bzl", all_fbpkg_mounts = "ret_empty_list") # @oss-enable
# @oss-disable

load("//antlir/bzl:oss_shim.bzl", fb_defaults = "empty_dict") # @oss-enable
# @oss-disable
load(
    "//antlir/antlir2/features/mount:mount.bzl",
    "DefaultMountpointInfo",
)
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package", "should_all_images_in_package_use_default_os")
# @oss-disable
load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "RepoInfo", "RepoSetInfo")
load("//antlir/bzl:build_defs.bzl", "is_facebook")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:types.bzl", "types")
load("//antlir/bzl/build_defs.bzl", "config", "get_visibility")
load(":cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load(":depgraph.bzl", "build_depgraph")
load(":facts.bzl", "facts")
load(":mount_types.bzl", "mount_record")  # @unused Used as type
load(
    ":mounts.bzl",
    "all_mounts",
    "container_mount_args",
)

def _compile(
        *,
        ctx: AnalysisContext,
        identifier: str,
        parent: LayerContents | None,
        logs: OutputArtifact,
        rootless: bool,
        target_arch: str,
        topo_features: Artifact,
        plans: typing.Any,
        hidden_deps: typing.Any) -> LayerContents:
    """
    Compile features into a new image layer
    """
    antlir2 = ctx.attrs.antlir2[RunInfo]
    if ctx.attrs._overlayfs:
        parent_layers = (parent.overlayfs.layers + [parent.overlayfs.top]) if parent else []
        overlayfs_model_out = ctx.actions.declare_output(identifier, "overlayfs-out.json")
        data_dir = ctx.actions.declare_output(identifier, "overlayfs-data-dir", dir = True)
        manifest = ctx.actions.declare_output(identifier, "overlayfs-manifest.json")
        overlayfs_model_out = ctx.actions.write_json(overlayfs_model_out, struct(
            top = OverlayLayer(
                data_dir = data_dir.as_output(),
                manifest = manifest.as_output(),
            ),
            layers = parent_layers,
        ), with_inputs = True)
        overlayfs_model = ctx.actions.declare_output(identifier, "overlayfs.json")
        overlayfs_model_with_inputs = ctx.actions.write_json(overlayfs_model, struct(
            top = OverlayLayer(
                data_dir = data_dir,
                manifest = manifest,
            ),
            layers = parent_layers,
        ), with_inputs = True)
        overlayfs = OverlayFs(
            top = OverlayLayer(
                data_dir = data_dir,
                manifest = manifest,
            ),
            layers = parent_layers,
            json_file_with_inputs = overlayfs_model_with_inputs,
            json_file = overlayfs_model,
        )
        subvol_symlink = None
    else:
        overlayfs_model_out = None
        overlayfs = None
        subvol_symlink = ctx.actions.declare_output(identifier, "subvol_symlink")
    ctx.actions.run(
        cmd_args(
            cmd_args("sudo") if not rootless else cmd_args(),
            antlir2,
            cmd_args(logs, format = "--logs={}"),
            "compile",
            "--working-dir=antlir2-out",
            cmd_args(str(ctx.label), format = "--label={}"),
            cmd_args(parent.subvol_symlink, format = "--parent={}") if parent and not ctx.attrs._overlayfs else cmd_args(),
            cmd_args(
                subvol_symlink.as_output() if not ctx.attrs._overlayfs else overlayfs_model_out,
                format = "--output={}",
            ),
            cmd_args("--rootless") if rootless else cmd_args(),
            cmd_args(target_arch, format = "--target-arch={}"),
            cmd_args(topo_features, format = "--features={}"),
            cmd_args(plans, format = "--plans={}"),
            cmd_args("--working-format=overlayfs") if ctx.attrs._overlayfs else cmd_args(),
            hidden = hidden_deps,
        ),
        category = "antlir2",
        env = {
            "RUST_LOG": "antlir2=trace",
        },
        identifier = identifier,
        # needs local subvolumes
        local_only = not ctx.attrs._overlayfs,
        # the old output is used to clean up the local subvolume
        no_outputs_cleanup = not ctx.attrs._overlayfs,
        error_handler = antlir2_error_handler,
    )

    return LayerContents(
        overlayfs = overlayfs,
        subvol_symlink = subvol_symlink,
    )

def _container_sub_target(
        binary: Dependency | None,
        layer: LayerContents,
        mounts: list[mount_record],
        rootless: bool) -> list[Provider]:
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
            "sudo" if not rootless else cmd_args(),
            binary[RunInfo],
            "--rootless" if rootless else cmd_args(),
            cmd_args(layer.subvol_symlink, format = "--subvol={}"),
            cmd_args([container_mount_args(mount) for mount in mounts]),
            dev_mode_args,
        )),
    ]

def _implicit_image_test(layer: LayerContents, implicit_image_test: ExternalRunnerTestInfo) -> ExternalRunnerTestInfo:
    implicit_image_test = ExternalRunnerTestInfo(
        type = implicit_image_test.test_type,
        command = implicit_image_test.command,
        env = (implicit_image_test.env or {}) | {
            "ANTLIR2_LAYER": (layer.overlayfs.json_file_with_inputs if layer.overlayfs else None) or layer.subvol_symlink,
        },
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
                ctx.attrs.flavor[FlavorInfo].label.raw_target() == parent_flavor[FlavorInfo].label.raw_target(),
                "flavor ({}) was different from parent_layer's flavor ({})",
                ctx.attrs.flavor[FlavorInfo].label.raw_target(),
                parent_flavor[FlavorInfo].label.raw_target(),
            )
    if ctx.attrs.parent_layer:
        flavor = ctx.attrs.parent_layer[LayerInfo].flavor
    if not flavor:
        flavor = ctx.attrs.flavor
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

    for logical_id in ctx.attrs.dnf_exclude_repos:
        to_remove = None
        for repo in dnf_available_repos:
            if repo[RepoInfo].logical_id == logical_id:
                to_remove = repo
        if not to_remove:
            fail("Logical id '{}' does not match any repo ({}), remove it".format(
                logical_id,
                [r[RepoInfo].logical_id for r in dnf_available_repos],
            ))
        dnf_available_repos.remove(to_remove)

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

    if ctx.attrs._overlayfs and not ctx.attrs._rootless:
        fail("overlayfs is only supported with rootless")

    layer = ctx.attrs.parent_layer[LayerInfo].contents if ctx.attrs.parent_layer else None
    facts_db = ctx.attrs.parent_layer[LayerInfo].facts_db if ctx.attrs.parent_layer else None
    debug_sub_targets = {}

    # See Planner.previous_phase_plans for rationale
    previous_phase_plans = {}

    for phase in BuildPhase.values():
        phase = BuildPhase(phase)
        logs = {}
        phase_sub_targets = {}

        identifier = phase.value

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
        if not features and not (phase == BuildPhase("compile") and layer == None):
            continue

        # Some feature types must be reduced to one instance per phase (eg
        # package managers)
        features = reduce_features(features)

        # facts_db also holds the depgraph
        facts_db, topo_features = build_depgraph(
            ctx = ctx,
            features = features,
            identifier = identifier,
            parent = facts_db,
            phase = phase,
        )
        phase_sub_targets["depgraph"] = [DefaultInfo(facts_db)]
        phase_sub_targets["topo_features.json"] = [DefaultInfo(topo_features)]

        target_arch = ctx.attrs._selected_target_arch

        # All deps that are needed for *compiling* the features (but not
        # depgraph analysis)
        compile_feature_hidden_deps = [
            [feat.analysis.required_artifacts for feat in features],
            [feat.analysis.required_run_infos for feat in features],
            [feat.plugin.plugin for feat in features],
            [feat.plugin.libs for feat in features],
        ]

        # Cover all the other inputs needed for compiling a feature by writing
        # it to a json file. This is just an easy way to just traverse the
        # structure to find any artifacts, but this json file is not directly
        # read anywhere
        compile_feature_hidden_deps.append(
            ctx.actions.write_json(
                ctx.actions.declare_output(identifier, "features.json"),
                [f.analysis.data for f in features],
                with_inputs = True,
            ),
        )

        plans = {}
        plan_sub_targets = {}
        for feature in features:
            planner = feature.analysis.planner
            if planner:
                kwargs = {}
                if planner.label:
                    kwargs["label"] = ctx.label
                if planner.flavor:
                    kwargs["flavor"] = flavor_info
                if planner.build_appliance:
                    kwargs["build_appliance"] = build_appliance[BuildApplianceInfo]
                if planner.target_arch:
                    kwargs["target_arch"] = target_arch
                if planner.parent_layer_contents:
                    kwargs["parent_layer_contents"] = layer
                if planner.dnf:
                    kwargs |= {
                        "dnf_available_repos": dnf_available_repos,
                        "dnf_excluded_rpms": dnf_excluded_rpms,
                        "dnf_versionlock": dnf_versionlock,
                        "dnf_versionlock_extend": ctx.attrs.dnf_versionlock_extend,
                    }
                for id in planner.previous_phase_plans:
                    if id not in previous_phase_plans:
                        fail("previous_phase_plan '{}' does not exist".format(id))
                    kwargs["previous_phase_plan_{}".format(id)] = previous_phase_plans[id]

                plan_infos = planner.fn(
                    ctx = ctx,
                    identifier = identifier,
                    rootless = ctx.attrs._rootless,
                    feature = feature,
                    **(kwargs | planner.kwargs)
                )
                for pi in plan_infos:
                    if pi.id in plans:
                        fail("plan ids should be unique, but got '{}' multiple times".format(pi.id))
                    plans[pi.id] = pi
                    compile_feature_hidden_deps.append(pi.hidden)
                    if pi.sub_artifacts:
                        plan_sub_targets[pi.id] = [DefaultInfo(sub_targets = {
                            key: [DefaultInfo(artifact)]
                            for key, artifact in pi.sub_artifacts.items()
                        })]
        previous_phase_plans = plans

        phase_sub_targets["plan"] = [DefaultInfo(sub_targets = plan_sub_targets)]

        plans = ctx.actions.write_json(
            ctx.actions.declare_output(identifier, "plans.json"),
            {id: pi.output for id, pi in plans.items()},
            with_inputs = True,
        )

        logs["compile"] = ctx.actions.declare_output(identifier, "compile.log")
        layer = _compile(
            ctx = ctx,
            identifier = identifier,
            parent = layer,
            logs = logs["compile"].as_output(),
            rootless = ctx.attrs._rootless,
            target_arch = ctx.attrs._selected_target_arch,
            topo_features = topo_features,
            plans = plans,
            hidden_deps = compile_feature_hidden_deps,
        )

        facts_db = facts.new_facts_db(
            actions = ctx.actions,
            parent_facts_db = facts_db,
            layer = layer,
            build_appliance = build_appliance[BuildApplianceInfo],
            new_facts_db = ctx.attrs._new_facts_db[RunInfo],
            phase = phase,
            rootless = ctx.attrs._rootless,
        )

        all_logs = ctx.actions.declare_output(identifier, "logs", dir = True)
        ctx.actions.symlinked_dir(all_logs, {key + ".log": artifact for key, artifact in logs.items()})
        if layer.subvol_symlink:
            phase_sub_targets["subvol_symlink"] = [DefaultInfo(layer.subvol_symlink)]
            phase_sub_targets["container"] = _container_sub_target(
                ctx.attrs._run_container,
                layer,
                mounts = all_mounts(
                    features = features,
                    parent_layer = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None,
                ),
                rootless = ctx.attrs._rootless,
            )
        if layer.overlayfs:
            phase_sub_targets["overlayfs"] = [DefaultInfo(layer.overlayfs.json_file)]
            # TODO: support [container] for overlayfs backed layers

        debug_sub_targets[phase.value] = [
            DefaultInfo(
                sub_targets = phase_sub_targets | {
                    "facts": [DefaultInfo(facts_db)],
                    "logs": [DefaultInfo(all_logs, sub_targets = {
                        key: [DefaultInfo(artifact)]
                        for key, artifact in logs.items()
                    })],
                },
            ),
        ]

    debug_sub_targets["facts"] = [DefaultInfo(facts_db)]

    parent_layer_info = ctx.attrs.parent_layer[LayerInfo] if ctx.attrs.parent_layer else None
    mounts = all_mounts(features = all_features, parent_layer = parent_layer_info)
    # @oss-disable

    sub_targets["debug"] = [DefaultInfo(sub_targets = debug_sub_targets)]

    if layer.subvol_symlink:
        subvol_symlink = layer.subvol_symlink
        sub_targets["container"] = _container_sub_target(ctx.attrs._run_container, layer, mounts, ctx.attrs._rootless)
    elif ctx.attrs._materialize_to_subvol:
        subvol_symlink = ctx.actions.declare_output("subvol_symlink")
        ctx.actions.run(
            cmd_args(
                ctx.attrs._materialize_to_subvol[RunInfo],
                cmd_args(layer.overlayfs.json_file_with_inputs, format = "--model={}"),
                cmd_args(subvol_symlink.as_output(), format = "--subvol-symlink={}"),
            ),
            category = "materialize_to_subvol",
            local_only = True,  # deals with local subvolumes
        )
        sub_targets["subvol_symlink"] = [DefaultInfo(subvol_symlink)]
        sub_targets["container"] = _container_sub_target(
            ctx.attrs._run_container,
            LayerContents(subvol_symlink = subvol_symlink),
            mounts,
            ctx.attrs._rootless,
        )
    else:
        # This won't happen until we migrate more complex targets, since this
        # will only affect anon layers
        fail("RE builds must be provided with _materialize_to_subvol")

    sub_targets["subvol_symlink"] = [DefaultInfo(subvol_symlink)]

    providers = [
        DefaultInfo(
            subvol_symlink,
            sub_targets = sub_targets,
        ),
        LayerInfo(
            build_appliance = build_appliance,
            facts_db = facts_db,
            flavor = flavor,
            flavor_info = flavor_info,
            label = ctx.label,
            mounts = mounts,
            parent = ctx.attrs.parent_layer,
            features = all_features,
            contents = layer,
            subvol_symlink = subvol_symlink,
        ),
    ]

    if ctx.attrs._implicit_image_test:
        providers.append(
            _implicit_image_test(layer, ctx.attrs._implicit_image_test[ExternalRunnerTestInfo]),
        )

    if ctx.attrs.default_mountpoint:
        providers.append(DefaultMountpointInfo(default_mountpoint = ctx.attrs.default_mountpoint))

    return providers

_layer_attrs = {
    "antlir2": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2:antlir2")),
    "build_appliance": attrs.option(
        attrs.exec_dep(providers = [BuildApplianceInfo]),
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
    "dnf_exclude_repos": attrs.list(
        attrs.string(doc = "RepoInfo logical_id to exclude from the otherwise available repos"),
        default = [],
        doc = """
            Hide some repos from dnf resolution
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
    "_analyze_feature": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2_depgraph_if:analyze")),
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
    "_materialize_to_subvol": attrs.option(attrs.exec_dep(), default = None),
    "_new_facts_db": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2_facts:new-facts-db")),
    "_overlayfs": attrs.bool(default = False),
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
        rootless: bool | None = None,
        visibility: list[str] | None = None,
        **kwargs):
    """
    Create a new image layer

    Build a new image layer from the given `features` and `parent_layer`.
    """
    if use_default_os_from_package == None:
        use_default_os_from_package = should_all_images_in_package_use_default_os()
    if use_default_os_from_package:
        default_os = default_os or get_default_os_for_package()

    # TODO(vmagro): codemod existing callsites to use default_os directly
    if kwargs.get("flavor", None) and default_os:
        fail("default_os= is preferred, stop setting flavor=")

    force_flavor = kwargs.pop("force_flavor", None)
    if force_flavor:
        kwargs["flavor"] = force_flavor
        kwargs.pop("default_os", None)

    kwargs.update({"_feature_" + key: val for key, val in feature_attrs(features).items()})

    if is_facebook:
        # Set this as a default in the macro layer so that it doesn't need to be
        # set for anon layers
        kwargs.setdefault(
            "_dnf_auto_additional_repos",
            fb_defaults["_dnf_auto_additional_repos"],
        )

    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    if rootless == None:
        rootless = get_antlir2_rootless()

    if get_antlir2_use_overlayfs():
        kwargs["_overlayfs"] = True
        rootless = True

    if not rootless:
        kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: labels + ["uses_sudo"])

    return layer_rule(
        name = name,
        default_os = default_os,
        # @oss-disable
        rootless = rootless,
        visibility = get_visibility(visibility),
        _implicit_image_test = antlir2_dep("//antlir/antlir2/testing/implicit_image_test:implicit_image_test"),
        _run_container = antlir2_dep("//antlir/antlir2/container_subtarget:run"),
        _materialize_to_subvol = antlir2_dep("//antlir/antlir2/antlir2_overlayfs:materialize-to-subvol"),
        **kwargs
    )
