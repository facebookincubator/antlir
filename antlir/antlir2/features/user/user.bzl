# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "ParseTimeFeature",
)
load("//antlir/antlir2/features/ensure_dir_exists:ensure_dir_exists.bzl", "ensure_subdirs_exist")
load("//antlir/antlir2/features/group:group.bzl", "group_add")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

SHELL_BASH = "/bin/bash"
SHELL_NOLOGIN = "/sbin/nologin"

def user_add(
        *,
        username: str | Select,
        primary_group: str | Select,
        home_dir: str | Select,
        uid: int | Select | None = None,
        uidmap: str = "default",
        shell: str | Select = SHELL_NOLOGIN,
        supplementary_groups: list[str | Select] | Select = [],
        comment: str | None = None):
    """
    Add a user entry to /etc/passwd.

    Example usage:

    ```
    feature.group_add(
        gid = 1000,
        groupname = "myuser",
    )
    feature.user_add(
        uid = 1000,
        username = "myuser",
        primary_group = "myuser",
        home_dir = "/home/myuser",
    )
    feature.ensure_dirs_exist(
        dirs = "/home/myuser",
        mode = 0o755,
        user = "myuser",
        group = "myuser",
    )
    ```

    Unlike shadow-utils `useradd`, this item does not automatically create the new
    user's initial login group or home directory.

    - If `username` or `uid` conflicts with existing entries, image build will
        fail.
    - `primary_group` and `supplementary_groups` are specified as groupnames.
    - `home_dir` must exist
    """
    return ParseTimeFeature(
        feature_type = "user",
        plugin = "antlir//antlir/antlir2/features/user:user",
        deps = {
            "uidmap": ("antlir//antlir/uidmaps:" + uidmap) if ":" not in uidmap else uidmap,
        },
        kwargs = {
            "comment": comment,
            "home_dir": home_dir,
            "primary_group": primary_group,
            "shell": shell,
            "supplementary_groups": supplementary_groups,
            "uid": uid,
            "username": username,
        },
    )

def standard_user(
        username: str,
        groupname: str,
        uid: int | None = None,
        gid: int | None = None,
        uidmap: str = "default",
        home_dir: str | None = None,
        shell: str = SHELL_BASH,
        supplementary_groups: list[str] = []):
    """
    A convenient function that wraps `group_add`, `user_add`,
    and home dir creation logic.
    The parent directory of `home_dir` must already exist.
    """
    if home_dir == None:
        home_dir = "/home/" + username
    return [
        group_add(
            groupname = groupname,
            gid = gid,
            uidmap = uidmap,
        ),
        user_add(
            username = username,
            primary_group = groupname,
            home_dir = home_dir,
            shell = shell,
            uid = uid,
            uidmap = uidmap,
            supplementary_groups = supplementary_groups,
        ),
        ensure_subdirs_exist(
            into_dir = paths.dirname(home_dir),
            subdirs_to_create = paths.basename(home_dir),
            user = username,
            group = groupname,
            mode = 0o0750,
        ),
    ]

def _impl(ctx: AnalysisContext) -> list[Provider]:
    uidmap = ensure_single_output(ctx.attrs.uidmap)
    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "user",
            data = struct(
                comment = ctx.attrs.comment,
                home_dir = ctx.attrs.home_dir,
                primary_group = ctx.attrs.primary_group,
                shell = ctx.attrs.shell,
                supplementary_groups = ctx.attrs.supplementary_groups,
                uid = ctx.attrs.uid,
                uidmap = uidmap,
                username = ctx.attrs.username,
            ),
            build_phase = BuildPhase("compile"),
            required_artifacts = [uidmap],
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

user_rule = rule(
    impl = _impl,
    attrs = {
        "comment": attrs.option(attrs.string(), default = None),
        "home_dir": attrs.string(),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "primary_group": attrs.string(),
        "shell": attrs.string(),
        "supplementary_groups": attrs.list(attrs.string()),
        "uid": attrs.option(attrs.int()),
        "uidmap": attrs.dep(),
        "username": attrs.string(),
    },
)
