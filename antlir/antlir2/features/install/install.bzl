# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:debuginfo.bzl", "split_binary_anon")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "ParseTimeFeature",
)
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:stat.bzl", "stat")

def install(
        *,
        src: str | Select,
        dst: str | Select,
        mode: int | str | Select | None = None,
        user: str | Select = "root",
        group: str | Select = "root",
        xattrs: dict[str, str] | Select = {}) -> ParseTimeFeature:
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
        plugin = antlir2_dep("features/install:install"),
        deps_or_srcs = {"src": src},
        exec_deps = {
            "_objcopy": "fbsource//third-party/binutils:objcopy",
        },
        kwargs = {
            "dst": dst,
            "group": group,
            "mode": mode,
            "text": None,
            "user": user,
            "xattrs": xattrs,
        },
    )

def install_text(
        *,
        text: str | Select,
        dst: str | Select,
        mode: int | str | Select | None = None,
        user: str | Select = "root",
        group: str | Select = "root") -> ParseTimeFeature:
    # the default mode is determined later, after we know if the thing being
    # installed is a binary or not
    mode = stat.mode(mode) if mode != None else None

    return ParseTimeFeature(
        feature_type = "install",
        plugin = antlir2_dep("features/install:install"),
        kwargs = {
            "dst": dst,
            "group": group,
            "mode": mode,
            "text": text,
            "user": user,
        },
    )

installed_binary = record(
    debuginfo = field([Artifact, None], default = None),
    metadata = field([Artifact, None], default = None),
)

binary_record = record(
    dev = field([bool, None], default = None),
    installed = field([installed_binary, None], default = None),
)

def _impl(ctx: AnalysisContext) -> list[Provider]:
    binary_info = None
    required_run_infos = []
    required_artifacts = []
    if not ctx.attrs.src and ctx.attrs.text == None:
        fail("src or text must be set")
    src = ctx.attrs.src
    mode = ctx.attrs.mode
    if ctx.attrs.text != None:
        src = ctx.actions.write("install_text", ctx.attrs.text)
    if type(src) == "dependency":
        if mode == None:
            if RunInfo in src:
                # There is no need for the old buck1 `install_buck_runnable` stuff
                # in buck2, since we put a dep on the binary directly onto the layer
                # itself, which forces a rebuild when appropriate.
                mode = 0o555
            elif ctx.attrs.dst.endswith("/"):
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
            if ctx.attrs.skip_debuginfo_split or REPO_CFG.artifacts_require_repo:
                src = ensure_single_output(src)
                binary_info = binary_record(dev = REPO_CFG.artifacts_require_repo)
            else:
                split_anon_target = split_binary_anon(
                    ctx = ctx,
                    src = src,
                    objcopy = ctx.attrs._objcopy,
                )
                binary_info = binary_record(
                    installed = installed_binary(
                        debuginfo = split_anon_target.artifact("debuginfo"),
                        metadata = split_anon_target.artifact("metadata"),
                    ),
                )
                required_artifacts.extend([binary_info.installed.debuginfo, binary_info.installed.metadata])
                src = split_anon_target.artifact("src")
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
    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "install",
            build_phase = BuildPhase(ctx.attrs.build_phase),
            data = struct(
                src = src,
                dst = ctx.attrs.dst,
                mode = mode,
                user = ctx.attrs.user,
                group = ctx.attrs.group,
                binary_info = binary_info,
                xattrs = ctx.attrs.xattrs,
            ),
            required_artifacts = [src] + required_artifacts,
            required_run_infos = required_run_infos,
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

install_rule = rule(
    impl = _impl,
    attrs = {
        "build_phase": attrs.enum(BuildPhase.values(), default = "compile"),
        "dst": attrs.option(attrs.string(), default = None),
        "group": attrs.string(default = "root"),
        "mode": attrs.option(attrs.int(), default = None),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "skip_debuginfo_split": attrs.bool(default = False),
        "src": attrs.option(
            attrs.one_of(attrs.dep(), attrs.source()),
            default = None,
        ),
        "text": attrs.option(attrs.string(), default = None),
        "user": attrs.string(default = "root"),
        "xattrs": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "_objcopy": attrs.option(attrs.exec_dep(), default = None),
    },
)
