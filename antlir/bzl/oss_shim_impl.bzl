#
# IMPORTANT: You **MUST** wrap with `_wrap_internal()` any rules / macros
# exported from this file (any functions that define new targets).  Per the
# D23464472 description, consistently using `_wrap_internal()` creates a
# rule ecosystem where we are forced to consistently label targets as
# "internal" vs "external".  Read that diff for the full context.
#
# Please **ALSO** call `_assert_package()` from any function that a
# non-Antlir developer might accidentally call if their editor auto-imports
# `oss_shim.bzl`.
#
# If you came here to understand the `antlir_rule` kwarg, please read the
# docblocks for `_wrap_internal` and `_assert_package`.
#
load("@fbcode_macros//build_defs:config.bzl", "config")
load("@fbcode_macros//build_defs:cpp_unittest.bzl", "cpp_unittest")
load("@fbcode_macros//build_defs:custom_rule.bzl", "get_project_root_from_gen_dir")
load("@fbcode_macros//build_defs:export_files.bzl", "export_file")
load("@fbcode_macros//build_defs:native_rules.bzl", "buck_command_alias", "buck_filegroup", "buck_genrule", "buck_sh_binary", "buck_sh_test")
load("@fbcode_macros//build_defs:platform_utils.bzl", "platform_utils")
load("@fbcode_macros//build_defs:python_binary.bzl", "python_binary")
load("@fbcode_macros//build_defs:python_library.bzl", "python_library")
load("@fbcode_macros//build_defs:python_unittest.bzl", "python_unittest")
load("@fbcode_macros//build_defs/lib:target_utils.bzl", "target_utils")
load("@fbcode_macros//build_defs/lib:third_party.bzl", "third_party")
load("@fbcode_macros//build_defs/lib:visibility.bzl", "get_visibility")

# We have tens of thousands of `empty_rpm` targets, so eliminating the
# indirection of `buck_export_file` seems worthwhile.
#
# @lint-ignore-every BUCKFBCODENATIVE
load("@fbsource//tools/build_defs:fb_native_wrapper.bzl", "fb_native")
load("//kernel/kernels:kernels.bzl", "kernels")

# IMPORTANT: These _RULE constants are cloned to `oss_shim_impl.bzl.oss`
_RULE_TYPE_KWARG = "antlir_rule"
_RULE_PRIVATE = "antlir-private"
_RULE_USER_FACING = "user-facing"
_RULE_USER_INTERNAL = "user-internal"
_ALLOWED_RULES = [_RULE_PRIVATE, _RULE_USER_FACING, _RULE_USER_INTERNAL]

# CRUCIAL: Do NOT change this constant.  It is used inside the sitevar
# `SV_SANDCASTLE_RDEPS_COST_ADJUSTMENT` to ensure that fbcode TD triggers
# enough tests for source changes that affect image tests & artifacts.
# See also: https://fburl.com/antlir_rule_usage
_INTERNAL_RULE_LABEL = "antlir__internal"  # maps to "user-internal" above

_default_vm_layer = "//tupperware/image/vmtest:base"
_default_vm_package = "//tupperware/image/vmtest:base.btrfs"

# The primary purpose of this assertion is to ensure that Antlir devs
# annotate all targets that need to be hidden from fbcode target
# determinator's distance-4 heuristic, see `_wrap_internal()` below.
#
# An important side effect is that this prevents the usage of `oss_shim.bzl`
# from outside of the Antlir project.  This became necessary because some
# automation (VSCode?) kept inserting bad `load` statements.  Context:
# https://fb.prod.workplace.com/groups/btrmeup/permalink/3304265952986381/
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
            "Antlir API. If so, read https://fburl.com/antlir_rule_usage. ",
        )
    if package != "antlir" and not package.startswith("antlir/"):
        fail(
            "Package `" + package + "` must not " +
            '`load("//antlir/bzl:oss_shim.bzl")` -- did you mean to `load(' +
            '"@fbcode_macros//build_defs:RULE_NAME.bzl")` instead? Did ' +
            "your editor insert this for you? If you are using VSCode, " +
            "report the issue here: https://fburl.com/no_oss_shim_load " +
            "FOR ANTLIR DEVS ONLY: Please read D23464472 or " +
            "https://fburl.com/antlir_rule_usage.",
        )

