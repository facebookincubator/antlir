# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/antlir2/bzl:types.bzl",
    "BuildApplianceInfo",  # @unused Used as type
    "FlavorInfo",  # @unused Used as type
    "LayerContents",  # @unused Used as type
)
load("//antlir/antlir2/bzl/dnf:defs.bzl", "compiler_plan_to_local_repos", "repodata_only_local_repos")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "PlanInfo",
    "Planner",
    "feature_record",  # @unused Used as type
)
load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "RepoInfo")

def _plan_fn(
        *,
        ctx: AnalysisContext,
        identifier: str,
        feature: feature_record | typing.Any,
        dnf_available_repos: list[Dependency],
        **kwargs) -> list[PlanInfo]:
    items = ctx.actions.declare_output(identifier, "rpm/items.json")
    items = ctx.actions.write_json(items, feature.analysis.data.items, with_inputs = True)
    res = plan(
        ctx = ctx,
        identifier = identifier,
        items = items,
        dnf_available_repos = dnf_available_repos,
        **kwargs
    )
    return [plan_info(res)]

# Turn the result of the 'plan' function into a PlanInfo
def plan_info(res: struct) -> PlanInfo:
    return PlanInfo(
        id = "rpm",
        output = res.plan_json,
        hidden = res.hidden,
        sub_artifacts = {
            "repodatas": res.repodatas,
            "tx": res.tx_file,
        },
    )

def plan(
        *,
        ctx: AnalysisContext,
        identifier: str,
        rootless: bool,
        items: Artifact | typing.Any,
        label: Label,
        flavor: FlavorInfo | typing.Any,
        build_appliance: BuildApplianceInfo | typing.Any,
        parent_layer_contents: LayerContents | None,
        dnf_available_repos: list[Dependency],
        dnf_versionlock: Artifact | None,
        dnf_versionlock_extend: dict[str, str],
        dnf_excluded_rpms: list[str],
        target_arch: str,
        plan: Dependency) -> struct:
    tx = ctx.actions.declare_output(identifier, "rpm/transaction.json")

    dnf_repodatas = ctx.actions.anon_target(repodata_only_local_repos, {
        "repos": dnf_available_repos,
    }).artifact("repodatas")

    # Run without root if either explicitly configured to do so, or there is no
    # parent layer that we may need permission to read from
    rootless = rootless or not parent_layer_contents

    ctx.actions.run(
        cmd_args(
            "sudo" if not rootless else cmd_args(),
            plan[RunInfo],
            cmd_args(label, format = "--label={}"),
            "--rootless" if rootless else cmd_args(),
            cmd_args(parent_layer_contents.overlayfs.json_file_with_inputs, format = "--parent-overlayfs={}") if parent_layer_contents and parent_layer_contents.overlayfs else cmd_args(),
            cmd_args(parent_layer_contents.subvol_symlink, format = "--parent-subvol-symlink={}") if parent_layer_contents and parent_layer_contents.subvol_symlink else cmd_args(),
            cmd_args(build_appliance.dir, format = "--build-appliance={}"),
            cmd_args(dnf_repodatas, format = "--repodatas={}"),
            cmd_args(dnf_versionlock, format = "--versionlock={}") if dnf_versionlock else cmd_args(),
            cmd_args(json.encode(dnf_versionlock_extend), format = "--versionlock-extend={}"),
            cmd_args(dnf_excluded_rpms, format = "--exclude-rpm={}"),
            cmd_args(target_arch, format = "--target-arch={}"),
            cmd_args(items, format = "--items={}"),
            cmd_args(tx.as_output(), format = "--out={}"),
        ),
        category = "rpm_plan",
        identifier = identifier,
        # local_only if the parent is only available as a subvol
        local_only = bool(parent_layer_contents and not parent_layer_contents.overlayfs),
    )

    repos = compiler_plan_to_local_repos(
        ctx = ctx,
        identifier = identifier,
        dnf_available_repos = [r[RepoInfo] for r in dnf_available_repos],
        tx = tx,
        reflink_flavor = flavor.dnf_info.reflink_flavor,
    )

    plan_json = ctx.actions.declare_output(identifier, "rpm/plan.json")
    out = ctx.actions.write_json(
        plan_json,
        struct(
            tx_file = tx,
            build_appliance = build_appliance.dir,
            repos = repos,
            versionlock = dnf_versionlock,
            versionlock_extend = dnf_versionlock_extend,
            excluded_rpms = dnf_excluded_rpms,
        ),
        with_inputs = True,
    )

    return struct(
        repodatas = dnf_repodatas,
        repos = repos,
        plan_json = plan_json,
        hidden = [out],
        tx_file = tx,
    )

def rpm_planner(*, plan: Dependency) -> Planner:
    return Planner(
        fn = _plan_fn,
        parent_layer_contents = True,
        build_appliance = True,
        dnf = True,
        label = True,
        flavor = True,
        target_arch = True,
        kwargs = {
            "plan": plan,
        },
    )
