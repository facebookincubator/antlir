# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# This file redeclares (and potentially validates) JUST the part of the
# fbcode macro API that is allowed within `antlir/`.  This way,
# FB-internal contributors will be less likely to accidentally break
# open-source by starting to use un-shimmed features.
load(":build_defs_impl.bzl", "shim")

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
    # `antlir_rule` is forwarded to build_defs_impl.bzl and is used to mark
    # rules as "antlir-private", "user-internal", or "user-facing".  Read
    # the comments in that file for the detailed rationale.
    return {k: 1 for k in lst + ["antlir_rule"]}

_CPP_BINARY_KWARGS = _make_rule_kwargs_dict(
    [
        "compiler_flags",
        "deps",
        "external_deps",
        "labels",
        "link_style",
        "linker_flags",
        "name",
        "srcs",
        "tags",
        "visibility",
    ],
)

def cpp_binary(*args, **kwargs):
    _check_args("cpp_binary", args, kwargs, _CPP_BINARY_KWARGS)
    shim.cpp_binary(**kwargs)

_CPP_LIBRARY_KWARGS = _make_rule_kwargs_dict(
    [
        "compiler_flags",
        "deps",
        "exported_headers",
        "external_deps",
        "header_namespace",
        "headers",
        "include_directories",
        "labels",
        "linker_flags",
        "name",
        "preferred_linkage",
        "srcs",
        "tags",
        "visibility",
    ],
)

def cpp_library(*args, **kwargs):
    _check_args("cpp_library", args, kwargs, _CPP_LIBRARY_KWARGS)
    shim.cpp_library(**kwargs)

_CPP_UNITTEST_KWARGS = _make_rule_kwargs_dict(
    [
        "deps",
        "env",
        "external_deps",
        "headers",
        "labels",
        "name",
        "owner",
        "srcs",
        "tags",
        "visibility",
    ],
)

def cpp_unittest(*args, **kwargs):
    _check_args("cpp_unittest", args, kwargs, _CPP_UNITTEST_KWARGS)
    shim.cpp_unittest(**kwargs)

_CXX_GENRULE_KWARGS = _make_rule_kwargs_dict(
    [
        "cmd",
        "labels",
        "name",
        "out",
        "srcs",
        "tags",
        "type",
        "visibility",
    ],
)

def cxx_genrule(*args, **kwargs):
    _check_args("cxx_genrule", args, kwargs, _CXX_GENRULE_KWARGS)
    shim.cxx_genrule(**kwargs)

_PYTHON_BINARY_KWARGS = _make_rule_kwargs_dict(
    [
        "base_module",
        "deps",
        "labels",
        "main_module",
        "name",
        "package_style",
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
        "base_module",
        "deps",
        "labels",
        "name",
        "resources",
        "runtime_deps",
        "srcs",
        "tags",
        "type_stubs",
        "visibility",
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
        "flavor",
        "labels",
        "main_module",
        "name",
        "needed_coverage",
        "package_style",
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

def _third_party_libraries(names, platform = None):
    return [
        shim.third_party.library(name, platform = platform)
        for name in names
    ]

def _rust_common(rule, **kwargs):
    rustc_flags = kwargs.pop("rustc_flags", [])
    if not kwargs.pop("allow_unused_crate_dependencies", False):
        rustc_flags.append("--forbid=unused_crate_dependencies")
    rustc_flags.append("--warn=clippy::unwrap_used")
    # @oss-disable

    # always handled by the antlir macros themselves
    if rule != shim.rust_unittest and rule != shim.rust_python_extension and rule != shim.rust_bindgen_library:
        kwargs["unittests"] = False

    if shim.is_facebook:
        rustc_flags.append("--cfg=facebook")
        kwargs["deps"] = list(kwargs.pop("deps", [])) + list(kwargs.pop("fb_deps", []))
        if kwargs.get("unittests", False):
            kwargs["test_deps"] = list(kwargs.pop("test_deps", [])) + list(kwargs.pop("fb_test_deps", []))

    kwargs["rustc_flags"] = rustc_flags

    deps = [_normalize_rust_dep(d) for d in kwargs.pop("deps", [])]
    rule(deps = deps, **kwargs)

def rust_python_extension(**kwargs):
    _rust_common(shim.rust_python_extension, **kwargs)

def rust_library(**kwargs):
    kwargs, test_kwargs = _split_rust_kwargs(kwargs)
    _rust_common(shim.rust_library, **kwargs)
    _rust_implicit_test(kwargs, test_kwargs)

def rust_binary(**kwargs):
    kwargs, test_kwargs = _split_rust_kwargs(kwargs)
    _rust_common(shim.rust_binary, **kwargs)
    _rust_implicit_test(kwargs, test_kwargs)

def rust_unittest(**kwargs):
    _rust_common(shim.rust_unittest, **kwargs)

def rust_bindgen_library(**kwargs):
    _rust_common(shim.rust_bindgen_library, **kwargs)

def _rust_implicit_test(kwargs, test_kwargs):
    if kwargs.pop("unittests", True):
        test_kwargs["name"] = kwargs["name"] + "-unittest"
        test_kwargs["crate"] = kwargs.get("crate") or kwargs["name"].replace("-", "_")
        test_kwargs.pop("autocargo", None)
        test_kwargs.pop("doctests", None)
        test_kwargs.pop("link_style", None)
        test_kwargs.pop("linker_flags", None)
        test_kwargs["srcs"] = test_kwargs.get("srcs", []) + kwargs.get("srcs", [])
        test_kwargs["deps"] = test_kwargs.get("deps", []) + kwargs.get("deps", [])
        _rust_common(shim.rust_unittest, **test_kwargs)

def _split_rust_kwargs(kwargs):
    test_kwargs = dict(kwargs)
    test_kwargs = {k: v for k, v in test_kwargs.items() if not k.startswith("test_")}
    test_kwargs.update({k[len("test_"):]: v for k, v in kwargs.items() if k.startswith("test_")})
    kwargs = {k: v for k, v in kwargs.items() if not k.startswith("test_")}

    return kwargs, test_kwargs

def _normalize_rust_dep(dep):
    if ":" in dep:
        return dep
    return shim.third_party.library(dep, platform = "rust")

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
is_facebook = shim.is_facebook
get_cxx_platform_for_current_buildfile = shim.get_cxx_platform_for_current_buildfile
do_not_use_repo_cfg = shim.do_not_use_repo_cfg
rpm_vset = shim.rpm_vset
repository_name = shim.repository_name
target_utils = shim.target_utils
alias = shim.alias
add_test_framework_label = shim.add_test_framework_label
third_party = struct(
    library = shim.third_party.library,
    source = shim.third_party.source,
    libraries = _third_party_libraries,
)
thrift_library = shim.thrift_library
