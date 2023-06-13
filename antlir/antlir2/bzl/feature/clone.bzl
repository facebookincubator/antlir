# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeDependency", "ParseTimeFeature")

def clone(
        *,
        src_layer: [str.type, "selector"],
        src_path: [str.type, "selector"],
        dst_path: [str.type, "selector"]) -> ParseTimeFeature.type:
    """
    Copies a subtree of an existing layer into the one under construction. To
    the extent possible, filesystem metadata are preserved.

    ### Trailing slashes on both paths are significant

    The three supported cases are:
    - "s/rc" -> "dest/" creates "dest/rc"
    - "s/rc/" -> "dest/" creates "dest/(children of rc)"
    - "s/rc" -> "dest" creates "dest"

    More explicitly:
    - A trailing slash in `src_path` means "use the `rsync` convention":
        * Do not clone the source directory, but only its contents.
        * `dest_path` must be a pre-existing dir, and it must end in `/`
    - A trailing slash in `dst_path` means that it's a
        pre-existing directory (e.g.  made by `ensure_dirs_exist`), and
        `clone` will only write to:
        * `dst/(basename of src_path)` if `src_path` lacks a trailing /
        * `dst/(children of src_path)` if `src_path` has a trailing /

    ### Known deviations from perfect cloning

    Most likely, SELinux attrs change.

    ### UID/GID remapping

    `src_layer` and the destination layer must have the same user/group _names_
    available, but those names do not need to map to the same ids. uid/gids will
    be remapped to the appropriate numeric id of that user/group in the
    destination layer

    ### When to use this?

    Often, instead of using this, you should prefer `layer_mount`, which allows
    you to compose independent pieces of the filesystem at *runtime*, without
    incurring the cost of publishing images with a lot of duplicated content.

    If you're trying to copy the output of a regular Buck target, instead use
    `install` or `install_buck_runnable`. These rewrite filesystem metadata to a
    deterministic state, while the state of the on-disk metadata in `buck-out`
    is undefined.
    """
    return ParseTimeFeature(
        feature_type = "clone",
        deps = {
            "src_layer": ParseTimeDependency(
                dep = src_layer,
                providers = [LayerInfo],
            ),
        },
        kwargs = {
            "dst_path": dst_path,
            "src_path": src_path,
        },
    )

clone_record = record(
    src_layer = layer_dep.type,
    src_path = str.type,
    dst_path = str.type,
    omit_outer_dir = bool.type,
    pre_existing_dest = bool.type,
)

def clone_analyze(
        src_path: str.type,
        dst_path: str.type,
        deps: {str.type: "dependency"}) -> FeatureAnalysis.type:
    omit_outer_dir = src_path.endswith("/")
    pre_existing_dest = dst_path.endswith("/")
    if omit_outer_dir and not pre_existing_dest:
        fail(
            "Your `src_path` {} ends in /, which means only the contents " +
            "of the directory will be cloned. Therefore, you must also add a " +
            "trailing / to `dst_path` to signal that clone will write " +
            "inside that pre-existing directory dst_path".format(src_path),
        )

    src_layer = deps["src_layer"]

    return FeatureAnalysis(
        data = clone_record(
            src_layer = layer_dep_analyze(src_layer),
            src_path = src_path,
            dst_path = dst_path,
            omit_outer_dir = omit_outer_dir,
            pre_existing_dest = pre_existing_dest,
        ),
        required_layers = [src_layer[LayerInfo]],
    )
