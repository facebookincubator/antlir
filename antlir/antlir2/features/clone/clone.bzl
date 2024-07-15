# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:dependency_layer_info.bzl", "layer_dep", "layer_dep_analyze")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def clone(
        *,
        src_layer: str | Select,
        path: str | Select | None = None,
        src_path: str | Select | None = None,
        dst_path: str | Select | None = None,
        user: str | None = None,
        group: str | None = None):
    """
    Copies a subtree of an existing layer into the one under construction. To
    the extent possible, filesystem metadata are preserved.

    This copies from `src_path` in the `src_layer` to `dst_path` in this layer.
    If both paths are the same, you can just set `path` and it will serve as
    both the source and destination paths.

    ### Trailing slashes on both paths are significant

    The three supported cases are:
    - `s/rc` -> `dest/` creates `dest/rc`
    - `s/rc/` -> `dest/` creates `dest/(children of rc)`
    - `s/rc` -> `dest` creates `dest`

    More explicitly:
    - A trailing slash in `src_path` means "use the `rsync` convention":
        * Do not clone the source directory, but only its contents.
        * `dest_path` must be a pre-existing dir, and it must end in `/`
    - A trailing slash in `dst_path` means that it's a
        pre-existing directory (e.g.  made by `ensure_dirs_exist`), and
        `clone` will only write to:
        * `dst/(basename of src_path)` if `src_path` lacks a trailing `/`
        * `dst/(children of src_path)` if `src_path` has a trailing `/`

    ### Known deviations from perfect cloning

    Most likely, SELinux attrs change.

    ### UID/GID remapping

    `src_layer` and the destination layer must have the same user/group _names_
    available, but those names do not need to map to the same ids. uid/gids will
    be remapped to the appropriate numeric id of that user/group in the
    destination layer.

    ### When to use this?

    Often, instead of using this, you should prefer
    [`layer_mount`](#featurelayer_mount), which allows you to compose
    independent pieces of the filesystem at *runtime*, without incurring the
    cost of publishing images with a lot of duplicated content.

    If you're trying to copy the output of a regular Buck target, instead use
    [`feature.install`](#featureinstall).

    Args:
        src_layer: Buck target pointing to source `image.layer`.

            This image must contain the contents to be cloned

        src_path: Root path to clone from in `src_layer`
        dst_path: Root path to clone into in the layer being built
        user: Set owning user on all files and directories

            If not set, the same username is used between `src_layer` and the
            layer being built

        group: Set owning group on all files and directories

            If not set, the same group name is used between `src_layer` and the
            layer being built
    """
    return ParseTimeFeature(
        feature_type = "clone",
        plugin = "antlir//antlir/antlir2/features/clone:clone",
        antlir2_configured_deps = {
            "src_layer": src_layer,
        },
        kwargs = {
            "dst_path": dst_path,
            "group": group,
            "path": path,
            "src_path": src_path,
            "user": user,
        },
    )

clone_usergroup = record(
    user = str,
    group = str,
)

clone_record = record(
    src_layer = layer_dep,
    src_path = str,
    dst_path = str,
    omit_outer_dir = bool,
    pre_existing_dest = bool,
    usergroup = clone_usergroup | None,
)

def _impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.attrs.path and (ctx.attrs.src_path or ctx.attrs.dst_path):
        fail("'path' is mutually exclusive with 'src_path' and 'dst_path'")
    if not ctx.attrs.path and (not ctx.attrs.src_path or not ctx.attrs.dst_path):
        fail("'src_path' and 'dst_path' must be set if 'path' is missing")
    src_path = ctx.attrs.src_path or ctx.attrs.path
    dst_path = ctx.attrs.dst_path or ctx.attrs.path

    omit_outer_dir = src_path.endswith("/")
    pre_existing_dest = dst_path.endswith("/")
    if omit_outer_dir and not pre_existing_dest:
        fail(
            "Your `src_path` {} ends in /, which means only the contents " +
            "of the directory will be cloned. Therefore, you must also add a " +
            "trailing / to `dst_path` to signal that clone will write " +
            "inside that pre-existing directory dst_path".format(src_path),
        )

    usergroup = None
    if ctx.attrs.user and ctx.attrs.group:
        usergroup = clone_usergroup(
            user = ctx.attrs.user,
            group = ctx.attrs.group,
        )
    elif ctx.attrs.user or ctx.attrs.group:
        fail("either none or both of {user, group} must be set")

    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "clone",
            data = clone_record(
                src_layer = layer_dep_analyze(ctx.attrs.src_layer),
                src_path = src_path,
                dst_path = dst_path,
                omit_outer_dir = omit_outer_dir,
                pre_existing_dest = pre_existing_dest,
                usergroup = usergroup,
            ),
            required_artifacts = [ctx.attrs.src_layer[LayerInfo].facts_db],
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

clone_rule = rule(
    impl = _impl,
    attrs = {
        "dst_path": attrs.option(attrs.string(), default = None),
        "group": attrs.option(attrs.string(), default = None),
        "path": attrs.option(attrs.string(), default = None),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "src_layer": attrs.dep(providers = [LayerInfo]),
        "src_path": attrs.option(attrs.string(), default = None),
        "user": attrs.option(attrs.string(), default = None),
    },
)
