# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

SnapshottableInfo = provider(fields = {
    # A directory artifact that should contain all the metadata artifacts that
    # should be snapshotted into persistent storage. This is considered cheap to
    # produce and store, as opposed to package blobs (which are belligerent and
    # numerous https://www.youtube.com/watch?v=HvVJQMnB-N8), so the target that
    # contains this provider should just produce the entire tree of metadata
    # artifacts every time.
    "metadata_tree": Artifact,
})
