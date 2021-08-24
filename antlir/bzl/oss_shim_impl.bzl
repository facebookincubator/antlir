# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:types.bzl", "types")
load("@config//:config.bzl", _do_not_use_repo_cfg = "do_not_use_repo_cfg")
load("//third-party/fedora33/kernel:kernels.bzl", "kernels")
# @lint-ignore-every BUCKLINT

_RULE_TYPE_KWARG = "antlir_rule"

_RULE_PRIVATE = "antlir-private"

_RULE_USER_FACING = "user-facing"

_RULE_USER_INTERNAL = "user-internal"

_ALLOWED_RULES = [
    _RULE_PRIVATE,
    _RULE_USER_FACING,
    _RULE_USER_INTERNAL,
]

# The default native platform to use for shared libraries and static binary
# dependencies.  Right now this tooling only supports one platform and so
# this is not a method, but in the future as we support other native platforms
# (like Debian, Arch Linux, etc..) this should be expanded to allow for those.
_DEFAULT_NATIVE_PLATFORM = "fedora33"

# Serves two important purposes:
#  - Ensures that all user-instanted rules are annotated with
#    `antlir_rule = "user-{facing,internal}", which is important for FB CI.
#  - Discourages users from loading rules or functions from `oss_shim.bzl`.
def _assert_package():
    package = native.package_name()
    if package == "antlir/compiler/test_images":
        fail(
            '`antlir/compiler/test_images` is treated as "outside of the ' +
            'Antlir codebase" for the purposes of testing `antlir_rule`. ' +
            "Therefore, you may not access `oss_shim.bzl` directly from its " +
            "build file -- instead add and use a shim inside of " +
            "`antlir/compiler/test_images/defs.bzl`. You may also get this " +
            "error if you are adding a new user-instantiatable rule to the " +
            "Antlir API. If so, read the `antlir_rule` section in " +
            "`website/docs/coding-conventions/bzl-and-targets.md`",
        )

    # In OSS, the shimmed rules are preferred over the native rules (the
    # implicit loads of native rules is disabled) for consistency. Everything
    # in the main cell except the above exception(s) are allowed to use
    # oss_shim.bzl
    cell = native.repository_name()

    # TODO: if antlir is intended to _only_ be used as a Buck cell, the '@'
    # check should be disabled. This is not currently the way the project is
    # setup, so it is required for now.
    if cell != "@antlir" and cell != "@":
        fail(
            'Package `{}` must not `load("//antlir/bzl:'.format(package) +
            'oss_shim.bzl")`. Antlir devs: read about `antlir_rule` in ' +
            "`website/docs/coding-conventions/bzl-and-targets.md`.",
        )

def _invert_dict(d):
    """ In OSS Buck some of the dicts used by targets (`srcs` and `resources`
    specifically) are inverted, where internally this:

        resources = { "//target:name": "label_of_resource" }

    In OSS Buck this is:

        resources = { "label_of_resource": "//target:name" }
    """
    if d and types.is_dict(d):
        result = {value: key for key, value in d.items()}

        if len(result) != len(d):
            fail("_invert_dict fail! len(result): " + len(result) + " != len(d): " + len(d))

        return result
    else:
        return d

def _kernel(version):
    """ Resolve a kernel version to its corresponding kernel artifact.
    Currently, the only `kernel_artifact` available is in
    //third-party/fedora33/kernel:kernels.bzl.

    a `kernel_artifact`is a struct containing the following members:
    - uname
    - vmlinuz: compressed vmlinux
    - modules: kernel modules
    - headers: Includes the C header files that specify the interface between the
               Linux kernel and user-space libraries and programs.
    - devel:   Contains the kernel headers and makefiles sufficient to build modules
               against the kernel package.
    """
    if version in kernels:
        return kernels[version]
    else:
        fail("Unknown kernel version: {}".format(version))

def _normalize_deps(deps, more_deps = None):
    """  Create a single list of deps from one or 2 provided lists of deps.
    Additionally exclude any deps that have `/facebook` in the path as these
    are internal and require internal FB infra.
    """

    _deps = deps or []
    _more_deps = more_deps or []
    _deps = _deps + _more_deps

    derps = []
    for dep in _deps:
        if dep.find("facebook") < 0:
            derps.append(dep)

    return derps

def _normalize_resources(resources):
    """ Exclude any resources that have `/facebook` in the path as these
    are internal and require internal FB infra. Only applies to resources
    specified in the `dict` format

    Will also go ahead an invert the dictionary using `_invert_dict`
    """
    if resources and types.is_dict(resources):
        _normalized_dict_keys = _normalize_deps(resources.keys())
        _normalized_resources = {key: resources[key] for key in _normalized_dict_keys}

        return _invert_dict(_normalized_resources)
    else:
        return resources

