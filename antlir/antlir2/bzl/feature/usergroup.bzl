# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load(":ensure_dirs_exist.bzl", "ensure_subdirs_exist")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

SHELL_BASH = "/bin/bash"
SHELL_NOLOGIN = "/sbin/nologin"

def user_add(
        *,
        username: str | Select,
        primary_group: str | Select,
        home_dir: str | Select,
        shell: str | Select = SHELL_NOLOGIN,
        uid: int | Select | None = None,
        supplementary_groups: list[str | Select] | Select = [],
        comment: str | None = None) -> ParseTimeFeature:
    """
    Add a user entry to /etc/passwd.

    Example usage:

    ```
    feature.group_add("myuser")
    feature.user_add(
        "myuser",
        primary_group = "myuser",
        home_dir = "/home/myuser",
    )
    feature.ensure_dirs_exist(
        "/home/myuser",
        mode = 0o755,
        user = "myuser",
        group = "myuser",
    )
    ```

    Unlike shadow-utils `useradd`, this item does not automatically create the new
    user's initial login group or home directory.

    - If `username` or `uid` conflicts with existing entries, image build will
        fail. It is recommended to avoid specifying UID unless absolutely
        necessary.
    - `primary_group` and `supplementary_groups` are specified as groupnames.
    - `home_dir` must exist
    """
    return ParseTimeFeature(
        feature_type = "user",
        plugin = antlir2_dep("features:user"),
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

def group_add(
        *,
        groupname: str | Select,
        gid: int | Select | None = None) -> ParseTimeFeature:
    """
    Add a group entry to /etc/group

    Group add semantics generally follow `groupadd`. If groupname or GID
    conflicts with existing entries, image build will fail. It is recommended to
    avoid specifying GID unless absolutely necessary.
    """
    return ParseTimeFeature(
        feature_type = "group",
        plugin = antlir2_dep("features:group"),
        kwargs = {
            "gid": gid,
            "groupname": groupname,
        },
    )

def usermod(
        *,
        username: str | Select,
        add_supplementary_groups: list[str | Select] | Select = []) -> ParseTimeFeature:
    """
    Modify an existing entry in the /etc/passwd and /etc/group databases
    """
    return ParseTimeFeature(
        feature_type = "user_mod",
        plugin = antlir2_dep("features:usermod"),
        kwargs = {
            "add_supplementary_groups": add_supplementary_groups,
            "username": username,
        },
    )

user_rule = data_only_feature_rule(
    feature_attrs = {
        "comment": attrs.option(attrs.string(), default = None),
        "home_dir": attrs.string(),
        "primary_group": attrs.string(),
        "shell": attrs.string(),
        "supplementary_groups": attrs.list(attrs.string()),
        "uid": attrs.option(attrs.int(), default = None),
        "username": attrs.string(),
    },
    feature_type = "user",
)

group_rule = data_only_feature_rule(
    feature_attrs = {
        "gid": attrs.option(attrs.int(), default = None),
        "groupname": attrs.string(),
    },
    feature_type = "group",
)

usermod_rule = data_only_feature_rule(
    feature_attrs = {
        "add_supplementary_groups": attrs.list(attrs.string()),
        "username": attrs.string(),
    },
    feature_type = "user_mod",
)

def standard_user(
        username: str,
        groupname: str,
        home_dir: str | None = None,
        shell: str = SHELL_BASH,
        uid: int | None = None,
        gid: int | None = None,
        supplementary_groups: list[str] = []) -> list[ParseTimeFeature | list[ParseTimeFeature]]:
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
        ),
        user_add(
            username = username,
            primary_group = groupname,
            home_dir = home_dir,
            shell = shell,
            uid = uid,
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
