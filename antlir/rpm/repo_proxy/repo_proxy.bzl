# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoInfo")

types.lint_noop(RepoInfo)

def repo_proxy_config(ctx: "context", repos: {str.type: RepoInfo.type}) -> "artifact":
    return ctx.actions.write_json(
        "repo_proxy.json",
        {id: {
            "offline_dir": repo.offline,
            "repodata_dir": repo.repodata,
            "urlgen": repo.urlgen,
        } for id, repo in repos.items()},
    )