def _kernel_artifact_version(version):
    """
    Internally, we have a collection of kernel versions built at
    //kernel/kernels:kernels.bzl. We allow users to specify either
    the short form (used in `official`) or the full uname (used in `kernels`).

    Internally, a `kernel_artifact`is a struct containing the following members:
    - uname
    - vmlinuz: compressed vmlinux
    - vmlinux: Linux kernel in an statically linked executable file format
    - modules: kernel modules
    - initrd:  used to boot this kernel in a virtual machine and setup the root
               disk as a btrfs seed device with the second disk for writes to go to.
    - headers: Includes the C header files that specify the interface between the
               Linux kernel and user-space libraries and programs.
    - devel:   Contains the kernel headers and makefiles sufficient to build modules
               against the kernel package.
    - vm:      a python_library containing all the artifacts required to run a kernel VM
    """
    return kernels.kernel(version)

# Use = in the default filename to avoid clashing with RPM names.
# The constant must match `update_allowed_versions.py`.
def _rpm_vset(name, src = "empty=rpm=vset"):
    # _assert_package() omitted for perf reasons, we have 60k callsites, and
    # it's unlikely that a non-Antlir eng will use this by accident.
    fb_native.export_file(
        name = name,
        src = src,
        mode = "reference",
        # `image.layer`s all over the repo will depend on these
        visibility = ["PUBLIC"],
        labels = [_INTERNAL_RULE_LABEL],
    )

def _third_party_library(project, rule = None, platform = None):
    """
    In FB land we want to find the correct build for a third-party target
    based on the current platform.

    If the platform is a python libary, use the pyfi infrastructure so
    there is no need for `external_deps` in python targets.
    """

    # _assert_package(): This is mostly just called from `antlir` TARGETS
    # files, but we also have a callsite in `vm/initrd.bzl`, which can be
    # invoked from customer TARGETS files.  I could work around this with an
    # extra boolean flag, but it should be unlikely that people start using
    # this helper by accident (so far, it happened once).
    if not rule:
        rule = project

    if not platform:
        platform = platform_utils.get_platform_for_current_buildfile()

    if platform == "python":
        return "//python/wheel/" + project + ":" + rule
    else:
        return third_party.third_party_target(platform, project, rule)

def _wrap_internal(fn, args, kwargs, is_titled_labels = False):
    """
    Wrap a build target rule with support for the `antlir_rule` kwarg.

    Three rule types are supported:

      - "antlir-private" (default): Such rules MAY NOT be defined in user
        packages (outside of the Antlir codebase) -- see `_assert_package()`.

      - "user-internal": May be defined by user packages, and excluded from
        the fbcode TD-4 dependency distance calculation.  Specificially,
        _INTERNAL_RULE_LABEL` will be added to labels/tags of the wrapped
        rule.  That tells fbcode target determinator to skip the tagged rule
        when calculating the build/test nodes affected by a diff -- nodes
        farther than (currently) 4 dependency hops from their source will
        not be validated on-diff. Wiki: https://fburl.com/antlir_td4

        Labeling rules "user-internal" is critical for CI to work as
        expected, since the image building toolchain creates many
        intermediary targets that artificially inflate the distance between
        legitimate dependencies.

      - "user-facing": Allowed in user packages, and counted towards the
        TD-4 heuristic calculation.

    Rules are private by default to force Antlir devs to explicitly annotate
    "user-internal" and "user-facing" rules.  We really only just want the
    "internal" annotations to be correct so that fbcode TD works well, but
    we cannot force the annotation of one without the other.  Since the
    number of "user-facing" targets is very small, this is a good tradeoff.
    """
    rule_type = kwargs.pop(_RULE_TYPE_KWARG, _RULE_PRIVATE)
    if rule_type == _RULE_PRIVATE:
        # Private rules should only be defined within `antlir/`.
        _assert_package()
    elif rule_type == _RULE_USER_INTERNAL:
        # This target is generated from a customer's package, but is a
        # detail of Antlir's plumbing, and as such should not count towards
        # the inter-rule dependency distance for fbcode TD4 purposes.
        label_arg = "labels" if is_titled_labels else "tags"
        kwargs.setdefault(label_arg, []).append(_INTERNAL_RULE_LABEL)
    elif rule_type != _RULE_USER_FACING:
        fail(
            "Bad value {}, must be one of {}".format(rule_type, _ALLOWED_RULES),
            _RULE_TYPE_KWARG,
        )
    fn(*args, **kwargs)

