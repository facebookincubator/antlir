# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "RepoInfo")
load(
    "//antlir/antlir2/package_managers/dnf/rules:rpm.bzl",
    "RpmInfo",  # @unused Used as type
    "package_href",
)

LocalReposInfo = provider(
    fields = {
        "repos_dir": Artifact,
    }
)

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
    repos_dir = ctx.actions.copied_dir("repodatas", tree, has_content_based_path = False)
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

def _best_rpm_artifact(*, rpm_info: RpmInfo | Provider, reflink_flavor: str | None) -> Artifact:
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

def _compiler_plan_to_local_repos_impl(actions: AnalysisActions, tx: ArtifactValue, dnf_available_repos: list, reflink_flavor: str | None, dir: OutputArtifact):
    """
    Dynamic action implementation that reads the transaction file and builds
    the local repos directory.
    """
    tx_content = tx.read_json()
    tree = {}

    # collect all rpms keyed by repo, then nevra
    by_repo = {}
    for repo_info in dnf_available_repos:
        by_repo[repo_info.id] = {"nevras": {}, "repo_info": repo_info}
        for rpm_info in repo_info.all_rpms:
            by_repo[repo_info.id]["nevras"][rpm_info.nevra] = rpm_info

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

    for install in tx_content["install"]:
        found = False

        # If this rpm is being installed from a local file and not a repo,
        # skip this materialize-into-a-repo logic
        if install["repo"] == None:
            continue

        nevra = "{name}-{epoch}:{version}-{release}.{arch}".format(**install["package"])

        # The same exact NEVRA may appear in multiple repositories, and then
        # we have no guarantee that dnf will resolve the transaction the
        # same way, so we must look in every repo in addition to the one
        # that was initially recorded
        for repo in by_repo.values():
            if nevra in repo["nevras"]:
                repo_i = repo["repo_info"]
                rpm_i = repo["nevras"][nevra]
                tree[paths.join(repo_i.id, package_href(nevra, rpm_i.pkgid))] = _best_rpm_artifact(
                    rpm_info = rpm_i,
                    reflink_flavor = reflink_flavor,
                )
                found = True

        if not found:
            # This should be impossible (but through dnf, all things are
            # possible so jot that down) because the dnf transaction
            # resolution will fail before we even get to this, but format a
            # nice warning anyway.
            fail("'{}' does not appear in any repos".format(nevra))

    # copied_dir instead of symlink_dir so that this can be directly bind
    # mounted into the container
    actions.copied_dir(dir, tree)
    return []

_compiler_plan_to_local_repos_dynamic = dynamic_actions(
    impl = _compiler_plan_to_local_repos_impl,
    attrs = {
        "dir": dynattrs.output(),
        "dnf_available_repos": dynattrs.value(list),
        "reflink_flavor": dynattrs.value(str | None),
        "tx": dynattrs.artifact_value(),
    },
)

def compiler_plan_to_local_repos(
    *, ctx: AnalysisContext, identifier: str, dnf_available_repos: list[RepoInfo | Provider], tx: Artifact, reflink_flavor: str | None
) -> Artifact:
    """
    Use the planned dnf transaction to build a directory of all the RPM repodata
    and RPM blobs we need to perform the dnf installations in the image.
    """
    dir = ctx.actions.declare_output(identifier, "dnf_repos", dir = True, has_content_based_path = False)

    ctx.actions.dynamic_output_new(
        _compiler_plan_to_local_repos_dynamic(
            tx = tx,
            dnf_available_repos = dnf_available_repos,
            reflink_flavor = reflink_flavor,
            dir = dir.as_output(),
        ),
    )
    return dir
