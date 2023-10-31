# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load(":ensure_dirs_exist.bzl", "ensure_subdirs_exist")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

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
            "name": username,
            "primary_group": primary_group,
            "shell": shell,
            "supplementary_groups": supplementary_groups,
            "uid": uid,
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
            "name": groupname,
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

user_record = record(
    name = str,
    uid = [int, None],
    home_dir = str,
    shell = str,
    primary_group = str,
    supplementary_groups = list[str],
    comment = [str, None],
)

user_analyze = data_only_feature_analysis_fn(
    user_record,
    feature_type = "user",
)

group_record = record(
    name = str,
    gid = [int, None],
)

group_analyze = data_only_feature_analysis_fn(
    group_record,
    feature_type = "group",
)

usermod_record = record(
    username = str,
    add_supplementary_groups = list[str],
)

usermod_analyze = data_only_feature_analysis_fn(
    usermod_record,
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
