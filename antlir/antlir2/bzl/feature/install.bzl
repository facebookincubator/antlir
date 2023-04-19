# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:stat.bzl", "stat")
load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

types.lint_noop()

def install(
        *,
        src: types.or_selector(str.type),
        dst: types.or_selector(str.type),
        mode: [int.type, str.type, "selector", None] = None,
        user: types.or_selector(str.type) = "root",
        group: types.or_selector(str.type) = "root",
        separate_debug_symbols: types.or_selector(bool.type) = True) -> ParseTimeFeature.type:
    """
    `install("//path/fs:data", "dir/bar")` installs file or directory `data` to
    `dir/bar` in the image. `dir/bar` must not exist, otherwise the operation
    fails.

    `src` is either a regular file or a directory. If it is a directory, it must
    contain only regular files and directories (recursively).

    `mode` can be automatically determined if `src` is a buck binary, but in all
    other cases is required to be explicitly set by the user.

    See `stat.bzl` for information about how `mode` is interpreted.

    The arguments `user` and `group` change file owner and group of `dst`
    """

    # the default mode is determined later, after we know if the thing being
    # installed is a binary or not
    mode = stat.mode(mode) if mode else None

    return ParseTimeFeature(
        feature_type = "install",
        deps_or_sources = {"src": src},
        kwargs = {
            "dst": dst,
            "group": group,
            "mode": mode,
            "separate_debug_symbols": separate_debug_symbols,
            "user": user,
        },
    )

install_record = record(
    src = "artifact",
    dst = str.type,
    mode = int.type,
    user = str.type,
    group = str.type,
    separate_debug_symbols = bool.type,
)

def install_analyze(
        dst: str.type,
        group: str.type,
        mode: [int.type, None],
        user: str.type,
        separate_debug_symbols: bool.type,
        deps_or_sources: {str.type: ["artifact", "dependency"]}) -> FeatureAnalysis.type:
    src = deps_or_sources["src"]
    if type(src) == "dependency":
        # Unfortunately we can only determine `mode` automatically if the dep is
        # an executable, since a plain source might be a directory
        if not mode and RunInfo in src:
            # There is no need for the old buck1 `install_buck_runnable` stuff
            # in buck2, since we put a dep on the binary directly onto the layer
            # itself, which forces a rebuild when appropriate.
            mode = 0o555

        src = ensure_single_output(src)
    if not mode:
        # We can't tell if a source is a file or directory, so we need to
        # force the user to specify it
        # https://fb.workplace.com/groups/buck2users/posts/3346711265585231
        fail("Unable to automatically determine 'mode'. Please specify it with something like 'mode=\"a+r\"'")
    return FeatureAnalysis(
        data = install_record(
            src = src,
            dst = dst,
            mode = mode,
            user = user,
            group = group,
            separate_debug_symbols = separate_debug_symbols,
        ),
        required_artifacts = [src],
    )
