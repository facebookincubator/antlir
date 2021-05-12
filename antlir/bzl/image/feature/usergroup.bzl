# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

user_t = shape.shape(
    name = str,
    id = shape.field(int, optional = True),
    primary_group = str,
    supplementary_groups = shape.list(str),
    shell = shape.path(),
    home_dir = shape.path(),
    comment = shape.field(str, optional = True),
)

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

    user = shape.new(
        user_t,
        name = username,
        id = uid,
        primary_group = primary_group,
        supplementary_groups = supplementary_groups or [],
        shell = shell,
        home_dir = home_dir,
        comment = comment,
    )
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(users = [user]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image/feature:usergroup"],
    )

group_t = shape.shape(
    name = str,
    id = shape.field(int, optional = True),
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

    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(groups = [shape.new(group_t, name = groupname, id = gid)]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image/feature:usergroup"],
    )
