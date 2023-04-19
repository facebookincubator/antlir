# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "ParseTimeFeature")

types.lint_noop()

SHELL_BASH = "/bin/bash"
SHELL_NOLOGIN = "/sbin/nologin"

def user_add(
        *,
        username: types.or_selector(str.type),
        primary_group: types.or_selector(str.type),
        home_dir: types.or_selector(str.type),
        shell: types.or_selector(str.type) = SHELL_NOLOGIN,
        uid: types.optional(types.or_selector(int.type)) = None,
        supplementary_groups: types.or_selector([str.type]) = [],
        comment: [str.type, None] = None) -> ParseTimeFeature.type:
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
        groupname: types.or_selector(str.type),
        gid: types.optional(types.or_selector(int.type)) = None) -> ParseTimeFeature.type:
    """
    Add a group entry to /etc/group

    Group add semantics generally follow `groupadd`. If groupname or GID
    conflicts with existing entries, image build will fail. It is recommended to
    avoid specifying GID unless absolutely necessary.
    """
    return ParseTimeFeature(
        feature_type = "group",
        kwargs = {
            "gid": gid,
            "name": groupname,
        },
    )

def usermod(
        *,
        username: types.or_selector(str.type),
        add_supplementary_groups: types.optional(types.or_selector([str.type])) = None) -> ParseTimeFeature.type:
    """
    Modify an existing entry in the /etc/passwd and /etc/group databases
    """
    return ParseTimeFeature(
        feature_type = "user_mod",
        kwargs = {
            "add_supplementary_groups": add_supplementary_groups or [],
            "username": username,
        },
    )

user_record = record(
    name = str.type,
    uid = [int.type, None],
    home_dir = str.type,
    shell = str.type,
    primary_group = str.type,
    supplementary_groups = [str.type],
    comment = [str.type, None],
)

user_to_json = user_record

group_record = record(
    name = str.type,
    gid = [int.type, None],
)

group_to_json = group_record

usermod_record = record(
    username = str.type,
    add_supplementary_groups = [str.type],
)

usermod_to_json = usermod_record
