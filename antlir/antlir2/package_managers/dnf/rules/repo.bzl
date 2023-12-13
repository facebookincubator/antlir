# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":rpm.bzl", "RpmInfo", "nevra_to_string", "package_href")

RepoInfo = provider(fields = [
    "all_rpms",  # All RpmInfos contained in this repo
    "base_url",  # Optional upstream URL that was used to populate this target
    "dnf_conf_json",  # JSON serialized dnf.conf KV for this repo
    "gpg_keys",  # Optional artifact against which signatures will be checked
    "id",  # Repo name
    "offline",  # Complete offline archive of repodata and all RPMs
    "proxy_config",  # proxy config for this repo
    "repodata",  # Populated repodata/ directory
    "urlgen",  # URL generator for repo_proxy::RpmRepo
])

def _impl(ctx: AnalysisContext) -> list[Provider]:
    rpm_infos = [rpm[RpmInfo] for rpm in ctx.attrs.rpms]

    repo_id = ctx.label.name.replace("/", "_")

    # Construct repodata XML blobs from each individual RPM
    xml_dir = ctx.actions.declare_output("xml", dir = True)
    ctx.actions.copied_dir(xml_dir, {nevra_to_string(rpm.nevra): rpm.xml for rpm in rpm_infos})
    optional_args = []
    if ctx.attrs.timestamp != None:
        optional_args += ["--timestamp={}".format(ctx.attrs.timestamp)]

    # First build a repodata directory that just contains repodata (this would
    # be suitable as a baseurl for dnf)
    plain_repodata = ctx.actions.declare_output("repodata", dir = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs.makerepo[RunInfo],
            cmd_args(repo_id, format = "--repo-id={}"),
            cmd_args(xml_dir, format = "--xml-dir={}"),
            cmd_args(ctx.attrs.module_md, format = "--module-md={}") if ctx.attrs.module_md else cmd_args(),
            cmd_args(plain_repodata.as_output(), format = "--out={}"),
            "--compress={}".format(ctx.attrs.compress),
            optional_args,
        ),
        category = "repodata",
    )

    # On the local host (because we need system python3 with dnf), pre-build
    # .solv(x) files so that dnf installation is substantially faster
    repodata = ctx.actions.declare_output("repodata_with_solv", dir = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs.build_solv[RunInfo],
            repo_id,
            plain_repodata,
            repodata.as_output(),
        ),
        category = "solv",
    )

    # Create an artifact that is the _entire_ repository for completely offline
    # usage
    offline = ctx.actions.declare_output("offline", dir = True)
    offline_map = {
        "repodata": repodata,
    }
    for rpm in rpm_infos:
        offline_map[package_href(rpm.nevra, rpm.pkgid)] = rpm.raw_rpm
    ctx.actions.copied_dir(offline, offline_map)

    # repos that are not backed by manifold must use the "offline" urlgen
    # setting as well as setting the offline directory as a dependency of the
    # `[serve]` sub-target
    offline_only_repo = not ctx.attrs.bucket
    urlgen_config = {
        "Manifold": {
            "api_key": ctx.attrs.api_key,
            "bucket": ctx.attrs.bucket,
            "snapshot_base": "flat/",
        },
    } if not offline_only_repo else {"Offline": None}
    proxy_config = {
        "gpg_keys": ctx.attrs.gpg_keys,
        "offline_dir": offline,
        "offline_only": offline_only_repo,
        "repodata_dir": repodata,
        "urlgen": urlgen_config,
    }
    combined_proxy_config = ctx.actions.write_json("proxy_config.json", {
        ctx.label.name: proxy_config,
    })

    dnf_conf_json = ctx.actions.write_json("dnf_conf.json", ctx.attrs.dnf_conf)

    return [
        DefaultInfo(default_outputs = [repodata], sub_targets = {
            "offline": [DefaultInfo(default_outputs = [offline])],
            "repodata": [DefaultInfo(default_outputs = [repodata])],
            "serve": [DefaultInfo(), RunInfo(
                args = cmd_args(ctx.attrs.repo_proxy[RunInfo], "--repos-json", combined_proxy_config)
                    .hidden(repodata)
                    .hidden([offline] if offline_only_repo else []),
            )],
        }),
        RepoInfo(
            id = repo_id,
            repodata = repodata,
            gpg_keys = ctx.attrs.gpg_keys,
            offline = offline,
            base_url = ctx.attrs.base_url,
            urlgen = urlgen_config,
            all_rpms = rpm_infos,
            proxy_config = proxy_config,
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
    "build_solv": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/package_managers/dnf/rules:build-solv")),
    "compress": attrs.enum(["none", "gzip"], default = "gzip"),
    "deleted_base_key": attrs.option(
        attrs.string(),
        doc = "base key for recently-deleted packages in manifold",
        default = None,
    ),
    "dnf_conf": attrs.dict(attrs.string(), attrs.string(), default = {}),
    "gpg_keys": attrs.list(attrs.source(doc = "GPG keys that packages are signed with"), default = []),
    "makerepo": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/package_managers/dnf/rules:makerepo")),
    "module_md": attrs.option(attrs.source(), default = None),
    "repo_proxy": attrs.default_only(attrs.exec_dep(default = "//antlir/rpm/repo_proxy:repo-proxy")),
    "rpms": attrs.list(
        attrs.dep(providers = [RpmInfo]),
        doc = "All RPMs that should be included in this repo",
    ),
    "source_base_key": attrs.option(
        attrs.string(),
        doc = "base key in manifold",
        default = None,
    ),
    "timestamp": attrs.option(attrs.int(doc = "repomd.xml revision"), default = None),
}

repo = rule(
    impl = _impl,
    attrs = repo_attrs,
)

RepoSetInfo = provider(fields = ["repo_infos", "proxy_cmd"])

def _repo_set_impl(ctx: AnalysisContext) -> list[Provider]:
    combined_repodatas = ctx.actions.declare_output("repodatas")
    all_repos = {}
    for repo in ctx.attrs.repos:
        repo_info = repo[RepoInfo]
        if repo_info.id in all_repos:
            fail("repo id '{}' found twice".format(repo_info.id))
        all_repos[repo_info.id] = repo_info
    for set in ctx.attrs.repo_sets:
        for repo_info in set[RepoSetInfo].repo_infos:
            if repo_info.id in all_repos:
                fail("repo id '{}' found twice".format(repo_info.id))
            all_repos[repo_info.id] = repo_info

    ctx.actions.copied_dir(combined_repodatas, {id: repo_info.repodata for id, repo_info in all_repos.items()})

    proxy_config = ctx.actions.write_json(
        "proxy_config.json",
        {
            id: repo_info.proxy_config
            for id, repo_info in all_repos.items()
        },
    )

    proxy_cmd = (
        cmd_args(ctx.attrs.repo_proxy[RunInfo], "--repos-json", proxy_config)
            .hidden([repo_info.repodata for repo_info in all_repos.values()])
            .hidden(
            # repos that are offline_only (not backed by a remote store &
            # urlgen) must be materialized locally before serving this repo_set
            [repo_info.offline for repo_info in all_repos.values() if repo_info.proxy_config["offline_only"]],
        )
    )
    # .hidden([offline] if offline_only_repo else []),

    return [
        RepoSetInfo(
            repo_infos = all_repos.values(),
            proxy_cmd = proxy_cmd,
        ),
        DefaultInfo(
            combined_repodatas,
            sub_targets = {
                "proxy": [
                    DefaultInfo(),
                    RunInfo(
                        args = proxy_cmd,
                    ),
                ],
            },
        ),
    ]

repo_set = rule(
    impl = _repo_set_impl,
    attrs = {
        "repo_proxy": attrs.default_only(attrs.exec_dep(default = "//antlir/rpm/repo_proxy:repo-proxy")),
        "repo_sets": attrs.list(attrs.dep(providers = [RepoSetInfo]), default = []),
        "repos": attrs.list(attrs.dep(providers = [RepoInfo]), default = []),
    },
    doc = "Collect a set of repos into a single easy-to-use rule",
)