def _normalize_coverage(coverages):
    """ Exclude any coverage requirements that have `facebook` in the path as
    these are internal.
    """
    return [
        (percent, dep)
        for percent, dep in coverages
        if "facebook" not in dep
    ] if coverages else None

def _normalize_visibility(vis, name = None):
    """ OSS Buck has a slightly different handling of visibility.
    The default is to be not visible.
    For more info see: https://buck.build/concept/visibility.html
    """
    if vis == None:
        return ["PUBLIC"]
    else:
        return vis

def _normalize_pkg_style(style):
    """
    Internally, zip and fastzip internally behave similar to how an
    `inplace` python binary behaves in OSS Buck.
    """
    if not style:
        return None

    # Support some aliases that are used internally, otherwise return the style
    # directly if it is unrecognized
    if style in ("zip", "fastzip"):
        return "inplace"
    if style in ("xar",):
        return "standalone"
    return style

def _third_party_library(project, rule = None, platform = None):
    """
    Generate a target for a third-party library.  This will return a target
    that is normalized into the form (see the README in `//third-party/...`
    more info on these targets):

        //third-party/<platform>/<project>:<rule>
        or
        //third-party/python:<project> for python rules

    Thee are currently only 2 platforms supported in OSS:
        - python
        - fedora33

    If `platform` is not provided it is assumed to be `fedora33`.

    If `rule` is not provided it is assumed to be the same as `project`.
    """
    _assert_package()  # Antlir-private: only use with `oss_shim.bzl` macros.

    if not rule:
        rule = project

    if not platform:
        platform = _DEFAULT_NATIVE_PLATFORM

    if platform == "rust":
        if not rule == project:
            fail("rust dependencies must omit rule or be identical to project")
        return "//third-party/rust:" + project

    if platform == "python":
        if not rule == project:
            fail("python projects must omit rule or be identical to project")
        return "//third-party/python:" + project

    return "//third-party/" + platform + "/" + project + ":" + rule

def _wrap_internal(fn, args, kwargs):
    """
    Wrap a build target rule with support for the `antlir_rule` kwarg.

    Three rule types are supported:

      - "antlir-private" (default): Such rules MAY NOT be defined in user
        packages (outside of the Antlir codebase) -- see `_assert_package()`.

      - "user-internal": May be defined by user packages, but does not
        produce a build artifact that the user can use **directly**, e.g.
        "SUFFIX-<name>" intermediate rules, or `image.{feature,layer}`.

      - "user-facing": Allowed in user packages, and builds artifacts that
        the end user can use directly, e.g. `image.package`.

    Rules are private by default to force Antlir devs to explicitly annotate
    "user-internal" and "user-facing" rules.

    See also `website/docs/coding-conventions/bzl-and-targets.md`.
    """
    rule_type = kwargs.pop(_RULE_TYPE_KWARG, _RULE_PRIVATE)
    if rule_type == _RULE_PRIVATE:
        # Private rules should only be defined within `antlir/`.
        _assert_package()
    elif rule_type not in _ALLOWED_RULES:
        fail(
            "Bad value {}, must be one of {}".format(rule_type, _ALLOWED_RULES),
            _RULE_TYPE_KWARG,
        )

    # Antlir build outputs should not be visible outside of antlir by default. This
    # helps prevent our abstractions from leaking into other codebases as Antlir
    # becomes more widely adopted.
    kwargs["visibility"] = _normalize_visibility(kwargs.pop("visibility", None)) + [
        "//antlir/...",
        "//bot_generated/antlir/...",
    ]

    fn(*args, **kwargs)

def _command_alias(*args, **kwargs):
    _wrap_internal(native.command_alias, args, kwargs)

def _filegroup(*args, **kwargs):
    _wrap_internal(native.filegroup, args, kwargs)

def _genrule(*args, **kwargs):
    # For future use to support target platforms
    kwargs.pop("flavor_config", None)
    _wrap_internal(native.genrule, args, kwargs)

def _http_file(*args, **kwargs):
    _wrap_internal(native.http_file, args, kwargs)

def _http_archive(*args, **kwargs):
    _wrap_internal(native.http_archive, args, kwargs)

def _sh_binary(*args, **kwargs):
    _wrap_internal(native.sh_binary, args, kwargs)

def _sh_test(*args, **kwargs):
    _wrap_internal(native.sh_test, args, kwargs)

def _worker_tool(*args, **kwargs):
    _wrap_internal(native.worker_tool, args, kwargs)

