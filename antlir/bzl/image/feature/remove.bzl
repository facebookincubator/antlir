# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")
load(":remove.shape.bzl", "remove_paths_t")

def feature_remove(dest, must_exist = True):
    """
`feature.remove("/a/b")` recursively removes the file or directory `/a/b` --

These are allowed to remove paths inherited from the parent layer, or those
installed by RPMs even in this layer. However, removing other items
explicitly added by the current layer is not allowed since that seems like a
design smell -- you should probably refactor the constituent image features
not to conflict with each other.

By default, it is an error if the specified path is missing from the image,
though this can be avoided by setting `must_exist` to `False`.
    """
    remove_spec = remove_paths_t(
        path = dest,
        must_exist = must_exist,
    )
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(remove_paths = [remove_spec]),
    )
