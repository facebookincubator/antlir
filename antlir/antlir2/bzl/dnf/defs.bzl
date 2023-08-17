# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/rpm/dnf2buck:rpm.bzl", "nevra_to_string", "package_href")

def repodata_only_local_repos(ctx: AnalysisContext, dnf_available_repos: list["RepoInfo"]) -> Artifact:
    """
    Produce a directory that contains a local copy of the available RPM repo's
    repodata directories.
    This directory is used during dnf resolution while forming the compiler
    plan, so it's ok that the Packages/ directory will be missing.
    """
    dir = ctx.actions.declare_output("dnf_repodatas", dir = True)

    tree = {}
    for repo_info in dnf_available_repos:
        tree[paths.join(repo_info.id, "repodata")] = repo_info.repodata
        for key in repo_info.gpg_keys:
            tree[paths.join(repo_info.id, "gpg-keys", key.basename)] = key
        tree[paths.join(repo_info.id, "dnf_conf.json")] = repo_info.dnf_conf_json

    # copied_dir instead of symlink_dir so that this can be directly bind
    # mounted into the container
    ctx.actions.copied_dir(dir, tree)
    return dir

def _best_rpm_artifact(
        *,
        rpm_info: "RpmInfo",
        reflink_flavor: str | None,
        disable_reflink: bool) -> Artifact:
    if disable_reflink:
        return rpm_info.raw_rpm

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

def _possible_rpm_artifacts(*, rpm_info: "RpmInfo", reflink_flavor: str | None) -> list[Artifact]:
    artifacts = [rpm_info.raw_rpm]
    if reflink_flavor and reflink_flavor in rpm_info.extents:
        artifacts.append(rpm_info.extents[reflink_flavor])
    return artifacts

def compiler_plan_to_local_repos(
        ctx: AnalysisContext,
        identifier_prefix: str,
        dnf_available_repos: list["RepoInfo"],
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

        # On CentOS 8 we cannot mix reflink and non-reflink rpms
        if reflink_flavor == "centos8":
            # TODO(T160732259) falling back for the entire transaction significantly slows
            # down builds, but rpm barfs if there are transcoded and non-transcoded
            # rpms in the same transaction
            disable_reflink = False
            user_rpms_causing_no_reflink = []
            num_user_rpms = 0

            for install in tx["install"]:
                if install["reason"] == "user":
                    num_user_rpms += 1
                for repo in by_repo.values():
                    if install["nevra"] in repo["nevras"]:
                        if repo["repo_info"].disable_rpm_reflink:
                            disable_reflink = True

                            # If it's a dep, there's not really much they can do...
                            if install["reason"] == "user":
                                user_rpms_causing_no_reflink.append(install["nevra"])

            if disable_reflink and user_rpms_causing_no_reflink and len(user_rpms_causing_no_reflink) < num_user_rpms:
                if ctx.label.name != "mix-reflink-and-not--layer":
                    message = """{label}: UNNECESSARILY SLOW BUILD ALERT!!!!
    Some RPMs are causing your entire layer to fallback to non-reflink RPM
    installation. This will CONSIDERABLY SLOW DOWN your build. Until T160732259 is
    resolved upstream, the only way to avoid this slowdown is to install these RPMs
    in a separate layer (must be parent_layer if using chef-solo).
    {rpms}
                    """.format(label = ctx.label.raw_target(), rpms = user_rpms_causing_no_reflink)
                    warning(message)
        else:
            disable_reflink = False

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
                        disable_reflink = disable_reflink,
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
