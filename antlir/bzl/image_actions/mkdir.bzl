load("//antlir/bzl:add_stat_options.bzl", "add_stat_options")
load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

def image_mkdir(parent, dest, mode = None, user = None, group = None):
    """
  `image.mkdir("/a/b", "c/d")` makes the directories `c/d` in the image
  inside the pre-existing directory `/a/b` --
    - `parent` is an image-absolute path, inside which the directory will be
      created.
    - `dest` is a path relative to `parent`, which will be created.
    """

    dir_spec = {
        "into_dir": parent,
        "path_to_make": dest,
    }
    add_stat_options(dir_spec, mode, user, group)
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(make_dirs = [dir_spec]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:mkdir"],
    )
