# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:debuginfo.bzl", "SplitBinaryInfo", "split_binary")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:stat.bzl", "stat")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def install(
        *,
        src: [str.type, "selector"],
        dst: [str.type, "selector"],
        mode: [int.type, str.type, "selector", None] = None,
        user: [str.type, "selector"] = "root",
        group: [str.type, "selector"] = "root") -> ParseTimeFeature.type:
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
        deps_or_srcs = {"src": src},
        kwargs = {
            "dst": dst,
            "group": group,
            "mode": mode,
            "user": user,
        },
        analyze_uses_context = True,
    )

installed_binary = record(
    debuginfo = field(["artifact", None], default = None),
    metadata = field(["artifact", None], default = None),
)

binary_record = record(
    dev = field([bool.type, None], default = None),
    installed = field([installed_binary.type, None], default = None),
)

install_record = record(
    src = "artifact",
    dst = str.type,
    mode = int.type,
    user = str.type,
    group = str.type,
    binary_info = field([binary_record.type, None], default = None),
)

def install_analyze(
        ctx: "AnalyzeFeatureContext",
        dst: str.type,
        group: str.type,
        mode: [int.type, None],
        user: str.type,
        deps_or_srcs: {str.type: ["artifact", "dependency"]}) -> FeatureAnalysis.type:
    src = deps_or_srcs["src"]
    binary_info = None
    required_run_infos = []
    required_artifacts = []
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

            # dev mode binaries don't get stripped, they just get symlinked
            if REPO_CFG.artifacts_require_repo:
                src = ensure_single_output(src)
                binary_info = binary_record(
                    dev = True,
                )
            else:
                split_anon_target = ctx.actions.anon_target(split_binary, {
                    "cxx_toolchain": ctx.toolchains.cxx,
                    "name": "debuginfo//:" + ensure_single_output(src).short_path,
                    "src": src,
                })
                binary_info = binary_record(
                    installed = installed_binary(
                        debuginfo = ctx.actions.artifact_promise(split_anon_target.map(lambda x: x[SplitBinaryInfo].debuginfo)),
                        metadata = ctx.actions.artifact_promise(split_anon_target.map(lambda x: x[SplitBinaryInfo].metadata)),
                    ),
                )
                required_artifacts.extend([binary_info.installed.debuginfo, binary_info.installed.metadata])
                src = ctx.actions.artifact_promise(split_anon_target.map(lambda x: x[SplitBinaryInfo].stripped))
        else:
            src = ensure_single_output(src)
            binary_info = None
    elif type(src) == "artifact":
        # If the source is an artifact, that means it was given as an
        # `attrs.source()`, and is thus not a dependency.
        # Buck2 does not allow a user to pass a raw directory as an
        # `attrs.source()`, then we can default the mode to 444
        if mode == None:
            mode = 0o444
    return FeatureAnalysis(
        feature_type = "install",
        data = install_record(
            src = src,
            dst = dst,
            mode = mode,
            user = user,
            group = group,
            binary_info = binary_info,
        ),
        required_artifacts = [src] + required_artifacts,
        required_run_infos = required_run_infos,
    )
