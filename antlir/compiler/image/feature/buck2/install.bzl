# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:add_stat_options.bzl", "add_stat_options")
load("//antlir/bzl:image_source.bzl", "image_source")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load(":helpers.bzl", "generate_feature_target_name")
load(":image_source.shape.bzl", "image_source_t")
load(":install.shape.bzl", "install_files_t")
load(":providers.bzl", "feature_provider")

def _forbid_layer_source(source_dict):
    if source_dict["layer"] != None:
        fail(
            "Cannot use image.source(layer=...) with `feature.install*` " +
            "actions: {}".format(source_dict),
        )

def feature_install_rule_impl(ctx: "context") -> ["provider"]:
    source_dict = shape.as_dict_shallow(image_source(ctx.attr.source_str))
    source_dict["source"] = {"__BUCK_TARGET": source_dict["source"]}
    _forbid_layer_source(source_dict)

    install_spec = {
        "dest": ctx.attr.dest,
        "source": shape.new(image_source_t, **source_dict),
    }
    add_stat_options(install_spec, ctx.attr.mode or None, ctx.attr.user or None, ctx.attr.group or None)

    return feature_provider(
        "install_files",
        shape.new(install_files_t, **install_spec),
    )

feature_install_rule = rule(
    implementation = feature_install_rule_impl,
    attrs = {
        "dest": attr.string(),
        "group": attr.string(default = ""),
        "mode": attr.string(default = ""),
        "source": attr.source(),
        "source_str": attr.string(),
        "type": attr.string(default = "image_feature"),  # for query
        "user": attr.string(default = ""),
    },
)

def feature_install(source, dest, mode = None, user = None, group = None):
    """
`feature.install("//path/fs:data", "dir/bar")` installs file or directory
`data` to `dir/bar` in the image. `dir/bar` must not exist, otherwise
the operation fails.

The arguments `source` and `dest` are mandatory; `mode`, `user`, and `group` are
optional.

`source` is either a regular file or a directory. If it is a directory, it must
contain only regular files and directories (recursively).

`mode` can be used only if `source` is a regular file.

 - If set, it changes file mode bits of `dest` (after installation of `source`
to `dest`). `mode` can be an integer fully specifying the bits or a symbolic
string like `u+rx`. In the latter case, the changes are applied on top of
mode 0.
 - If not set, the mode of `source` is ignored, and instead the mode of `dest`
(and all files and directories inside the `dest` if it is a directory) is set
according to the following rule: "u+rwx,og+rx" for directories, "a+rx" for files
executable by the Buck repo user, "a+r" for other files.

The arguments `user` and `group` change file owner and group of all
directories in `dest`. `user` and `group` can be integers or symbolic strings.
In the latter case, the passwd/group database from the host (not from the
image) is used. The default for `user` and `group` is `root`.
    """
    target_name = generate_feature_target_name(
        name = "install",
        include_in_name = {
            "dest": dest,
            "source": source,
        },
        include_only_in_hash = {
            "group": group,
            "mode": mode,
            "user": user,
        },
    )

    if not native.rule_exists(target_name):
        feature_install_rule(
            name = target_name,
            source = source,
            source_str = normalize_target(source),
            dest = dest,
            mode = mode,
            user = user,
            group = group,
        )

    return ":" + target_name
