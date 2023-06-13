# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

SHELL_BASH = "/bin/bash"
SHELL_NOLOGIN = "/sbin/nologin"

def user_add(
        *,
        username: [str.type, "selector"],
        primary_group: [str.type, "selector"],
        home_dir: [str.type, "selector"],
        shell: [str.type, "selector"] = SHELL_NOLOGIN,
        uid: [int.type, "selector", None] = None,
        supplementary_groups: [[[str.type, "selector"]], "selector"] = [],
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
        groupname: [str.type, "selector"],
        gid: [int.type, "selector", None] = None) -> ParseTimeFeature.type:
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
        username: [str.type, "selector"],
        add_supplementary_groups: [[[str.type, "selector"]], "selector"] = []) -> ParseTimeFeature.type:
    """
    Modify an existing entry in the /etc/passwd and /etc/group databases
    """
    return ParseTimeFeature(
        feature_type = "user_mod",
        kwargs = {
            "add_supplementary_groups": add_supplementary_groups,
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

user_analyze = data_only_feature_analysis_fn(user_record)

group_record = record(
    name = str.type,
    gid = [int.type, None],
)

group_analyze = data_only_feature_analysis_fn(group_record)

usermod_record = record(
    username = str.type,
    add_supplementary_groups = [str.type],
)

usermod_analyze = data_only_feature_analysis_fn(usermod_record)
