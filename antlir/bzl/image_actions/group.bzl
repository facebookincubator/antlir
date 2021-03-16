# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

group_t = shape.shape(
    name = str,
    id = shape.field(int, optional = True),
)

def image_group_add(groupname, gid = None):
    """
`image.group_add("leet")` adds a group `leet` with an auto-assigned group ID.
`image.group_add("leet", 1337)` adds a group `leet` with GID 1337.

Group add semantics generally follow `groupadd`. If groupname or GID conflicts
with existing entries, image build will fail. It is recommended to avoid
specifying GID unless absolutely necessary.

It is also recommended to always reference groupnames and not GIDs; since GIDs
are auto-assigned, they may change if underlying layers add/remove groups.
    """

    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(groups = [shape.new(group_t, name = groupname, id = gid)]),
    )
