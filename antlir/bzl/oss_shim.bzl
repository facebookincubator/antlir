# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

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

_CPP_BINARY_KWARGS = _make_rule_kwargs_dict(
    ["name", "srcs", "deps", "compiler_flags", "linker_flags", "link_style", "visibility", "external_deps"],
)

def cpp_binary(*args, **kwargs):
    _check_args("cpp_binary", args, kwargs, _CPP_BINARY_KWARGS)
    shim.cpp_binary(**kwargs)

_CPP_LIBRARY_KWARGS = _make_rule_kwargs_dict(
    [
        "name",
        "srcs",
        "deps",
        "compiler_flags",
        "headers",
        "header_namespace",
        "exported_headers",
        "include_directories",
        "linker_flags",
        "preferred_linkage",
        "visibility",
        "external_deps",
    ],
)

def cpp_library(*args, **kwargs):
    _check_args("cpp_library", args, kwargs, _CPP_LIBRARY_KWARGS)
    shim.cpp_library(**kwargs)

_CPP_UNITTEST_KWARGS = _make_rule_kwargs_dict(
    ["name", "deps", "env", "headers", "srcs", "tags", "visibility", "external_deps", "owner"],
)

def cpp_unittest(*args, **kwargs):
    _check_args("cpp_unittest", args, kwargs, _CPP_UNITTEST_KWARGS)
    shim.cpp_unittest(**kwargs)

_CXX_GENRULE_KWARGS = _make_rule_kwargs_dict(
    ["name", "srcs", "cmd", "type", "out", "visibility", "tags"],
)

def cxx_genrule(*args, **kwargs):
    _check_args("cxx_genrule", args, kwargs, _CXX_GENRULE_KWARGS)
    shim.cxx_genrule(**kwargs)

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
        "tags",
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
        "tags",
        "visibility",
        "type_stubs",
    ],
)

def python_library(*args, **kwargs):
    _check_args("python_library", args, kwargs, _PYTHON_LIBRARY_KWARGS)
    shim.python_library(**kwargs)

_PYTHON_UNITTEST_KWARGS = _make_rule_kwargs_dict(
    [
        "base_module",
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
        "flavor",
        "package_style",
    ],
)

def python_unittest(*args, **kwargs):
    _check_args("python_unittest", args, kwargs, _PYTHON_UNITTEST_KWARGS)
    shim.python_unittest(**kwargs)

def _third_party_libraries(names, platform = None):
    return [
        shim.third_party.library(name, platform = platform)
        for name in names
    ]

def rust_python_extension(*args, **kwargs):
    if not kwargs.get("types", []):
        fail("rust_python_extension is taking a step backwards without pyre")
    shim.rust_python_extension(*args, **kwargs)

antlir_buck_env = shim.antlir_buck_env
buck_command_alias = shim.buck_command_alias
buck_filegroup = shim.buck_filegroup
buck_genrule = shim.buck_genrule
buck_sh_binary = shim.buck_sh_binary
buck_sh_test = shim.buck_sh_test
buck_worker_tool = shim.buck_worker_tool
config = shim.config
export_file = shim.export_file
get_visibility = shim.get_visibility
http_file = shim.http_file
http_archive = shim.http_archive
is_buck2 = shim.is_buck2
do_not_use_repo_cfg = shim.do_not_use_repo_cfg
rpm_vset = shim.rpm_vset
repository_name = shim.repository_name
rust_binary = shim.rust_binary
rust_bindgen_library = shim.rust_bindgen_library
rust_library = shim.rust_library
rust_unittest = shim.rust_unittest
target_utils = shim.target_utils
third_party = struct(
    library = shim.third_party.library,
    source = shim.third_party.source,
    libraries = _third_party_libraries,
)
thrift_library = shim.thrift_library
