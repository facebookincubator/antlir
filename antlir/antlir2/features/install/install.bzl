# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//python:python.bzl", "PythonLibraryInfo")
load("@prelude//utils:expect.bzl", "expect")
load("//antlir/antlir2/bzl:binaries_require_repo.bzl", "binaries_require_repo")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:debuginfo.bzl", "split_binary_anon")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "MultiFeatureAnalysis",
    "ParseTimeFeature",
)
load("//antlir/antlir2/features/rpm:rpm.bzl", "rpms_rule")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:internal_external.bzl", "internal_external")
load("//antlir/bzl:stat.bzl", "stat")
load("//antlir/bzl:types.bzl", "types")

default_permissions = record(
    binary = field(int | None, default = None),
    file = field(int | None, default = None),
    directory = field(int | None, default = None),
)

transition_to_distro_platform = enum("no", "yes", "yes-without-rpm-deps")
_transition_to_distro_platform_enum = transition_to_distro_platform

def install(
        *,
        src: str | Select,
        dst: str | Select,
        mode: int | str | Select | None = None,
        user: str | int | Select = "root",
        group: str | int | Select = "root",
        xattrs: dict[str, str] | Select = {},
        never_use_dev_binary_symlink: bool = False,
        split_debuginfo: bool = True,
        always_use_gnu_debuglink: bool = False,
        setcap: str | None = None,
        default_permissions: default_permissions = default_permissions(),
        ignore_symlink_tree: bool = False,
        transition_to_distro_platform: transition_to_distro_platform | str | bool = False):
    """
    Install a file or directory into the image.

    Arguments:
        src: source file or buck target
        mode: mode to set on the installed file

            In most cases this can be left unset and antlir2 will choose the
            most reasonable default based on the source.
            Buck-built binaries will automatically be marked as executable.
            These defaults can be manually overriden by `default_permissions`.

        default_permissions: Default fallback permissions when mode is unset

        user: owner of the installed contents
        group: owner of the installed contents
        xattrs: extended attributes to set on the installed contents
        never_use_dev_binary_symlink: always install a binary as a regular file

            In most cases this means that your binary will not be runnable in
            `@mode/dev` builds, but it guarantees that the binary will always be
            a regular file and never a symlink.

        split_debuginfo: strip debuginfo from the binary and place it into
            `/usr/lib/debug`

        setcap: add file capabilities to the installed file

            Specified in the form described in `cap_from_text(3)`.

        transition_to_distro_platform: build 'src' for the image distro

            Transition the 'src' build target platform to match the distro of
            this image, including linking against distro-provided third-party
            deps.
            If 'yes', any RPM dependencies of 'src' will be determined and
            installed automatically.
            If 'yes-without-rpm-deps', 'src' will be reconfigured for the distro
            platform, but there will be no attempt to determine and install RPM
            dependencies that 'src' may need.
    """

    # the default mode is determined later, after we know if the thing being
    # installed is a binary or not
    mode = stat.mode(mode) if mode != None else None

    if setcap and not never_use_dev_binary_symlink:
        fail("setcap does not work on dev mode binaries. You must set never_use_dev_binary_symlink=True")

    if always_use_gnu_debuglink and not split_debuginfo:
        fail("always_use_gnu_debuglink requires split_debuginfo=True")

    exec_deps = {
        "_objcopy": internal_external(
            fb = "fbsource//third-party/binutils:objcopy",
            oss = "toolchains//:objcopy",
        ),
    }
    deps = {}
    deps_or_srcs = {}
    distro_platform_deps = {}

    if types.is_bool(transition_to_distro_platform):
        transition_to_distro_platform = _transition_to_distro_platform_enum("yes" if transition_to_distro_platform else "no")
    if types.is_string(transition_to_distro_platform):
        transition_to_distro_platform = _transition_to_distro_platform_enum(transition_to_distro_platform)

    if transition_to_distro_platform == _transition_to_distro_platform_enum("no"):
        deps_or_srcs["src"] = src
    elif transition_to_distro_platform == _transition_to_distro_platform_enum("yes"):
        distro_platform_deps["src"] = src
        exec_deps["_rpm_find_requires"] = "antlir//antlir/distro/rpm:find-requires"
        exec_deps["_rpm_plugin"] = "antlir//antlir/antlir2/features/rpm:rpm"
        exec_deps["_rpm_plan"] = "antlir//antlir/antlir2/features/rpm:plan"
        deps["_rpm_driver"] = "antlir//antlir/antlir2/features/rpm:driver"
    elif transition_to_distro_platform == _transition_to_distro_platform_enum("yes-without-rpm-deps"):
        distro_platform_deps["src"] = src
    else:
        fail("impossible transition_to_distro_platform value '{}'".format(transition_to_distro_platform))

    return ParseTimeFeature(
        feature_type = "install",
        plugin = "antlir//antlir/antlir2/features/install:install",
        deps_or_srcs = deps_or_srcs,
        deps = deps,
        distro_platform_deps = distro_platform_deps,
        exec_deps = exec_deps,
        kwargs = {
            "always_use_gnu_debuglink": always_use_gnu_debuglink,
            "default_binary_mode": default_permissions.binary,
            "default_directory_mode": default_permissions.directory,
            "default_file_mode": default_permissions.file,
            "dst": dst,
            "group": group,
            "ignore_symlink_tree": ignore_symlink_tree,
            "mode": mode,
            "never_use_dev_binary_symlink": never_use_dev_binary_symlink,
            "setcap": setcap,
            "split_debuginfo": split_debuginfo,
            "text": None,
            "transition_to_distro_platform": transition_to_distro_platform.value,
            "user": user,
            "xattrs": xattrs,
            "_binaries_require_repo": binaries_require_repo.select_value,
        },
    )

