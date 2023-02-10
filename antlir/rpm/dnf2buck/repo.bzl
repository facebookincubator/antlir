# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":rpm.bzl", "RpmInfo", "nevra_to_string", "package_href")

RepoInfo = provider(fields = {
    "base_url": "Optional upstream URL that was used to populate this target",
    "id": "Repo name",
    "offline": "Complete offline archive of repodata and all RPMs",
    "repodata": "Populated repodata/ directory",
})

def _impl(ctx: "context") -> ["provider"]:
    rpm_infos = [rpm[RpmInfo] for rpm in ctx.attrs.rpms]

    # Construct repodata XML blobs from each individual RPM
    repodata = ctx.actions.declare_output("repodata", dir = True)
    primary_dir = ctx.actions.declare_output("primary", dir = True)
    ctx.actions.copied_dir(primary_dir, {nevra_to_string(rpm.nevra): rpm.xml_primary for rpm in rpm_infos})
    filelists_dir = ctx.actions.declare_output("filelists", dir = True)
    ctx.actions.copied_dir(filelists_dir, {nevra_to_string(rpm.nevra): rpm.xml_filelists for rpm in rpm_infos})
    other_dir = ctx.actions.declare_output("other", dir = True)
    ctx.actions.copied_dir(other_dir, {nevra_to_string(rpm.nevra): rpm.xml_other for rpm in rpm_infos})
    optional_args = []
    if ctx.attrs.timestamp != None:
        optional_args += ["--timestamp={}".format(ctx.attrs.timestamp)]
    ctx.actions.run(
        cmd_args(
            ctx.attrs.makerepo[RunInfo],
            cmd_args(primary_dir, format = "--primary-dir={}"),
            cmd_args(filelists_dir, format = "--filelists-dir={}"),
            cmd_args(other_dir, format = "--other-dir={}"),
            cmd_args(repodata.as_output(), format = "--out={}"),
            "--compress={}".format(ctx.attrs.compress),
            optional_args,
        ),
        category = "makerepo",
    )

    # Create an artifact that is the _entire_ repository for completely offline
    # usage
    offline = ctx.actions.declare_output("offline", dir = True)
    offline_map = {
        "repodata": repodata,
    }
    for rpm in rpm_infos:
        offline_map[package_href(rpm.nevra, rpm.pkgid)] = rpm.rpm
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
    proxy_config = ctx.actions.write_json("proxy_config.json", {
        ctx.label.name: {
            "offline_dir": offline,
            "repodata_dir": repodata,
            "urlgen": urlgen_config,
        },
    })

    return [
        DefaultInfo(default_outputs = [repodata], sub_targets = {
            "offline": [DefaultInfo(default_outputs = [offline])],
            "proxy_config": [DefaultInfo(default_outputs = [proxy_config])],
            "repodata": [DefaultInfo(default_outputs = [repodata])],
            "serve": [DefaultInfo(), RunInfo(
                args = cmd_args(ctx.attrs.repo_proxy[RunInfo], "--repos-json", proxy_config)
                    .hidden(repodata)
                    .hidden([offline] if offline_only_repo else []),
            )],
        }),
        RepoInfo(
            id = ctx.label.name,
            repodata = repodata,
            offline = offline,
            base_url = ctx.attrs.base_url,
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
    "makerepo": attrs.default_only(attrs.exec_dep(default = "//antlir/rpm/dnf2buck:makerepo")),
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
