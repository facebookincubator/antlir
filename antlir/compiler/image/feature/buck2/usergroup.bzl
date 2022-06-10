# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:usergroup.shape.bzl", "group_t", "user_t")
load(":helpers.bzl", "generate_feature_target_name")
load(":providers.bzl", "feature_provider")

SHELL_BASH = "/bin/bash"
SHELL_NOLOGIN = "/sbin/nologin"
NO_UID, NO_GID = -1, -1

def feature_user_add_rule_impl(ctx: "context") -> ["provider"]:
    return feature_provider(
        "users",
        shape.new(
            user_t,
            name = ctx.attr.username,
            id = ctx.attr.uid if ctx.attr.uid != NO_UID else None,
            primary_group = ctx.attr.primary_group,
            supplementary_groups = ctx.attr.supplementary_groups,
            shell = ctx.attr.shell,
            home_dir = ctx.attr.home_dir,
            comment = ctx.attr.comment or None,
        ),
    )

feature_user_add_rule = rule(
    implementation = feature_user_add_rule_impl,
    attrs = {
        "comment": attr.string(default = ""),
        "home_dir": attr.string(),
        "primary_group": attr.string(),
        "shell": attr.string(),
        "supplementary_groups": attr.list(attr.string(), default = []),
        "uid": attr.int(default = NO_UID),
        "username": attr.string(),
    },
)

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
image.ensure_dirs_exist(
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
- `home_dir` should exist, but this item does not ensure/depend on it to avoid
    a circular dependency on directory's owner user.
    """
    target_name = generate_feature_target_name(
        name = "user_add",
        include_in_name = {"username": username},
        include_only_in_hash = {
            "comment": comment,
            "home_dir": home_dir,
            "primary_group": primary_group,
            "shell": shell,
            "supplementary_groups": supplementary_groups,
            "uid": uid,
        },
    )

    if not native.rule_exists(target_name):
        feature_user_add_rule(
            name = target_name,
            username = username,
            primary_group = primary_group,
            home_dir = home_dir,
            shell = shell,
            uid = uid,
            supplementary_groups = supplementary_groups,
            comment = comment,
        )

    return ":" + target_name

def feature_group_add_rule_impl(ctx: "context") -> ["provider"]:
    return feature_provider(
        "groups",
        shape.new(
            group_t,
            name = ctx.attr.groupname,
            id = ctx.attr.gid if ctx.attr.gid != NO_GID else None,
        ),
    )

feature_group_add_rule = rule(
    implementation = feature_group_add_rule_impl,
    attrs = {
        "gid": attr.int(default = NO_GID),
        "groupname": attr.string(),
    },
)

def feature_group_add(groupname, gid = None):
    """
`feature.group_add("leet")` adds a group `leet` with an auto-assigned group ID.
`feature.group_add("leet", 1337)` adds a group `leet` with GID 1337.

Group add semantics generally follow `groupadd`. If groupname or GID conflicts
with existing entries, image build will fail. It is recommended to avoid
specifying GID unless absolutely necessary.

It is also recommended to always reference groupnames and not GIDs; since GIDs
are auto-assigned, they may change if underlying layers add/remove groups.
    """
    target_name = generate_feature_target_name(
        name = "group_add",
        include_in_name = {
            "gid": gid,
            "groupname": groupname,
        },
    )

    if not native.rule_exists(target_name):
        feature_group_add_rule(
            name = target_name,
            groupname = groupname,
            gid = gid,
        )

    return ":" + target_name
