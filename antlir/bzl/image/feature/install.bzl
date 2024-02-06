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

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2 = "feature")
load("//antlir/bzl:image_source.bzl", "image_source_to_buck2_src")

def feature_install_buck_runnable(
        source,
        dest,
        mode = None,
        user = "root",
        group = "root",
        separate_debug_symbols = True,
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
    buck2_src = image_source_to_buck2_src(source)

    return antlir2.install(
        src = buck2_src,
        dst = dest,
        mode = mode,
        user = user,
        group = group,
    )

def feature_install(
        source,
        dest,
        mode = None,
        user = "root",
        group = "root",
        separate_debug_symbols = True,
        # buildifier: disable=unused-variable
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
    buck2_src = image_source_to_buck2_src(source)

    return antlir2.install(
        src = buck2_src,
        dst = dest,
        mode = mode,
        user = user,
        group = group,
    )