def install_text(
        *,
        text: str | Select,
        dst: str | Select,
        mode: int | str | Select | None = None,
        user: str | int | Select = "root",
        group: str | int | Select = "root",
        xattrs: dict[str, str] | Select = {}):
    # the default mode is determined later, after we know if the thing being
    # installed is a binary or not
    mode = stat.mode(mode) if mode != None else None

    return ParseTimeFeature(
        feature_type = "install",
        plugin = "antlir//antlir/antlir2/features/install:install",
        kwargs = {
            "dst": dst,
            "group": group,
            "mode": mode,
            "text": text,
            "user": user,
            "xattrs": xattrs,
        },
    )

installed_binary = record(
    debuginfo = field([Artifact, None], default = None),
    dwp = field([Artifact, None], default = None),
    metadata = field([Artifact, None], default = None),
)

binary_record = record(
    dev = field([bool, None], default = None),
    installed = field([installed_binary, None], default = None),
)

shared_library_record = record(
    soname = field(str),
    target = field(Artifact),
)

shared_libraries_record = record(
    so_targets = field(list[shared_library_record]),
    dir_name = field(str),
)

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    binary_info = None
    required_run_infos = []
    required_artifacts = []
    shared_libraries = None
    if not ctx.attrs.src and ctx.attrs.text == None:
        fail("src or text must be set")
    src = ctx.attrs.src
    mode = ctx.attrs.mode
    if ctx.attrs.text != None:
        src = ctx.actions.write("install_text", ctx.attrs.text)

    # If the user is installing a directory, we require they include a trailing
    # '/' in `dst` because there is otherwise no way to tell
    dst_is_dir = ctx.attrs.dst.endswith("/")

    if isinstance(src, Dependency):
        is_executable = RunInfo in src
        expect(LayerInfo not in src, "Layers ({}) cannot be used as install `src`, consider using feature.mount instead".format(src.label))
        if mode == None:
            if is_executable:
                # There is no need for the old buck1 `install_buck_runnable` stuff
                # in buck2, since we put a dep on the binary directly onto the layer
                # itself, which forces a rebuild when appropriate.
                mode = ctx.attrs.default_binary_mode or 0o555
            elif dst_is_dir:
                mode = ctx.attrs.default_directory_mode or 0o755
            else:
                mode = ctx.attrs.default_file_mode or 0o444

        if is_executable:
            # depending on the RunInfo ensures that all the dynamic library
            # dependencies of this binary are made available on the local
            # machine
            required_run_infos.append(src[RunInfo])

        src_subtargets = ctx.attrs.src[DefaultInfo].sub_targets
        if "rpath-tree" in src_subtargets and not ctx.attrs.ignore_symlink_tree:
            rpath_tree_info = src_subtargets["rpath-tree"][DefaultInfo]
            rpath_tree_out = ensure_single_output(rpath_tree_info)
            required_artifacts.append(rpath_tree_out)

            so_targets = []
            for soname, so_subtarget in src_subtargets["shared-libraries"][DefaultInfo].sub_targets.items():
                so_info = so_subtarget[DefaultInfo]
                so_out = ensure_single_output(so_info)
                so_targets.append(shared_library_record(
                    soname = soname,
                    target = so_out,
                ))
                required_artifacts.append(so_out)

            shared_libraries = shared_libraries_record(
                so_targets = so_targets,
                dir_name = rpath_tree_out.basename,
            )

        # Determining if a binary is standalone or not is surprisingly hard:
        #
        # Default to whatever we know about the entire build mode (opt or dev).
        # If we can't tell from the build mode, assume that binaries are not
        # standalone to be safe.
        standalone = not ctx.attrs._binaries_require_repo if ctx.attrs._binaries_require_repo != None else False

        # However, an individual binary may still be standalone, so let's check
        # the binary instead of solely relying on the mode of the entire build.
        # We trust the build mode more than inspecting individual binaries, so
        # we never want to "downgrade" a binary to non-standalone status if the
        # build mode indicates that every binary is in fact standalone
        if not standalone:
            standalone = binaries_require_repo.is_standalone(src)

        # Non-standalone (aka dev-mode) binaries don't get stripped, they just
        # get symlinked. The split action does not (currently) support directory
        # sources, so just skip it
        if not dst_is_dir and ctx.attrs.split_debuginfo and (standalone or ctx.attrs.never_use_dev_binary_symlink):
            split_anon_target = split_binary_anon(
                ctx = ctx,
                src = src,
                objcopy = ctx.attrs._objcopy,
            )
            binary_info = binary_record(
                installed = installed_binary(
                    debuginfo = split_anon_target.artifact("debuginfo"),
                    metadata = split_anon_target.artifact("metadata"),
                    dwp = split_anon_target.artifact("dwp"),
                ),
            )
            required_artifacts.extend([
                binary_info.installed.debuginfo,
                binary_info.installed.metadata,
                binary_info.installed.dwp,
            ])
            src = split_anon_target.artifact("src")
        else:
            src = ensure_single_output(src)
            binary_info = None
            if is_executable:
                if not standalone:
                    binary_info = binary_record(dev = True)
                if ctx.attrs.never_use_dev_binary_symlink:
                    binary_info = None
            elif ctx.attrs.setcap:
                fail("install src {} is not a binary, setcap should not be used".format(ctx.attrs.src))
    elif type(src) == "artifact":
        # If the source is an artifact, that means it was given as an
        # `attrs.source()`, and is thus not a dependency.
        # Buck2 does not allow a user to pass a raw directory as an
        # `attrs.source()`, then we can default the mode to 444
        if mode == None:
            mode = 0o444

    install_feature = FeatureAnalysis(
        buck_only_data = struct(
            original_source = ctx.attrs.src,
        ),
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
            setcap = ctx.attrs.setcap,
            always_use_gnu_debuglink = ctx.attrs.always_use_gnu_debuglink,
            shared_libraries = shared_libraries,
        ),
        required_artifacts = [src] + required_artifacts,
        required_run_infos = required_run_infos,
        plugin = ctx.attrs.plugin[FeaturePluginInfo],
    )

    if ctx.attrs.transition_to_distro_platform != "yes":
        return [
            DefaultInfo(),
            install_feature,
        ]

    # the rpm dependency finder can only find native dependencies, it doesn't
    # understand things like PEX "binaries"
    extra_rpm_deps = []
    if isinstance(ctx.attrs.src, Dependency):
        # this is just an easy way to guess if 'src' is a python_binary based on
        # the limited information we have at this point
        if "library-info" in ctx.attrs.src[DefaultInfo].sub_targets and PythonLibraryInfo in ctx.attrs.src.sub_target("library-info"):
            extra_rpm_deps += ["python3", "unzip"]

    # otherwise we need to produce an rpm feature too
    rpm_subjects = ctx.actions.declare_output("rpm_requires.txt")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._rpm_find_requires[RunInfo],
            rpm_subjects.as_output(),
            src,
            extra_rpm_deps,
        ),
        category = "rpm_find_requires",
    )
    anon_rpm_target = ctx.actions.anon_target(
        rpms_rule,
        {
            "action": "install",
            "driver": ctx.attrs._rpm_driver,
            "plan": ctx.attrs._rpm_plan,
            "plugin": ctx.attrs._rpm_plugin,
            "subjects": [],
            "subjects_src": rpm_subjects,
        },
    )

    def _map(rpm_feature_providers):
        return [
            DefaultInfo(),
            MultiFeatureAnalysis(
                features = [install_feature, rpm_feature_providers[FeatureAnalysis]],
            ),
        ]

    return anon_rpm_target.promise.map(_map)

