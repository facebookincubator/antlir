load("//antlir/bzl:add_stat_options.bzl", "add_stat_options")
load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

def image_ensure_dir_exists(path, mode = None, user = None, group = None):
    """
  `image.ensure_dir_exists("/a/b/c")` creates the directories `/a/b/c` in the
  image.

  The argument `path` is mandatory; `mode`, `user`, and `group` are optional.

  The argument `mode` changes file mode bits of all directories in `path. It can
  be an integer fully specifying the bits or a symbolic string like `u+rx`. In
  the latter case, the changes are applied on top of mode 0.

  The arguments `user` and `group` change file owner and group of all
  directories in `path`. `user` and `group` can be integers or symbolic strings.
  In the latter case, the passwd/group database from the host (not from the
  image) is used.

  NB: If any directories in `path` already exist in the image, they will be
  checked to ensure their attributes match the attributes provided here. If any
  do not match (i.e. they were created by another image feature with different
  attributes), the build will fail.
    """

    dir_spec = {"path": path}
    add_stat_options(dir_spec, mode, user, group)
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(ensure_dir_exists = [dir_spec]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:ensure_dir_exists"],
    )