def _buck_command_alias(*args, **kwargs):
    _wrap_internal(buck_command_alias, args, kwargs, is_titled_labels = True)

def _buck_filegroup(*args, **kwargs):
    _wrap_internal(buck_filegroup, args, kwargs, is_titled_labels = True)

def _buck_genrule(*args, **kwargs):
    _wrap_internal(buck_genrule, args, kwargs, is_titled_labels = True)

def _buck_sh_binary(*args, **kwargs):
    _wrap_internal(buck_sh_binary, args, kwargs, is_titled_labels = True)

def _buck_sh_test(*args, **kwargs):
    _wrap_internal(buck_sh_test, args, kwargs, is_titled_labels = True)

def _export_file(*args, **kwargs):
    _wrap_internal(export_file, args, kwargs, is_titled_labels = True)

def _cpp_unittest(*args, **kwargs):
    _wrap_internal(cpp_unittest, args, kwargs)

def _python_binary(*args, **kwargs):
    _wrap_internal(python_binary, args, kwargs)

def _python_library(*args, **kwargs):
    _wrap_internal(python_library, args, kwargs)

def _python_unittest(*args, **kwargs):
    _wrap_internal(python_unittest, args, kwargs)

# Please keep each section lexicographically sorted.
shim = struct(
    #
    # Rules -- IMPORTANT -- wrap **ALL** rules with `_wrap_internal`
    #
    buck_command_alias = _buck_command_alias,
    buck_filegroup = _buck_filegroup,
    buck_genrule = _buck_genrule,
    buck_sh_binary = _buck_sh_binary,
    buck_sh_test = _buck_sh_test,
    cpp_unittest = _cpp_unittest,
    export_file = _export_file,
    python_binary = _python_binary,
    python_library = _python_library,
    python_unittest = _python_unittest,
    rpm_vset = _rpm_vset,  # Not wrapped due to perf paranoia, we have 60k calls
    #
    # Utility functions -- use `_assert_package()`, if at all possible.
    #
    config = struct(
        get_current_repo_name = config.get_current_repo_name,
        get_project_root_from_gen_dir = get_project_root_from_gen_dir,
    ),
    get_visibility = get_visibility,
    target_utils = struct(
        parse_target = target_utils.parse_target,
        to_label = target_utils.to_label,
    ),
    third_party = struct(
        library = _third_party_library,
    ),
    #
    # Constants
    #
    default_vm_image = struct(
        layer = _default_vm_layer,
        package = _default_vm_package,
    ),
    # Access these via `constants.bzl`, do not use this dict directly.
    #
    # IMPORTANT: Before changing a config, please review the corresponding
    # docblock in `constants.bzl`, many of these are sensitive.
    #
    # Keep in mind that this should be a `Dict[str, Optional[str]]` to allow
    # overrides via Buck's `-c` CLI option.
    #
    # These `fbcode`-specific configs are not in `.buckconfig` because of
    # https://fb.prod.workplace.com/groups/fbcode/permalink/3264530036917146/
    do_not_use_repo_cfg = {
        "build_appliance_default": "//tupperware/image/build_appliance:fb_build_appliance",
        "host_mounts_allowed_in_targets": " ".join([
            "//tupperware/image/features:fb_infra_support",
        ]),
        "host_mounts_for_repo_artifacts": " ".join([
            "/mnt/gvfs",
        ]),
        "rpm_installer_default": "yum",
        # Replace with "tw_jobs" once this is ready for prod:
        # https://fb.quip.com/vsM7AIvScYrA
        "version_set_default": None,
        # DO NOT ADD to this list without reading the doc in `constants.bzl`.
        "version_set_to_path": " ".join([
            (k + " " + v)
            for k, v in {
                "tw_jobs": "//bot_generated/antlir/version_sets/tw_jobs",
            }.items()
        ]),
    },
    kernel_artifact = struct(
        default_kernel = _kernel_artifact_version("5.2"),
        version = _kernel_artifact_version,
    ),
)