def _cxx_external_deps(kwargs):
    external_deps = kwargs.pop("external_deps", [])
    return ["//third-party/cxx:" + lib for _project, _version, lib in external_deps]

def _impl_cpp_binary(name, tags = None, **kwargs):
    native.cxx_binary(
        name = name,
        labels = tags or [],
        deps = _normalize_deps(kwargs.pop("deps", []), _cxx_external_deps(kwargs)),
        **kwargs
    )

def _cpp_binary(*args, **kwargs):
    _wrap_internal(_impl_cpp_binary, args, kwargs)

def _impl_cpp_library(name, tags = None, **kwargs):
    native.cxx_library(
        name = name,
        labels = tags or [],
        deps = _normalize_deps(kwargs.pop("deps", []), _cxx_external_deps(kwargs)),
        **kwargs
    )

def _cpp_library(*args, **kwargs):
    _wrap_internal(_impl_cpp_library, args, kwargs)

def _impl_cpp_unittest(name, tags = None, **kwargs):
    native.cxx_test(
        name = name,
        labels = tags or [],
        deps = _normalize_deps(kwargs.pop("deps", []), _cxx_external_deps(kwargs)),
        **kwargs
    )

def _cpp_unittest(*args, **kwargs):
    _wrap_internal(_impl_cpp_unittest, args, kwargs)

def _export_file(*args, **kwargs):
    _wrap_internal(native.export_file, args, kwargs)

def _impl_python_binary(
        name,
        main_module,
        par_style = None,
        resources = None,
        visibility = None,
        **kwargs):
    _impl_python_library(
        name = name + "-library",
        resources = resources,
        visibility = visibility,
        **kwargs
    )

    native.python_binary(
        name = name,
        main_module = main_module,
        package_style = _normalize_pkg_style(par_style),
        deps = [":" + name + "-library"],
        visibility = visibility,
    )

def _python_binary(*args, **kwargs):
    _wrap_internal(_impl_python_binary, args, kwargs)

def _impl_python_library(
        name,
        deps = None,
        resources = None,
        srcs = None,
        **kwargs):
    native.python_library(
        name = name,
        deps = _normalize_deps(deps),
        resources = _normalize_resources(resources),
        srcs = _invert_dict(srcs),
        **kwargs
    )

def _python_library(*args, **kwargs):
    _wrap_internal(_impl_python_library, args, kwargs)

def _impl_python_unittest(
        cpp_deps = "ignored",
        deps = None,
        needed_coverage = None,
        par_style = None,
        tags = None,
        resources = None,
        srcs = None,
        **kwargs):
    native.python_test(
        deps = _normalize_deps(deps),
        labels = tags or [],
        needed_coverage = _normalize_coverage(needed_coverage),
        package_style = _normalize_pkg_style(par_style),
        resources = _normalize_resources(resources),
        srcs = _invert_dict(srcs),
        **kwargs
    )

def _python_unittest(*args, **kwargs):
    _wrap_internal(_impl_python_unittest, args, kwargs)

def _split_rust_kwargs(kwargs):
    # process some kwargs common to all rust targets, as well as some split
    # kwargs from rust_{binary,library} that are forwarded to implicit
    # rust_unittest targets
    if not kwargs.get("crate_root", None):
        topsrc_options = (kwargs.get("name") + ".rs", "main.rs")

        topsrc = []
        for src in (kwargs.get("srcs", None) or []):
            if src.startswith(":"):
                continue

            if paths.basename(src) in topsrc_options:
                topsrc.append(src)

        if len(topsrc) == 1:
            kwargs["crate_root"] = topsrc[0]
    test_kwargs = None

    # automatically generate a unittest target if the caller did not explicitly
    # opt out
    if kwargs.pop("unittests", True):
        test_mapped_srcs = kwargs.get("mapped_srcs", {})
        test_mapped_srcs.update(kwargs.pop("test_mapped_srcs", {}))
        test_kwargs = {
            "crate": kwargs.get("crate", kwargs.get("name").replace("-", "_")),
            "crate_root": kwargs.get("crate_root"),
            "deps": kwargs.get("deps", []) + kwargs.pop("test_deps", []),
            "labels": kwargs.get("labels", []),
            "mapped_srcs": test_mapped_srcs,
            "name": kwargs.get("name") + "-unittest",
            "srcs": kwargs.get("srcs", []) + kwargs.pop("test_srcs", []),
        }

    return kwargs, test_kwargs

def _rust_unittest(*args, **kwargs):
    _wrap_internal(native.rust_test, args, kwargs)

