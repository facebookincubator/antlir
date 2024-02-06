# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2 = "feature")

def feature_clone(src_layer, src_path, dest_path):
    """
`feature.clone("//path/to:src_layer", "src/path", "dest/path")` copies a
subtree of an existing layer into the one under construction. To the extent
possible, filesystem metadata are preserved.

### Trailing slashes on both paths are significant

The three supported cases are:
- "s/rc" -> "dest/" creates "dest/rc"
- "s/rc/" -> "dest/" creates "dest/(children of rc)"
- "s/rc" -> "dest" creates "dest"

More explicitly:
- A trailing slash in `src_path` means "use the `rsync` convention":
    * Do not clone the source directory, but only its contents.
    * `dest_path` must be a pre-existing dir, and it must end in `/`
- Similar to `image.symlink*`, a trailing slash in `dest_path` means that it's a
    pre-existing directory (e.g.  made by `feature.ensure_dirs_exist`), and
    `clone` will only write to:
    * `dest/(basename of src_path)` if `src_path` lacks a trailing /
    * `dest/(children of src_path)` if `src_path` has a trailing /

### Known deviations from perfect cloning

Most likely, SELinux attrs change. Future: add real tests for this?

### No UID/GID remapping is attempted

We assume that `:src_layer` has the same user/group DB as the new layer.

### When to use this?

Often, instead of using , you should prefer `feature.layer_mount`, which allows
you to compose independent pieces of the filesystem at *runtime*, without
incurring the cost of publishing images with a lot of duplicated content.

If you're trying to copy the output of a regular Buck target, instead use
`feature.install` or `feature.install_buck_runnable`. These rewrite filesystem
metadata to a deterministic state, while the state of the on-disk metadata in
`buck-out` is undefined.
    """
    return antlir2.clone(
        src_layer = src_layer,
        src_path = src_path,
        dst_path = dest_path,
        # antlir1's clone always makes files owned by root:root
        # antlir2 defaults to using the same user:group as the source, but
        # that's not always possible
        user = "root",
        group = "root",
    )
