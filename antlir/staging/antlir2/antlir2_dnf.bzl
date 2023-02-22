# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:flatten.bzl", "flatten")
load("//antlir/rpm/dnf2buck:rpm.bzl", "nevra_to_string", "package_href")

def repodata_only_local_repos(ctx: "context", available_rpm_repos: ["RepoSetInfo", None]) -> "artifact":
    """
    Produce a directory that contains a local copy of the available RPM repo's
    repodata directories.
    This directory is used during dnf resolution while forming the compiler
    plan, so it's ok that the Packages/ directory will be missing.
    """
    dir = ctx.actions.declare_output("dnf_repodatas", dir = True)

    tree = {
        paths.join(repo_info.id, "repodata"): repo_info.repodata
        for repo_info in (available_rpm_repos.repo_infos if available_rpm_repos else [])
    }

    # copied_dir instead of symlink_dir so that this can be directly bind
    # mounted into the container
    ctx.actions.copied_dir(dir, tree)
    return dir

def compiler_plan_to_local_repos(
        ctx: "context",
        available_rpm_repos: ["RepoSetInfo", None],
        compiler_plan: "artifact") -> "artifact":
    """
    Use the compiler plan to build a directory of all the RPM repodata and RPM
    blobs we need to perform the dnf installations in the image.
    """
    dir = ctx.actions.declare_output("dnf_repos", dir = True)

    # collect all rpms keyed by repo, then nevra
    by_repo = {}
    for repo_info in (available_rpm_repos.repo_infos if available_rpm_repos else []):
        by_repo[repo_info.id] = {"nevras": {}, "repo_info": repo_info}
        for rpm_info in repo_info.all_rpms:
            by_repo[repo_info.id]["nevras"][nevra_to_string(rpm_info.nevra)] = rpm_info

    def _dyn(ctx, artifacts, outputs, plan = compiler_plan, by_repo = by_repo, dir = dir):
        plan = artifacts[plan].read_json()
        tx = plan.get("dnf_transaction", {"install": []})
        tree = {}
        for install in tx["install"]:
            if install["nevra"] not in by_repo[install["repo"]]["nevras"]:
                # This is impossible because the dnf transaction resolution will
                # fail before we even get to this, but format a nice warning
                # anyway
                fail("'{}' is not in {}".format(install["nevra"], install["repo"]))

            repo_i = by_repo[install["repo"]]["repo_info"]
            rpm_i = by_repo[install["repo"]]["nevras"][install["nevra"]]
            tree[paths.join(repo_i.id, "repodata")] = repo_i.repodata
            tree[paths.join(repo_i.id, package_href(install["nevra"], rpm_i.pkgid))] = rpm_i.rpm

        # copied_dir instead of symlink_dir so that this can be directly bind
        # mounted into the container
        ctx.actions.copied_dir(outputs[dir], tree)

    ctx.actions.dynamic_output(
        # the dynamic action reads this
        dynamic = [compiler_plan],
        # to determine which dependencies it needs out of the set of every
        # available rpm
        inputs = flatten.flatten([[rpm_info.rpm for rpm_info in repo["nevras"].values()] for repo in by_repo.values()]),
        # to produce this, a directory that contains a (partial, but complete
        # for the transaction) copy of the repos needed to do the installation
        outputs = [dir],
        f = _dyn,
    )
    return dir
