load("@bazel_skylib//lib:types.bzl", "types")
load("//third-party/fedora31/kernel:kernels.bzl", "kernels")

# The default native platform to use for shared libraries and static binary
# dependencies.  Right now this tooling only supports one platform and so
# this is not a method, but in the future as we support other native platforms
# (like Debian, Arch Linux, etc..) this should be expanded to allow for those.
_DEFAULT_NATIVE_PLATFORM = "fedora31"

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

def _kernel_artifact_version(version):
    """ Resolve a kernel version to its corresponding kernel artifact.
    Currently, the only `kernel_artifact` available is in
    //third-party/fedora31/kernel:kernels.bzl.

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
    return [(percent, dep) for percent, dep in coverages 
            if "facebook" not in dep] if coverages else None
 
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
    if style and style in ("zip", "fastzip"):
        return "inplace"
    else:
        return "standalone"

def _third_party_library(project, rule = None, platform = None):
    """
    Generate a target for a third-party library.  This will return a target
    that is normalized into the form (see the README in `//third-party/...`
    more info on these targets):

        //third-party/<platform>/<project>:<rule>

    Thee are currently only 2 platforms supported in OSS:
        - python
        - fedora31

    If `platform` is not provided it is assumed to be `fedora31`.

    If `rule` is not provided it is assumed to be the same as `project`.
    """

    if not rule:
        rule = project

    if not platform:
        platform = _DEFAULT_NATIVE_PLATFORM

    return "//third-party/" + platform + "/" + project + ":" + rule

def _cpp_unittest(name, tags = [], visibility = None, **kwargs):
    cxx_test(
        name = name,
        labels = tags,
        visibility = _normalize_visibility(visibility),
        **kwargs
    )

def _python_binary(
        name,
        main_module,
        par_style = None,
        resources = None,
        visibility = None,
        **kwargs):
    _python_library(
        name = name + "-library",
        resources = resources,
        visibility = visibility,
        **kwargs
    )

    python_binary(
        name = name,
        deps = [":" + name + "-library"],
        main_module = main_module,
        package_style = _normalize_pkg_style(par_style),
        visibility = _normalize_visibility(visibility, name),
    )

def _python_library(
        name,
        deps = None,
        visibility = None,
        resources = None,
        srcs = None,
        **kwargs):
    python_library(
        name = name,
        deps = _normalize_deps(deps),
        resources = _normalize_resources(resources),
        srcs = _invert_dict(srcs),
        visibility = _normalize_visibility(visibility, name),
        **kwargs
    )

def _python_unittest(
        cpp_deps = "ignored",
        deps = None,
        needed_coverage = None,
        par_style = None,
        tags = None,
        resources = None,
        **kwargs):
    python_test(
        deps = _normalize_deps(deps),
        labels = tags if tags else [],
        needed_coverage = _normalize_coverage(needed_coverage),
        package_style = _normalize_pkg_style(par_style),
        resources = _normalize_resources(resources),
        **kwargs
    )

### BEGIN COPY-PASTA (@fbcode_macros//build_defs/lib:rule_target_types.bzl)
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

# Use = in the filename to avoid clashing with RPM names.
def rpm_vset(name, src = "empty=rpm=vset"):
    export_file(
        name = name,
        src = src,
        mode = "reference",
        # `image.layer`s all over the repo will depend on these
        visibility = ["PUBLIC"],
   )

shim = struct(
    buck_command_alias = command_alias,
    buck_filegroup = filegroup,
    buck_genrule = genrule,
    buck_sh_binary = sh_binary,
    buck_sh_test = sh_test,
    cpp_unittest = _cpp_unittest,
    config = struct(
        get_current_repo_name = native.repository_name,
        get_project_root_from_gen_dir = _get_project_root_from_gen_dir,
    ),
    default_vm_layer = None,
    get_visibility = _normalize_visibility,
    kernel_artifact = struct(
        default_kernel = _kernel_artifact_version("5.3.7-301.fc31.x86_64"),
        version = _kernel_artifact_version,
    ),
    platform_utils = None,
    python_binary = _python_binary,
    python_library = _python_library,
    python_unittest = _python_unittest,
    rpm_vset = rpm_vset,
    target_utils = struct(
        parse_target = _parse_target,
        to_label = _to_label,
    ),
    third_party = struct(
        library = _third_party_library,
    ),
)
