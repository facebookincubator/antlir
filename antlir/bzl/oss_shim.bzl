# This file redeclares (and potentially validates) JUST the part of the
# fbcode macro API that is allowed within `antlir/`.  This way,
# FB-internal contributors will be less likely to accidentally break
# open-source by starting to use un-shimmed features.
load(":oss_shim_impl.bzl", "shim")

def _check_args(rule, args, kwargs, allowed_kwargs):
    if args:
        fail("use kwargs")
    for kwarg in kwargs:
        if kwarg not in allowed_kwargs:
            fail("kwarg `{}` is not supported by the OSS shim for `{}`".format(
                kwarg,
                rule,
            ))

def _make_rule_kwargs_dict(lst):
    # `antlir_rule` is forwarded to oss_shim_impl.bzl and is used to mark
    # rules as "antlir-private", "user-internal", or "user-facing".  Read
    # the comments in that file for the detailed rationale.
    return {k: 1 for k in lst + ["antlir_rule"]}

_CPP_UNITTEST_KWARGS = _make_rule_kwargs_dict(
    ["name", "deps", "env", "headers", "srcs", "tags", "use_default_test_main", "visibility", "external_deps"],
)

def cpp_unittest(*args, **kwargs):
    _check_args("cpp_unittest", args, kwargs, _CPP_UNITTEST_KWARGS)
    shim.cpp_unittest(**kwargs)

_PYTHON_BINARY_KWARGS = _make_rule_kwargs_dict(
    [
        "name",
        "base_module",
        "deps",
        "main_module",
        "par_style",
        "resources",
        "runtime_deps",
        "srcs",
        "visibility",
    ],
)

def python_binary(*args, **kwargs):
    _check_args("python_binary", args, kwargs, _PYTHON_BINARY_KWARGS)
    shim.python_binary(**kwargs)

_PYTHON_LIBRARY_KWARGS = _make_rule_kwargs_dict(
    [
        "name",
        "base_module",
        "deps",
        "resources",
        "runtime_deps",
        "srcs",
        "visibility",
    ],
)

def python_library(*args, **kwargs):
    _check_args("python_library", args, kwargs, _PYTHON_LIBRARY_KWARGS)
    shim.python_library(**kwargs)

_PYTHON_UNITTEST_KWARGS = _make_rule_kwargs_dict(
    [
        "base_module",
        "check_types",
        "cpp_deps",
        "deps",
        "env",
        "main_module",
        "name",
        "needed_coverage",
        "par_style",
        "resources",
        "runtime_deps",
        "srcs",
        "tags",
        "visibility",
    ],
)

def python_unittest(*args, **kwargs):
    _check_args("python_unittest", args, kwargs, _PYTHON_UNITTEST_KWARGS)
    shim.python_unittest(**kwargs)

buck_command_alias = shim.buck_command_alias
buck_filegroup = shim.buck_filegroup
buck_genrule = shim.buck_genrule
buck_sh_binary = shim.buck_sh_binary
buck_sh_test = shim.buck_sh_test
config = shim.config
default_vm_image = shim.default_vm_image
export_file = shim.export_file
get_visibility = shim.get_visibility
kernel_artifact = shim.kernel_artifact
do_not_use_repo_cfg = shim.do_not_use_repo_cfg
rpm_vset = shim.rpm_vset
target_utils = shim.target_utils
third_party = shim.third_party
