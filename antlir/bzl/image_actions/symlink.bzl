load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

def _build_symlink_feature(link_target, link_name, symlinks_to_arg):
    symlink_spec = {
        "dest": link_name,
        "source": link_target,
    }
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(**{symlinks_to_arg: [symlink_spec]}),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:symlink"],
    )

def image_symlink_dir(link_target, link_name):
    """
`image.symlink_dir("/a", "/b/c")` symlinks directory `/a` to `/b/c`,

- `link_target` is the image-absolute source file/dir of the symlink.
    This file must exist as we do not support dangling symlinks.

    IMPORTANT: The emitted symlink will be **relative** by default, enabling
    easier inspection if images via `buck-image-out`. If this is a problem
    for you, we can add an `absolute` boolean kwarg.

- `link_name` is an image-absolute path. A trailing / is significant.

    A `link_name` that does NOT end in / is a full path in the new image,
    ending with a filename for the new symlink.

    As with `image.clone`, a traling / means that `link_name` must be a
    pre-existing directory in the image (e.g. created via `image.mkdir`), and
    the actual link will be placed at `link_name/(basename of link_target)`.
    """
    return _build_symlink_feature(link_target, link_name, "symlinks_to_dirs")

def image_symlink_file(link_target, link_name):
    """
`image.symlink_file("/d", "/e/")` symlinks file `/d` to `/e/d` --

- `link_target` is the image-absolute source file/dir of the symlink.
    This file must exist as we do not support dangling symlinks.

    IMPORTANT: The emitted symlink will be **relative** by default, enabling
    easier inspection if images via `buck-image-out`. If this is a problem
    for you, we can add an `absolute` boolean kwarg.

- `link_name` is an image-absolute path. A trailing / is significant.

    A `link_name` that does NOT end in / is a full path in the new image,
    ending with a filename for the new symlink.

    As with `image.clone`, a traling / means that `link_name` must be a
    pre-existing directory in the image (e.g. created via `image.mkdir`), and
    the actual link will be placed at `link_name/(basename of link_target)`.
    """
    return _build_symlink_feature(link_target, link_name, "symlinks_to_files")
