"""
`image.symlink_dir("/a", "/b/c")` symlinks directory `/a` to `/b/c`,
`image.symlink_file("/d", "/e/f")` symlinks file `/d` to `/e/f` --
  - `link_target` is the source file/dir of the symlink.  This file must
     exist as we do not support dangling symlinks.
  - `link_name` is an image-absolute path.  We follow the `rsync`
     convention -- if `dest` ends with a slash, the copy will be at
     `dest/output` filename of source.  Otherwise, `dest` is a full
     path, including a new filename for the target's output.  The
     directory of `dest` must get created by another image feature.
"""

load("//fs_image/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

def _build_symlink_feature(link_target, link_name, symlinks_to_arg):
    symlink_spec = {
        "dest": link_name,
        "source": link_target,
    }
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(**{symlinks_to_arg: [symlink_spec]}),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//fs_image/bzl/image_actions:symlink"],
    )

def image_symlink_dir(link_target, link_name):
    return _build_symlink_feature(link_target, link_name, "symlinks_to_dirs")

def image_symlink_file(link_target, link_name):
    return _build_symlink_feature(link_target, link_name, "symlinks_to_files")
