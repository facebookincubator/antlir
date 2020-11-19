load("//antlir/bzl:add_stat_options.bzl", "add_stat_options")
load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

def image_mkdir(parent, dest, mode = None, user = None, group = None):
    """
  `image.mkdir("/a/b", "c/d")` creates the directories `c/d` in the image
  inside the pre-existing directory `/a/b` --
    - `parent` is an image-absolute path, inside which the directory will be
      created.
    - `dest` is a path relative to `parent`, which will be created.

  The arguments `parent` and `dest` (`/a/b` and `c/d` in the example above) are
  mandatory; `mode`, `user`, and `group` are optional.

  The argument `mode` changes file mode bits of all directories in `dest`. It
  can be an integer fully specifying the bits or a symbolic string like `u+rx`.
  In the latter case, the changes are applied on top of mode 0.

  The arguments `user` and `group` change file owner and group of all
  directories in `dest`. `user` and `group` can be integers or symbolic strings.
  In the latter case, the passwd/group database from the host (not from the
  image) is used.
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
