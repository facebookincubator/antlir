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

## Rationale for having `install_buck_runnable`

This API forces you to distinguish between source targets that are
buck-runnable and those that are not, because (until Buck supports
providers), it is not possible to deduce this automatically at parse-time.

The implementation of `install_buck_runnable` differs significantly in
`@mode/dev` in order to support the execution of in-place binaries
(dynamically linked C++, linktree Python) from within an image.  Internal
implementation differences aside, the resulting image should "quack" like
your real, production `@mode/opt`.

[1] Corner case: if you want to copy a non-executable file from inside a
directory output by a Buck-runnable target, then you should use
`install`, even though the underlying rule is executable.
"""

load("//antlir/bzl:add_stat_options.bzl", "add_stat_options")
load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:target_tagger.bzl", "extract_tagged_target", "image_source_as_target_tagged_dict", "new_target_tagger", "tag_and_maybe_wrap_executable_target", "target_tagger_to_feature")

def _forbid_layer_source(source_dict):
    if source_dict["layer"] != None:
        fail(
            "Cannot use image.source(layer=...) with `image.install*` " +
            "actions: {}".format(source_dict),
        )

def image_install_buck_runnable(source, dest, mode = None, user = None, group = None):
    """
`image.install_buck_runnable("//path/fs:exe", "dir/foo")` copies
buck-runnable artifact `exe` to `dir/foo` in the image. Unlike `install`,
this supports only single files -- though you can extract a file from a
buck-runnable directory via `image.source`, see below.

### When to use `install_buck_runnable` vs `install`?

If the file being copied is a buck-runnable (e.g. `cpp_binary`,
`python_binary`), use `install_buck_runnable`. Ditto for copying executable
files from inside directories output by buck-runnable rules. For everything
else, use `install` [1].

Important: failing to use `install_buck_runnable` will cause the installed
binary to be unusable in image tests in @mode/dev.
    """

    target_tagger = new_target_tagger()

    # Normalize to the `image.source` interface
    tagged_source = image_source_as_target_tagged_dict(target_tagger, maybe_export_file(source))
    _forbid_layer_source(tagged_source)

    # NB: We don't have to wrap executables because they already come from a
    # layer, which would have wrapped them if needed.
    if tagged_source["source"]:
        was_wrapped, tagged_source["source"] = tag_and_maybe_wrap_executable_target(
            target_tagger = target_tagger,
            # Peel back target tagging since this helper expects untagged.
            target = extract_tagged_target(tagged_source.pop("source")),
            wrap_prefix = "install_buck_runnable_wrap_source",
            visibility = None,
            # NB: Buck makes it hard to execute something out of an
            # output that is a directory, but it is possible so long as
            # the rule outputting the directory is marked executable
            # (see e.g. `print-ok-too` in `feature_install_files`).
            path_in_output = tagged_source.get("path", None),
        )
        if was_wrapped:
            # The wrapper above has resolved `tagged_source["path"]`, so the
            # compiler does not have to.
            tagged_source["path"] = None

    install_spec = {"dest": dest, "source": tagged_source}
    add_stat_options(install_spec, mode, user, group)

    return target_tagger_to_feature(
        target_tagger,
        items = struct(install_files = [install_spec]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:install"],
    )

def image_install(source, dest, mode = None, user = None, group = None):
    """
`image.install("//path/fs:data", "dir/bar")` installs file or directory
`data` to `dir/bar` in the image.
    """

    target_tagger = new_target_tagger()
    install_spec = {
        "dest": dest,
        "source": image_source_as_target_tagged_dict(target_tagger, maybe_export_file(source)),
    }
    _forbid_layer_source(install_spec["source"])
    add_stat_options(install_spec, mode, user, group)

    # Future: We might use a Buck macro that enforces that the target is
    # non-executable, as I suggested on Q15839. This should probably go in
    # `tag_required_target_key` to ensure that we avoid "unwrapped executable"
    # bugs everywhere.  A possible reason NOT to do this is that it would
    # require fixes to `install` invocations that extract non-executable
    # contents out of a directory target that is executable.
    return target_tagger_to_feature(
        target_tagger,
        items = struct(install_files = [install_spec]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:install"],
    )