install_rule = rule(
    impl = _impl,
    attrs = {
        "always_use_gnu_debuglink": attrs.bool(default = True),
        "build_phase": attrs.enum(BuildPhase.values(), default = "compile"),
        "default_binary_mode": attrs.option(attrs.int(), default = None),
        "default_directory_mode": attrs.option(attrs.int(), default = None),
        "default_file_mode": attrs.option(attrs.int(), default = None),
        "dst": attrs.string(),
        "group": attrs.one_of(
            attrs.string(),
            attrs.int(),
            default = "root",
        ),
        "ignore_symlink_tree": attrs.bool(default = False),
        "mode": attrs.option(attrs.int(), default = None),
        "never_use_dev_binary_symlink": attrs.bool(
            default = False,
            doc = "Always install as a regular file, even in @mode/dev",
        ),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "setcap": attrs.option(attrs.string(), default = None),
        "split_debuginfo": attrs.bool(default = True),
        "src": attrs.option(
            attrs.one_of(
                attrs.dep(),
                attrs.source(),
            ),
            default = None,
        ),
        "text": attrs.option(attrs.string(), default = None),
        "transition_to_distro_platform": attrs.enum(transition_to_distro_platform.values(), default = "no"),
        "user": attrs.one_of(
            attrs.string(),
            attrs.int(),
            default = "root",
        ),
        "xattrs": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "_binaries_require_repo": binaries_require_repo.optional_attr,
        "_objcopy": attrs.option(attrs.exec_dep(), default = None),
        "_rpm_driver": attrs.option(attrs.dep(providers = [RunInfo]), default = None),
        "_rpm_find_requires": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "_rpm_plan": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "_rpm_plugin": attrs.option(attrs.exec_dep(providers = [FeaturePluginInfo]), default = None),
    },
)
