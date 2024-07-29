# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:selects.bzl", "selects")

# This file redeclares (and potentially validates) JUST the part of the
# fbcode macro API that is allowed within `antlir/`.  This way,
# FB-internal contributors will be less likely to accidentally break
# open-source by starting to use un-shimmed features.
load(":build_defs_impl.bzl", "shim")

def _third_party_libraries(names, platform = None):
    return [
        shim.third_party.library(name, platform = platform)
        for name in names
    ]

def _rust_common(rule, **kwargs):
    rustc_flags = kwargs.pop("rustc_flags", [])
    append = [
        "--warn=clippy::unwrap_used",
        # @oss-disable
    ]
    if not kwargs.pop("allow_unused_crate_dependencies", False):
        append.append("--forbid=unused_crate_dependencies")
    rustc_flags = selects.apply(rustc_flags, lambda rustc_flags: rustc_flags + append)
    kwargs["rustc_flags"] = rustc_flags

    # always handled by the antlir macros themselves
    if rule != shim.rust_unittest and rule != shim.rust_python_extension and rule != shim.rust_bindgen_library:
        kwargs["unittests"] = False

    if shim.is_facebook:
        kwargs["deps"] = selects.apply(kwargs.pop("deps", []), lambda deps: deps + list(kwargs.pop("fb_deps", [])))
        if kwargs.get("unittests", False):
            kwargs["test_deps"] = list(kwargs.pop("test_deps", [])) + list(kwargs.pop("fb_test_deps", []))
        else:
            kwargs.pop("fb_test_deps", None)
    else:
        kwargs.pop("fb_deps", None)
        kwargs.pop("fb_test_deps", None)

    deps = selects.apply(kwargs.pop("deps", []), lambda deps: [_normalize_rust_dep(d) for d in deps])
    rule(deps = deps, **kwargs)

def rust_python_extension(**kwargs):
    _rust_common(shim.rust_python_extension, **kwargs)

def rust_library(**kwargs):
    kwargs, test_kwargs = _split_rust_kwargs(kwargs)
    _rust_common(shim.rust_library, **kwargs)
    _rust_implicit_test(kwargs, test_kwargs)

def rust_binary(**kwargs):
    kwargs.setdefault("link_style", "static")
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

def _echo(ctx: AnalysisContext) -> list[Provider]:
    artifact = ctx.actions.write(ctx.label.name, ctx.attrs.content)
    return [DefaultInfo(default_output = artifact)]

echo = rule(
    impl = _echo,
    attrs = {"content": attrs.string()},
)

def internal_external(*, fb, oss):
    if is_facebook:
        return fb
    else:
        return oss

cpp_binary = shim.cpp_binary
cpp_library = shim.cpp_library
cpp_unittest = shim.cpp_unittest
cxx_genrule = shim.cxx_genrule
python_binary = shim.python_binary
python_library = shim.python_library
python_unittest = shim.python_unittest
buck_command_alias = shim.buck_command_alias
buck_filegroup = shim.buck_filegroup
buck_genrule = shim.buck_genrule
buck_sh_binary = shim.buck_sh_binary
buck_sh_test = shim.buck_sh_test
config = shim.config
export_file = shim.export_file
get_visibility = shim.get_visibility
http_file = shim.http_file
http_archive = shim.http_archive
is_buck2 = shim.is_buck2
is_facebook = shim.is_facebook
do_not_use_repo_cfg = shim.do_not_use_repo_cfg
target_utils = shim.target_utils
alias = shim.alias
add_test_framework_label = shim.add_test_framework_label
third_party = struct(
    library = shim.third_party.library,
    source = shim.third_party.source,
    libraries = _third_party_libraries,
)
