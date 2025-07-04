# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/bzl:internal_external.bzl", "is_facebook")
load(":rpm.bzl", "RpmInfo", "package_href")

RepoInfo = provider(fields = [
    "all_rpms",  # All RpmInfos contained in this repo
    "logical_id",  # ID/Name of a Repo as in dnf.conf
    "base_url",  # Optional upstream URL that was used to populate this target
    "dnf_conf_json",  # JSON serialized dnf.conf KV for this repo
    "gpg_keys",  # Optional artifact against which signatures will be checked
    "id",  # Repo name
    "repodata",  # Populated repodata/ directory
])

def _impl(ctx: AnalysisContext) -> list[Provider]:
    rpm_infos = [rpm[RpmInfo] for rpm in ctx.attrs.rpms]

    if "//" in ctx.label.name:
        fail("repo names must not contain consecutive slashes")

    repo_id = ctx.label.name.replace("_", "__").replace("/", "_")

    # Construct repodata XML blobs from each individual RPM
    xml_dir = ctx.actions.declare_output("xml", dir = True)
    ctx.actions.copied_dir(xml_dir, {rpm.nevra: rpm.xml for rpm in rpm_infos})
    optional_args = []

    # First build a repodata directory that just contains repodata (this would
    # be suitable as a baseurl for dnf)
    if not ctx.attrs.repodata:
        plain_repodata = ctx.actions.declare_output("repodata", dir = True)
        ctx.actions.run(
            cmd_args(
                ctx.attrs.makerepo[RunInfo],
                cmd_args(repo_id, format = "--repo-id={}"),
                cmd_args(xml_dir, format = "--xml-dir={}"),
                cmd_args(ctx.attrs.module_md, format = "--module-md={}") if ctx.attrs.module_md else cmd_args(),
                cmd_args(plain_repodata.as_output(), format = "--out={}"),
                "--compress={}".format(ctx.attrs.compress),
                "--expected-rpm-count={}".format(len(ctx.attrs.rpms)),
                optional_args,
            ),
            category = "repodata",
            local_only = True,
            allow_cache_upload = True,
        )
    else:
        plain_repodata = ctx.attrs.repodata

    if is_facebook:
        # Pre-build .solv(x) files so that dnf installation is substantially faster
        # TODO: use repomdxml2solv from libsolv-tools instead of this sketchiness
        repodata = ctx.actions.declare_output("repodata_with_solv", dir = True)
        ctx.actions.run(
            cmd_args(
                ctx.attrs.makecache[RunInfo],
                repo_id,
                plain_repodata,
                repodata.as_output(),
            ),
            category = "solv",
        )
    else:
        repodata = plain_repodata

    # Create an artifact that is the _entire_ repository for completely offline
    # usage
    offline = ctx.actions.declare_output("offline", dir = True)
    offline_map = {
        "repodata": repodata,
    }
    for rpm in rpm_infos:
        offline_map[package_href(rpm.nevra, rpm.pkgid)] = rpm.raw_rpm
    ctx.actions.copied_dir(offline, offline_map)

    dnf_conf_json = ctx.actions.write_json("dnf_conf.json", ctx.attrs.dnf_conf)

    return [
        DefaultInfo(default_outputs = [repodata], sub_targets = {
            "offline": [DefaultInfo(default_outputs = [offline])],
            "plain_repodata": [DefaultInfo(default_outputs = [plain_repodata])],
            "repodata": [DefaultInfo(default_outputs = [repodata])],
        }),
        RepoInfo(
            id = repo_id,
            logical_id = ctx.attrs.logical_id,
            repodata = repodata,
            gpg_keys = ctx.attrs.gpg_keys,
            base_url = ctx.attrs.base_url,
            all_rpms = rpm_infos,
            dnf_conf_json = dnf_conf_json,
        ),
    ]

repo_attrs = {
    "api_key": attrs.option(attrs.string(doc = "manifold api key"), default = None),
    "base_url": attrs.option(
        attrs.string(),
        doc = "baseurl where this repo was snapshotted from",
        default = None,
    ),
    "bucket": attrs.option(attrs.string(doc = "manifold bucket"), default = None),
    "compress": attrs.enum(["none", "gzip"], default = "gzip"),
    "deleted_base_key": attrs.option(
        attrs.string(),
        doc = "base key for recently-deleted packages in manifold",
        default = None,
    ),
    "dnf_conf": attrs.dict(attrs.string(), attrs.string(), default = {}),
    "gpg_keys": attrs.list(attrs.source(doc = "GPG keys that packages are signed with"), default = []),
    "logical_id": attrs.option(attrs.string(), doc = "repo name as in dnf.conf", default = None),
    "makecache": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/package_managers/dnf/rules/makecache:makecache")),
    "makerepo": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/package_managers/dnf/rules/makerepo:makerepo")),
    "module_md": attrs.option(attrs.source(), default = None),
    "repodata": attrs.option(
        attrs.source(allow_directory = True),
        default = None,
        doc = "Pre-built repodata, if available",
    ),
    "rpms": attrs.list(
        attrs.dep(providers = [RpmInfo]),
        doc = "All RPMs that should be included in this repo",
    ),
    "source_base_key": attrs.option(
        attrs.string(),
        doc = "base key in manifold",
        default = None,
    ),
}

_repo = rule(
    impl = _impl,
    attrs = repo_attrs,
    supports_incoming_transition = True,
)

_repo_macro = rule_with_default_target_platform(_repo)

def repo(**kwargs):
    # the repodata action is local_only=True, so we can't run it remotely
    kwargs.setdefault("exec_compatible_with", ["prelude//platforms:may_run_local"])
    _repo_macro(**kwargs)

RepoSetInfo = provider(fields = ["repos"])

def _repo_set_impl(ctx: AnalysisContext) -> list[Provider]:
    combined_repodatas = ctx.actions.declare_output("repodatas", dir = True)
    repos = {}
    for repo in ctx.attrs.repos:
        repo_info = repo[RepoInfo]
        if repo_info.id in repos:
            fail("repo id '{}' found twice".format(repo_info.id))
        repos[repo_info.id] = repo
    for set in ctx.attrs.repo_sets:
        for repo in set[RepoSetInfo].repos:
            repo_info = repo[RepoInfo]
            if repo_info.id in repos:
                fail("repo id '{}' found twice".format(repo_info.id))
            repos[repo_info.id] = repo

    ctx.actions.copied_dir(
        combined_repodatas,
        {
            id: repo[RepoInfo].repodata
            for id, repo in repos.items()
        },
    )

    return [
        RepoSetInfo(
            repos = repos.values(),
        ),
        DefaultInfo(
            combined_repodatas,
            sub_targets = {
                "repo": [DefaultInfo(sub_targets = {
                    repo[RepoInfo].logical_id or repo[RepoInfo].id: repo.providers
                    for repo in repos.values()
                })],
            },
        ),
    ]

_repo_set = rule(
    impl = _repo_set_impl,
    attrs = {
        "repo_sets": attrs.list(attrs.dep(providers = [RepoSetInfo]), default = []),
        "repos": attrs.list(attrs.dep(providers = [RepoInfo]), default = []),
    },
    doc = "Collect a set of repos into a single easy-to-use rule",
)

repo_set = rule_with_default_target_platform(_repo_set)
