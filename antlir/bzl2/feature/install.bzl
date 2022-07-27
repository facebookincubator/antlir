# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT

"""
## Usage of `install_*` actions

The object to be installed is specified using `image.source` syntax, except
that `layer=` is prohibited (use `image.clone` instead, to be implemented).
Docs are in `image_source.bzl`, but briefly: target paths, repo file paths,
and `image.source` objects are accepted.  The latter form is useful for
extracting a part of a directory output.

The source must not contains anything but regular files or directories.

`stat (2)` attributes of the source are NOT preserved.  Rather, they are set
uniformly, as follows.

Ownership can be set via the kwargs `user` and `group`, with these defaults:
    user = "root"
    group = "root"

Mode for single source files:
    mode = "a+rx" if it is executable by the Buck repo user, "a+r" otherwise

Mode in directory sources:
    dir_mode = "u+rwx,og+rx" (used for directories)
    exe_mode = "a+rx" (used for source files executable by the Buck repo user)
    data_mode = "a+r" (used for other source files)

Directories are currently left as writable since adding files seems natural,
but we may later reconsider the default (and patch existing users).

Prefer to omit the above kwargs instead of repeating the defaults.

`dest` must be an image-absolute path, including a filename for the file being
copied. The parent directory of `dest` must get created by another image
feature.
"""

load("//antlir/bzl:image_source.bzl", "image_source")
load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:oss_shim.bzl", "is_buck2")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:stat.bzl", "stat")
load("//antlir/bzl:target_tagger.shape.bzl", image_source_t = "target_tagged_image_source_t")
load("//antlir/bzl/image/feature:install.shape.bzl", "install_files_t")
load("//antlir/bzl2:generate_feature_target_name.bzl", "generate_feature_target_name")
load("//antlir/bzl2:image_source_helper.bzl", "normalize_target_and_mark_path_in_source_dict")
load("//antlir/bzl2:providers.bzl", "ItemInfo")

def _forbid_layer_source(source_dict):
    if source_dict["layer"] != None:
        fail(
            "Cannot use image.source(layer=...) with `feature.install*` " +
            "actions: {}".format(source_dict),
        )

def _generate_shape(source_dict, dest, mode, user, group):
    return install_files_t(
        dest = dest,
        source = image_source_t(**source_dict),
        mode = stat.mode(mode) if mode else None,
        user = user,
        group = group,
    )

def _install_rule_impl(ctx):
    if ctx.attrs.is_executable and ctx.attrs.unwrapped_target[native.RunInfo]:
        install_shape = ctx.attrs.wrapped_shape
    else:
        install_shape = ctx.attrs.unwrapped_shape

    return [
        native.DefaultInfo(),
        ItemInfo(items = struct(**{ctx.attrs.key: [install_shape]})),
    ]

_install_rule = native.rule(
    impl = _install_rule_impl,
    attrs = {
        "is_executable": native.attrs.bool(),
        "key": native.attrs.string(),

        # for query
        "type": native.attrs.string(default = "image_feature"),
        "unwrapped_shape": native.attrs.dict(native.attrs.string(), native.attrs.any()),
        "unwrapped_target": native.attrs.dep(),
        "wrapped_shape": native.attrs.dict(native.attrs.string(), native.attrs.any()),
        "wrapped_target": native.attrs.dep(),
    },
) if is_buck2() else None

def maybe_add_install_rule(
        unwrapped_shape,
        wrapped_shape,
        unwrapped_target,
        wrapped_target,
        is_executable,
        include_in_target_name = None,
        debug = False):
    name = "install"
    key = "install_files"

    target_name = generate_feature_target_name(
        name = name,
        key = key,
        feature_shape = unwrapped_shape,
        include_in_name = include_in_target_name if debug else None,
    )

    if not native.rule_exists(target_name):
        _install_rule(
            name = target_name,
            key = key,
            unwrapped_shape = shape.as_serializable_dict(unwrapped_shape),
            wrapped_shape = shape.as_serializable_dict(wrapped_shape),
            unwrapped_target = unwrapped_target,
            wrapped_target = wrapped_target,
            is_executable = is_executable,
        )

    return ":" + target_name

def feature_install_buck_runnable(
        source,
        dest,
        mode = None,
        user = shape.DEFAULT_VALUE,
        group = shape.DEFAULT_VALUE,
        runs_in_build_steps_causes_slow_rebuilds = False):
    """
    Deprecated. Now merged with feature_install.
    """
    return feature_install(
        source,
        dest,
        mode,
        user,
        group,
        runs_in_build_steps_causes_slow_rebuilds,
    )

def feature_install(
        source,
        dest,
        mode = None,
        user = shape.DEFAULT_VALUE,
        group = shape.DEFAULT_VALUE,
        is_executable = True,
        runs_in_build_steps_causes_slow_rebuilds = False):
    """
    `feature.install("//path/fs:data", "dir/bar")` installs file or directory
    `data` to `dir/bar` in the image. `dir/bar` must not exist, otherwise
    the operation fails.

    The arguments `source` and `dest` are mandatory; `mode`, `user`, and `group`
    are optional.

    `source` is either a regular file or a directory. If it is a directory, it
    must contain only regular files and directories (recursively).

    `mode` can be used only if `source` is a regular file.

     - If set, it changes file mode bits of `dest` (after installation of
        `source` to `dest`). `mode` can be an integer fully specifying the bits
        or a symbolic string like `u+rx`. In the latter case, the changes are
        applied on top of mode 0.
     - If not set, the mode of `source` is ignored, and instead the mode of
        `dest` (and all files and directories inside the `dest` if it is a
        directory) is set according to the following rule: "u+rwx,og+rx" for
        directories, "a+rx" for files executable by the Buck repo user, "a+r"
        for other files.

    The arguments `user` and `group` change file owner and group of all
    directories in `dest`. `user` and `group` can be integers or symbolic
    strings. In the latter case, the passwd/group database from the host (not
    from the image) is used. The default for `user` and `group` is `root`.

    `is_executable` - Ignore unless you are installing a non-executable file
    created by a genrule, in which case it needs to be set to `False`. This is
    necessary because there is no way for us to determine if a target with a
    `RunInfo` provider refers to a file that is non-executable, so we just
    assume it is executable.

    Only set `runs_in_build_steps_causes_slow_rebuilds = True` if you get a
    build-time error requesting it.  This flag allows the target being wrapped
    to be executed in an Antlir container as part of a Buck build step.  It
    defaults to `False` to speed up incremental rebuilds.
    """
    source_dict = shape.as_dict_shallow(image_source(maybe_export_file(source)))
    _forbid_layer_source(source_dict)

    unwrapped_source_dict, unwrapped_target = \
        normalize_target_and_mark_path_in_source_dict(dict(source_dict))
    wrapped_source_dict, wrapped_target = \
        normalize_target_and_mark_path_in_source_dict(
            dict(source_dict),
            is_buck_runnable = True,
            runs_in_build_steps_causes_slow_rebuilds =
                runs_in_build_steps_causes_slow_rebuilds,
        )

    return maybe_add_install_rule(
        include_in_target_name = {
            "dest": dest,
            "source": unwrapped_source_dict["source"],
        },
        unwrapped_shape = _generate_shape(
            unwrapped_source_dict,
            dest,
            mode,
            user,
            group,
        ),
        wrapped_shape = _generate_shape(
            wrapped_source_dict,
            dest,
            mode,
            user,
            group,
        ),
        unwrapped_target = unwrapped_target,
        wrapped_target = wrapped_target,
        is_executable = is_executable,
    )
