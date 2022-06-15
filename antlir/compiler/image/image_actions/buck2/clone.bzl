# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image_source.bzl", "image_source")
load("//antlir/bzl:shape.bzl", "shape")
load(
    "//antlir/compiler/image/feature/buck2:helpers.bzl",
    "normalize_target_and_mark_path",
)
load(
    "//antlir/compiler/image/feature/buck2:image_source.shape.bzl",
    "image_source_t",
)
load(
    "//antlir/compiler/image/feature/buck2:rules.bzl",
    "maybe_add_feature_rule",
)
load(":clone.shape.bzl", "clone_t")

def _generate_shape(source_dict, src_layer, src_path, dest_path):
    omit_outer_dir = src_path.endswith("/")
    pre_existing_dest = dest_path.endswith("/")
    if omit_outer_dir and not pre_existing_dest:
        fail(
            "Your `src_path` {} ends in /, which means only the contents of " +
            "the directory will be cloned. Therefore, you must also add a " +
            "trailing / to `dest_path` to signal that clone will write " +
            "inside that pre-existing directory",
            "dest_path",
        )

    return shape.new(
        clone_t,
        dest = dest_path,
        omit_outer_dir = omit_outer_dir,
        pre_existing_dest = pre_existing_dest,
        source = shape.new(image_source_t, **source_dict),
        source_layer = {"__BUCK_LAYER_TARGET": src_layer},
    )

def image_clone(src_layer, src_path, dest_path):
    """
    `image.clone("//path/to:src_layer", "src/path", "dest/path")` copies a
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
    - Similar to `image.symlink*`, a trailing slash in `dest_path` means that
        it's a pre-existing directory (e.g.  made by `image.ensure_dirs_exist`),
        and `clone` will only write to:
        * `dest/(basename of src_path)` if `src_path` lacks a trailing /
        * `dest/(children of src_path)` if `src_path` has a trailing /

    ### Known deviations from perfect cloning

    Most likely, SELinux attrs change. Future: add real tests for this?

    ### No UID/GID remapping is attempted

    We assume that `:src_layer` has the same user/group DB as the new layer.

    ### When to use this?

    Often, instead of using , you should prefer `image.layer_mount`, which
    allows you to compose independent pieces of the filesystem at *runtime*,
    without incurring the cost of publishing images with a lot of duplicated
    content.

    If you're trying to copy the output of a regular Buck target, instead use
    `feature.install` or `feature.install_buck_runnable`. These rewrite
    filesystem metadata to a deterministic state, while the state of the on-disk
    metadata in `buck-out` is undefined.
    """
    source_dict = shape.as_dict_shallow(image_source(
        layer = src_layer,
        path = src_path,
    ))
    source_dict, normalized_target = normalize_target_and_mark_path(source_dict)

    return maybe_add_feature_rule(
        name = "clone",
        include_in_target_name = {
            "dest_path": dest_path,
            "src_layer": src_layer,
            "src_path": src_path,
        },
        feature_shape = _generate_shape(
            source_dict,
            normalized_target,
            src_path,
            dest_path,
        ),
        deps = [normalized_target],
    )
