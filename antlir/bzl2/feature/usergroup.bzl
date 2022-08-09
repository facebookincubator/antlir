# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl/image/feature:usergroup.shape.bzl", "group_t", "user_t")
load("//antlir/bzl2:feature_rule.bzl", "maybe_add_feature_rule")
load(":ensure_dirs_exist.bzl", "feature_ensure_subdirs_exist")

SHELL_BASH = "/bin/bash"
SHELL_NOLOGIN = "/sbin/nologin"

def feature_user_add(
        username,
        primary_group,
        home_dir,
        shell = SHELL_BASH,
        uid = None,
        supplementary_groups = None,
        comment = None):
    """
    `feature.user_add` adds a user entry to /etc/passwd.

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

    Unlike shadow-utils `useradd`, this item does not automatically create the
    new user's initial login group or home directory.

    - If `username` or `uid` conflicts with existing entries, image build will
        fail. It is recommended to avoid specifying UID unless absolutely
        necessary.
    - `primary_group` and `supplementary_groups` are specified as groupnames.
    - `home_dir` should exist, but this item does not ensure/depend on it to
        avoid a circular dependency on directory's owner user.
    """

    # copy in buck1 version
    return maybe_add_feature_rule(
        name = "user_add",
        key = "users",
        include_in_target_name = {"username": username},
        feature_shape = user_t(
            name = username,
            id = uid,
            primary_group = primary_group,
            supplementary_groups = supplementary_groups or [],
            shell = shell,
            home_dir = home_dir,
            comment = comment,
        ),
    )

def feature_group_add(groupname, gid = None):
    """
    `feature.group_add("leet")` adds a group `leet` with an auto-assigned group
    ID. `feature.group_add("leet", 1337)` adds a group `leet` with GID 1337.

    Group add semantics generally follow `groupadd`. If groupname or GID
    conflicts with existing entries, image build will fail. It is recommended to
    avoid specifying GID unless absolutely necessary.

    It is also recommended to always reference groupnames and not GIDs; since
    GIDs are auto-assigned, they may change if underlying layers add/remove
    groups.
    """

    # copy in buck1 version
    return maybe_add_feature_rule(
        name = "group_add",
        key = "groups",
        include_in_target_name = {
            "groupname": groupname,
        },
        feature_shape = group_t(
            name = groupname,
            id = gid,
        ),
    )

def feature_setup_standard_user(
        user,
        group,
        homedir = None,
        shell = SHELL_BASH):
    """
    A convenient function that wraps `feature.group_add`, `feature.user_add`,
    and home dir creation logic.
    The parent directory of `homedir` must already exist.
    """
    if homedir == None:
        homedir = "/home/" + user
    homedir_parent = paths.dirname(homedir)
    homedir_basename = paths.basename(homedir)
    return [
        feature_group_add(group),
        feature_user_add(
            user,
            group,
            homedir,
            shell,
        ),
        feature_ensure_subdirs_exist(
            homedir_parent,
            homedir_basename,
            user = user,
            group = group,
            mode = 0o0770,
        ),
    ]
