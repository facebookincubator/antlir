# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "RepoInfo")
load(
    "//antlir/antlir2/package_managers/dnf/rules:rpm.bzl",
    "RpmInfo",  # @unused Used as type
    "nevra_to_string",
    "package_href",
)

LocalReposInfo = provider(fields = {
    "repos_dir": Artifact,
})

def _repodata_only_local_repos_impl(ctx: AnalysisContext) -> list[Provider]:
    """
    Produce a directory that contains a local copy of the available RPM repo's
    repodata directories.
    This directory is used during dnf resolution while forming the compiler
    plan, so it's ok that the Packages/ directory will be missing.
    """

    tree = {}
    for repo in ctx.attrs.repos:
        repo_info = repo[RepoInfo]
        tree[paths.join(repo_info.id, "repodata")] = repo_info.repodata
        for key in repo_info.gpg_keys:
            tree[paths.join(repo_info.id, "gpg-keys", key.basename)] = key
        tree[paths.join(repo_info.id, "dnf_conf.json")] = repo_info.dnf_conf_json

    # copied_dir instead of symlink_dir so that this can be directly bind
    # mounted into the container
    repos_dir = ctx.actions.copied_dir("repodatas", tree)
    return [
        DefaultInfo(repos_dir),
        LocalReposInfo(repos_dir = repos_dir),
    ]

repodata_only_local_repos = anon_rule(
    impl = _repodata_only_local_repos_impl,
    attrs = {
        "repos": attrs.list(attrs.dep(providers = [RepoInfo])),
    },
    artifact_promise_mappings = {
        "repodatas": lambda x: x[LocalReposInfo].repos_dir,
    },
)

def _best_rpm_artifact(
        *,
        rpm_info: RpmInfo | Provider,
        reflink_flavor: str | None) -> Artifact:
    if not reflink_flavor:
        return rpm_info.raw_rpm
    else:
        # The default behavior is to fail the build if the flavor is reflinkable
        # and the rpm does not have any reflinkable artifacts. This is a safety
        # mechanism to ensure we don't silently regress rpm reflink support. If
        # that regressed, installations would still succeed but be orders of
        # magnitude slower, so instead we want to scream very loudly.
        if reflink_flavor not in rpm_info.extents:
            fail("{} does not have a reflinkable artifact for {}".format(rpm_info.nevra, reflink_flavor))
        return rpm_info.extents[reflink_flavor]

def _possible_rpm_artifacts(*, rpm_info: RpmInfo | Provider, reflink_flavor: str | None) -> list[Artifact]:
    artifacts = [rpm_info.raw_rpm]
    if reflink_flavor and reflink_flavor in rpm_info.extents:
        artifacts.append(rpm_info.extents[reflink_flavor])
    return artifacts

def compiler_plan_to_local_repos(
        ctx: AnalysisContext,
        identifier_prefix: str,
        dnf_available_repos: list[RepoInfo | Provider],
        compiler_plan: Artifact,
        reflink_flavor: str | None) -> Artifact:
    """
    Use the compiler plan to build a directory of all the RPM repodata and RPM
    blobs we need to perform the dnf installations in the image.
    """
    dir = ctx.actions.declare_output(identifier_prefix + "dnf_repos", dir = True)

    # collect all rpms keyed by repo, then nevra
    by_repo = {}
    for repo_info in dnf_available_repos:
        by_repo[repo_info.id] = {"nevras": {}, "repo_info": repo_info}
        for rpm_info in repo_info.all_rpms:
            by_repo[repo_info.id]["nevras"][nevra_to_string(rpm_info.nevra)] = rpm_info

    def _dyn(ctx, artifacts, outputs, plan = compiler_plan, by_repo = by_repo, dir = dir):
        plan = artifacts[plan].read_json()
        tx = plan.get("dnf_transaction", {"install": []})
        tree = {}

        # all repodata is made available even if there are no rpms being
        # installed from that repository, because of certain things *cough* chef
        # *cough* that directly query dnf to make runtime decisions, and having
        # only the necessary set of repositories cause it to make different,
        # stupid, decisions
        for repo in by_repo.values():
            repo_i = repo["repo_info"]
            tree[paths.join(repo_i.id, "repodata")] = repo_i.repodata
            for key in repo_i.gpg_keys:
                tree[paths.join(repo_i.id, "gpg-keys", key.basename)] = key
            tree[paths.join(repo_i.id, "dnf_conf.json")] = repo_i.dnf_conf_json

        for install in tx["install"]:
            found = False

            # If this rpm is being installed from a local file and not a repo,
            # skip this materialize-into-a-repo logic
            if install["repo"] == None:
                continue

            # The same exact NEVRA may appear in multiple repositories, and then
            # we have no guarantee that dnf will resolve the transaction the
            # same way, so we must look in every repo in addition to the one
            # that was initially recorded
            for repo in by_repo.values():
                if install["nevra"] in repo["nevras"]:
                    repo_i = repo["repo_info"]
                    rpm_i = repo["nevras"][install["nevra"]]
                    tree[paths.join(repo_i.id, package_href(install["nevra"], rpm_i.pkgid))] = _best_rpm_artifact(
                        rpm_info = rpm_i,
                        reflink_flavor = reflink_flavor,
                    )
                    found = True

            if not found:
                # This should be impossible (but through dnf, all things are
                # possible so jot that down) because the dnf transaction
                # resolution will fail before we even get to this, but format a
                # nice warning anyway.
                fail("'{}' does not appear in any repos".format(install["nevra"]))

        # copied_dir instead of symlink_dir so that this can be directly bind
        # mounted into the container
        ctx.actions.copied_dir(outputs[dir], tree)

    # All rpm artifacts are made available to the dynamic output computation. We
    # can't yet know whether or not rpmcow will be availalbe so must provide all
    # variants of the rpm artifact, but the dynamic output will still use the
    # most efficient possible.
    inputs = []
    for repo in by_repo.values():
        for rpm_info in repo["nevras"].values():
            inputs.extend(_possible_rpm_artifacts(
                rpm_info = rpm_info,
                reflink_flavor = reflink_flavor,
            ))

    ctx.actions.dynamic_output(
        # the dynamic action reads this
        dynamic = [compiler_plan],
        inputs = inputs,
        # to produce this, a directory that contains a (partial, but complete
        # for the transaction) copy of the repos needed to do the installation
        outputs = [dir],
        f = _dyn,
    )
    return dir
