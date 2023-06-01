# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
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
    mode = stat.mode(mode) if mode != None else None

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
    dev_mode = bool.type,
)

def install_analyze(
        dst: str.type,
        group: str.type,
        mode: [int.type, None],
        user: str.type,
        separate_debug_symbols: bool.type,
        deps_or_sources: {str.type: ["artifact", "dependency"]}) -> FeatureAnalysis.type:
    src = deps_or_sources["src"]
    dev_mode = False
    required_run_infos = []
    if type(src) == "dependency":
        if mode == None:
            if RunInfo in src:
                # There is no need for the old buck1 `install_buck_runnable` stuff
                # in buck2, since we put a dep on the binary directly onto the layer
                # itself, which forces a rebuild when appropriate.
                mode = 0o555
            elif dst.endswith("/"):
                # If the user is installing a directory, we require they include
                # a trailing '/' in `dst`
                mode = 0o755
            else:
                mode = 0o444

        if RunInfo in src:
            # depending on the RunInfo ensures that all the dynamic library
            # dependencies of this binary are made available on the local
            # machine
            required_run_infos.append(src[RunInfo])
            if REPO_CFG.artifacts_require_repo:
                dev_mode = True

        src = ensure_single_output(src)
    elif type(src) == "artifact":
        # If the source is an artifact, that means it was given as an
        # `attrs.source()`, and is thus not a dependency.
        # Buck2 does not allow a user to pass a raw directory as an
        # `attrs.source()`, then we can default the mode to 444
        if mode == None:
            mode = 0o444
    return FeatureAnalysis(
        data = install_record(
            src = src,
            dst = dst,
            mode = mode,
            user = user,
            group = group,
            separate_debug_symbols = separate_debug_symbols,
            dev_mode = dev_mode,
        ),
        required_artifacts = [src],
        required_run_infos = required_run_infos,
    )