def _rust_binary(*args, **kwargs):
    # Inside FB, we are a little special and explicitly use `malloc` as our
    # allocator, and avoid linking to some always-present FB libraries in order
    # to keep our environment simple and produce small binaries. In OSS, this
    # isn't required (yet), since the default platforms will be close to this
    # goal already.
    kwargs.pop("allocator", None)
    kwargs.pop("nodefaultlibs", None)

    kwargs, test_kwargs = _split_rust_kwargs(kwargs)
    _wrap_internal(native.rust_binary, args, kwargs)

    if test_kwargs:
        _rust_unittest(**test_kwargs)

def _rust_library(*args, **kwargs):
    kwargs, test_kwargs = _split_rust_kwargs(kwargs)
    _wrap_internal(native.rust_library, args, kwargs)

    if test_kwargs:
        _rust_unittest(**test_kwargs)

# Use = in the default filename to avoid clashing with RPM names.
# The constant must match `update_allowed_versions.py`.
# Omits `_wrap_internal` due to perf paranoia -- we have a callsite per RPM.
def _rpm_vset(name, src = "empty=rpm=vset"):
    native.export_file(
        name = name,
        src = src,
        mode = "reference",
        # `image.layer`s all over the repo will depend on these
        visibility = ["PUBLIC"],
    )

### BEGIN COPY-PASTA (@fbcode_macros//build_defs/lib:rule_target_types.bzl)
# @lint-ignore BUILDIFIERLINT
_RuleTargetProvider = provider(fields = [
    "name",  # The name of the rule
    "base_path",  # The base package within the repository
    "repo",  # Either the cell or None (for the root cell)
])

def _RuleTarget(repo, base_path, name):
    return _RuleTargetProvider(name = name, base_path = base_path, repo = repo)

### END COPY-PASTA

### BEGIN COPY-PASTA (@fbcode_macros//build_defs/lib:target_utils.bzl)
def _parse_target(target, default_repo = None, default_base_path = None):
    if target.count(":") != 1:
        fail('rule name must contain exactly one ":": "{}"'.format(target))

    repo_and_base_path, name = target.split(":")

    # Parse relative target.
    if not repo_and_base_path:
        return _RuleTarget(default_repo, default_base_path, name)

    # Parse absolute targets.
    if repo_and_base_path.count("//") != 1:
        fail('absolute rule name must contain one "//" before ":": "{}"'.format(target))
    repo, base_path = repo_and_base_path.split("//", 1)
    repo = repo or default_repo

    return _RuleTarget(repo, base_path, name)

def _to_label(repo, path, name):
    return "{}//{}:{}".format(repo, path, name)

### END COPY-PASTA

def _get_project_root_from_gen_dir():
    # NB: This will break if "buck-out" is set to something containing 1 or
    # more slashes (e.g.  `/my/buck/out`).  A fix would be to copy-pasta
    # `_get_buck_out_path`, but it seems like an unnecessary complication.
    return "../.."

# Please keep each section lexicographically sorted.
shim = struct(
    #
    # Rules -- IMPORTANT -- wrap **ALL** rules with `_wrap_internal`
    #
    buck_command_alias = _command_alias,
    buck_filegroup = _filegroup,
    buck_genrule = _genrule,
    buck_sh_binary = _sh_binary,
    buck_sh_test = _sh_test,
    buck_worker_tool = _worker_tool,
    #
    # Utility functions -- use `_assert_package()`, if at all possible.
    #
    config = struct(
        # @lint-ignore BUCKLINT
        get_current_repo_name = native.repository_name,
        get_project_root_from_gen_dir = _get_project_root_from_gen_dir,
    ),
    cpp_binary = _cpp_binary,
    cpp_library = _cpp_library,
    cpp_unittest = _cpp_unittest,
    #
    # Constants
    #
    default_vm_image = struct(
        layer = "//antlir/vm:default-image",
        package = "//antlir/vm:default-image.btrfs",
    ),
    do_not_use_repo_cfg = _do_not_use_repo_cfg,
    export_file = _export_file,
    get_visibility = _normalize_visibility,
    http_file = _http_file,
    http_archive = _http_archive,
    kernel_get = struct(
        base_target = "//third-party/fedora33/kernel",
        default = _kernel("5.8.15-301.fc33.x86_64"),
        get = _kernel,
        versions = kernels,
    ),
    platform_utils = None,
    python_binary = _python_binary,
    python_library = _python_library,
    python_unittest = _python_unittest,
    rust_binary = _rust_binary,
    rust_library = _rust_library,
    rust_unittest = _rust_unittest,
    rpm_vset = _rpm_vset,  # Not wrapped due to perf paranoia.
    target_utils = struct(
        parse_target = _parse_target,
        to_label = _to_label,
    ),
    third_party = struct(
        library = _third_party_library,
    ),
    vm_image_path = "//images/base:",
)
