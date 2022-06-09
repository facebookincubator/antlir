# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:new.bzl", "PRIVATE_DO_NOT_USE_feature_target_name")
load("//antlir/bzl/image/feature:symlink.shape.bzl", "symlink_t")
load(":providers.bzl", "feature_provider")

def feature_ensure_symlink_rule_impl(ctx: "context") -> ["provider"]:
    return feature_provider(
        ctx.attr.symlinks_to_arg,
        shape.new(
            symlink_t,
            dest = ctx.attr.link_name,
            source = ctx.attr.link_target,
        ),
    )

feature_ensure_symlink_rule = rule(
    implementation = feature_ensure_symlink_rule_impl,
    attrs = {
        "link_name": attr.string(),
        "link_target": attr.string(),
        "symlinks_to_arg": attr.string(),
    },
)

def feature_ensure_symlink(link_target, link_name, symlink_type):
    name = "LINK_TARGET__{link_target}__LINK_NAME__{link_name}".format(
        link_target = sha256_b64(link_target),
        link_name = sha256_b64(link_name),
    )
    target_name = PRIVATE_DO_NOT_USE_feature_target_name(name + "__feature_" + symlink_type)

    if not native.rule_exists(target_name):
        feature_ensure_symlink_rule(
            name = target_name,
            link_name = link_name,
            link_target = link_target,
            symlinks_to_arg = symlink_type,
        )

    return ":" + target_name

def feature_ensure_dir_symlink(link_target, link_name):
    """
The operation follows rsync convention for a destination (`link_name`):
`ends/in/slash/` means "write into this directory", `does/not/end/with/slash`
means "write with the specified filename":

- `feature.ensure_dir_symlink("/d", "/e/")` symlinks directory `/d` to `/e/d`
- `feature.ensure_dir_symlink("/a", "/b/c")` symlinks directory `/a` to `/b/c`

Both arguments are mandatory:

- `link_target` is the image-absolute source file/dir of the symlink.
    This file must exist as we do not support dangling symlinks.

    IMPORTANT: The emitted symlink will be **relative** by default, enabling
    easier inspection if images via `buck-image-out`. If this is a problem
    for you, we can add an `absolute` boolean kwarg.

- `link_name` is an image-absolute path. A trailing / is significant.

    A `link_name` that does NOT end in / is a full path in the new image,
    ending with a filename for the new symlink.

    As with `image.clone`, a traling / means that `link_name` must be a
    pre-existing directory in the image (e.g. created via
    `image.ensure_dirs_exist`), and the actual link will be placed at
    `link_name/(basename of link_target)`.

This item is indempotent: it is a no-op if a symlink already exists that
matches the spec.
    """
    return feature_ensure_symlink(link_target, link_name, "symlinks_to_dirs")

def feature_ensure_file_symlink(link_target, link_name):
    """
The operation follows rsync convention for a destination (`link_name`):
`ends/in/slash/` means "write into this directory", `does/not/end/with/slash`
means "write with the specified filename":

- `feature.ensure_file_symlink("/d", "/e/")` symlinks file `/d` to `/e/d`
- `feature.ensure_file_symlink("/a", "/b/c")` symlinks file `/a` to `/b/c`

Both arguments are mandatory:

- `link_target` is the image-absolute source file/dir of the symlink.
    This file must exist as we do not support dangling symlinks.

    IMPORTANT: The emitted symlink will be **relative** by default, enabling
    easier inspection if images via `buck-image-out`. If this is a problem
    for you, we can add an `absolute` boolean kwarg.

- `link_name` is an image-absolute path. A trailing / is significant.

    A `link_name` that does NOT end in / is a full path in the new image,
    ending with a filename for the new symlink.

    As with `image.clone`, a traling / means that `link_name` must be a
    pre-existing directory in the image (e.g. created via
    `image.ensure_dirs_exist`), and the actual link will be placed at
    `link_name/(basename of link_target)`.

This item is indempotent: it is a no-op if a symlink already exists that
matches the spec.
    """
    return feature_ensure_symlink(link_target, link_name, "symlinks_to_files")
