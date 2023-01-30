# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

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

load("//antlir/buck2/bzl:buck2_early_adoption.bzl", "buck2_early_adoption")
load("//antlir/buck2/bzl/feature:install.bzl?v2_only", buck2_install = "install")
load("//antlir/bzl:dummy_rule.bzl", "dummy_rule")
load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:stat.bzl", "stat")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep", "wrap_target")
load(
    "//antlir/bzl:target_tagger.bzl",
    "extract_tagged_target",
    "image_source_as_target_tagged_dict",
    "new_target_tagger",
    "tag_and_maybe_wrap_executable_target",
    "target_tagger_to_feature",
)
load("//antlir/bzl:target_tagger.shape.bzl", "target_tagged_image_source_t")
load(":install.shape.bzl", "install_files_t")

_BUCK_RUNNABLE_WRAP_SUFFIX = "install_buck_runnable_wrap_source"

def _forbid_layer_source(source_dict):
    if source_dict["layer"] != None:
        fail(
            "Cannot use image.source(layer=...) with `feature.install*` " +
            "actions: {}".format(source_dict),
        )

def _generate_shape(source_dict, dest, mode, user, group, separate_debug_symbols):
    return install_files_t(
        dest = dest,
        source = target_tagged_image_source_t(**source_dict),
        mode = stat.mode(mode) if mode else None,
        user = user,
        group = group,
        separate_debug_symbols = separate_debug_symbols,
    )

def _install_target_tagger(
        dest,
        target_tagger,
        unwrapped_target,
        unwrapped_shape,
        wrapped_target,
        wrapped_shape):
    return target_tagger_to_feature(
        target_tagger,
        items = struct(install_files = [wrapped_shape if wrapped_shape else unwrapped_shape]),
    )

# KEEP IN SYNC with its partial copy in `compiler/tests/sample_items.py`
def TEST_ONLY_wrap_buck_runnable(target, path_in_output):
    return wrap_target(target, _BUCK_RUNNABLE_WRAP_SUFFIX + path_in_output)[1]

def feature_install_buck_runnable(
        source,
        dest,
        mode = None,
        user = "root",
        group = "root",
        separate_debug_symbols = False,
        runs_in_build_steps_causes_slow_rebuilds = False):
    """
`feature.install_buck_runnable("//path/fs:exe", "dir/foo")` copies
buck-runnable artifact `exe` to `dir/foo` in the image. Unlike `install`,
this supports only single files -- though you can extract a file from a
buck-runnable directory via `image.source`, see below.

See **`install`** for documentation on arguments `mode`, `user`, and `group`.

### When to use `install_buck_runnable` vs `install`?

If the file being copied is a buck-runnable (e.g. `cpp_binary`,
`python_binary`), use `install_buck_runnable`. Ditto for copying executable
files from inside directories output by buck-runnable rules. For everything
else, use `install` [1].

Important: failing to use `install_buck_runnable` will cause the installed
binary to be unusable in image tests or `=container` targets in @mode/dev.

Only set `runs_in_build_steps_causes_slow_rebuilds = True` if you get a
build-time error requesting it.  This flag allows the target being wrapped
to be executed in an Antlir container as part of a Buck build step.  It
defaults to `False` to speed up incremental rebuilds.
    """
    if buck2_early_adoption.is_early_adopter():
        return buck2_install(
            src = source,
            dst = dest,
            mode = mode,
            user = user,
            group = group,
        )

    target_tagger = new_target_tagger()

    # Normalize to the `image.source` interface
    tagged_source = image_source_as_target_tagged_dict(target_tagger, maybe_export_file(source))
    _forbid_layer_source(tagged_source)

    unwrapped_target = extract_tagged_target(tagged_source["source"])
    unwrapped_shape = _generate_shape(tagged_source, dest, mode, user, group, separate_debug_symbols)

    # NB: We don't have to wrap executables because they already come from a
    # layer, which would have wrapped them if needed.
    if tagged_source["source"]:
        was_wrapped, tagged_source["source"] = tag_and_maybe_wrap_executable_target(
            target_tagger = target_tagger,
            # Peel back target tagging since this helper expects untagged.
            target = extract_tagged_target(tagged_source.pop("source")),
            wrap_suffix = _BUCK_RUNNABLE_WRAP_SUFFIX + (tagged_source.get("path") or ""),
            visibility = None,
            # NB: Buck makes it hard to execute something out of an
            # output that is a directory, but it is possible so long as
            # the rule outputting the directory is marked executable
            # (see e.g. `print-ok-too` in `feature_install_files`).
            path_in_output = tagged_source.get("path", None),
            runs_in_build_steps_causes_slow_rebuilds = runs_in_build_steps_causes_slow_rebuilds,
        )
        if was_wrapped:
            # The wrapper above has resolved `tagged_source["path"]`, so the
            # compiler does not have to.
            tagged_source["path"] = None

    wrapped_target = extract_tagged_target(tagged_source["source"])
    wrapped_shape = _generate_shape(tagged_source, dest, mode, user, group, separate_debug_symbols)

    return _install_target_tagger(
        dest,
        target_tagger,
        unwrapped_target,
        unwrapped_shape,
        wrapped_target,
        wrapped_shape,
    )

def feature_install(
        source,
        dest,
        mode = None,
        user = "root",
        group = "root",
        separate_debug_symbols = False,
        # @lint-ignore BUILDIFIERLINT
        wrap_as_buck_runnable = False):
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

The argument `wrap_as_buck_runnable` is only present because the Buck2
implementation uses that argument, and adding it here makes it easier to
integrate with that logic. It can be ignored.
    """
    if buck2_early_adoption.is_early_adopter():
        return buck2_install(
            src = source,
            dst = dest,
            mode = mode,
            user = user,
            group = group,
        )

    target_tagger = new_target_tagger()
    source_dict = image_source_as_target_tagged_dict(
        target_tagger,
        maybe_export_file(source),
    )
    _forbid_layer_source(source_dict)

    unwrapped_target = extract_tagged_target(source_dict["source"])
    unwrapped_shape = _generate_shape(source_dict, dest, mode, user, group, separate_debug_symbols)

    wrapped_target = dummy_rule(
        wrap_target(unwrapped_target, _BUCK_RUNNABLE_WRAP_SUFFIX + (source_dict.get("path") or ""))[1],
        deps = [
            antlir_dep(":repo-root"),
            unwrapped_target,
        ],
    )

    # Future: We might use a Buck macro that enforces that the target is
    # non-executable, as I suggested on Q15839. This should probably go in
    # `tag_required_target_key` to ensure that we avoid "unwrapped executable"
    # bugs everywhere.  A possible reason NOT to do this is that it would
    # require fixes to `install` invocations that extract non-executable
    # contents out of a directory target that is executable.
    return _install_target_tagger(
        dest,
        target_tagger,
        unwrapped_target,
        unwrapped_shape,
        wrapped_target,
        None,
    )
