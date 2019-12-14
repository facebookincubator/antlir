# This file redeclares (and potentially validates) JUST the part of the
# fbcode macro API that is allowed within `fs_image/`.  This way,
# FB-internal contributors will be less likely to accidentally break
# open-source by starting to use un-shimmed features.
load(":oss_shim_impl.bzl", "shim")

def _check_args(rule, args, kwargs, allowed_kwargs):
    if args:
        fail("use kwargs")
    for kwarg in kwargs:
        if kwarg not in allowed_kwargs:
            fail("kwarg {} is not supported by {}".format(
                kwarg,
                rule,
            ))

def _setify(l):
    return {k: 1 for k in l}

_CPP_UNITTEST_KWARGS = _setify(
    ["name", "deps", "env", "srcs", "tags", "use_default_test_main", "visibility"],
)

def cpp_unittest(*args, **kwargs):
    _check_args("cpp_unittest", args, kwargs, _CPP_UNITTEST_KWARGS)
    shim.cpp_unittest(**kwargs)

_PYTHON_BINARY_KWARGS = _setify(
    ["name", "base_module", "deps", "main_module", "par_style", "resources", "srcs", "visibility"],
)

def python_binary(*args, **kwargs):
    _check_args("python_binary", args, kwargs, _PYTHON_BINARY_KWARGS)
    shim.python_binary(**kwargs)

_PYTHON_LIBRARY_KWARGS = _setify(
    ["name", "base_module", "deps", "external_deps", "resources", "srcs", "visibility"],
)

def python_library(*args, **kwargs):
    _check_args("python_library", args, kwargs, _PYTHON_LIBRARY_KWARGS)
    shim.python_library(**kwargs)

_PYTHON_UNITTEST_KWARGS = _setify(
    [
        "base_module",
        "check_types",
        "cpp_deps",
        "deps",
        "external_deps",
        "env",
        "main_module",
        "name",
        "needed_coverage",
        "par_style",
        "resources",
        "srcs",
        "tags",
        "visibility",
    ],
)

def python_unittest(*args, **kwargs):
    _check_args("python_unittest", args, kwargs, _PYTHON_UNITTEST_KWARGS)
    shim.python_unittest(**kwargs)

buck_command_alias = shim.buck_command_alias
buck_genrule = shim.buck_genrule
buck_sh_binary = shim.buck_sh_binary
config = shim.config
get_visibility = shim.get_visibility
target_utils = shim.target_utils
third_party = shim.third_party
