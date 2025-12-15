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
load(":internal_external.bzl", "is_facebook")

def _third_party_libraries(names, platform = None):
    return [
        shim.third_party.library(name, platform = platform)
        for name in names
    ]

def _ensure_dep_is_public(dep: str):
    package = native.package_name()
    if not package.startswith("antlir"):
        # TODO: apply this same check to metalos
        return dep

    # don't run this check on non-shipped directories
    package = package.split("/")
    for fb_path in ("facebook", "fb", "fbpkg"):
        if fb_path in package:
            return dep

    # This covers deps in the same target as well as unqualified third party
    # dependencies
    if "//" not in dep:
        return dep
    if dep.startswith("fbsource//third-party/rust:"):
        fail("Do not use fbsource//third-party/rust:, instead just use '{}'".format(dep.removeprefix("fbsource//third-party/rust:")))
    fbcode_dep = dep.removeprefix("fbcode").removeprefix("antlir")
    if fbcode_dep.startswith("//antlir") or fbcode_dep.startswith("//metalos"):
        return dep
    else:
        fail("internal-only dependency '{}' must be moved to fb_deps (and its usage must be conditional on #[cfg(facebook)])".format(dep))

def _rust_common(rule, **kwargs):
    rustc_flags = kwargs.pop("rustc_flags", [])
    append = [
        "--warn=clippy::unwrap_used",
        # @oss-disable[end= ]: "--cfg=facebook",
    ]
    if not kwargs.pop("allow_unused_crate_dependencies", False):
        append.append("--forbid=unused_crate_dependencies")
    rustc_flags = selects.apply(rustc_flags, lambda rustc_flags: rustc_flags + append)
    kwargs["rustc_flags"] = rustc_flags

    # always handled by the antlir macros themselves
    if rule != shim.rust_unittest and rule != shim.rust_python_extension and rule != shim.rust_bindgen_library:
        kwargs["unittests"] = False

    if is_facebook:
        deps = selects.apply(kwargs.pop("deps", []), lambda deps: [_ensure_dep_is_public(dep) for dep in deps])
        kwargs["deps"] = selects.apply(deps, lambda deps: deps + list(kwargs.pop("fb_deps", [])))
        if kwargs.get("unittests", False):
            kwargs["test_deps"] = list(kwargs.pop("test_deps", [])) + list(kwargs.pop("fb_test_deps", []))
        else:
            kwargs.pop("fb_test_deps", None)
    else:
        kwargs.pop("fb_deps", None)
        kwargs.pop("fb_test_deps", None)

    deps = selects.apply(kwargs.pop("deps", []), lambda deps: [_normalize_rust_dep(d) for d in deps])
    named_deps = selects.apply(kwargs.pop("named_deps", {}), lambda named_deps: {
        key: _normalize_rust_dep(_ensure_dep_is_public(d))
        for key, d in (named_deps or {}).items()
    })
    rule(deps = deps, named_deps = named_deps, **kwargs)

def rust_python_extension(**kwargs):
    _rust_common(shim.rust_python_extension, **kwargs)

def rust_library(**kwargs):
    kwargs, test_kwargs = _split_rust_kwargs(kwargs)
    _rust_common(shim.rust_library, **kwargs)
    _rust_implicit_test(kwargs, test_kwargs)

def rust_binary(**kwargs):
    kwargs.setdefault("link_style", "static")

    # Use malloc here so that we can avoid issues with jemalloc's use of background threads
    # and unshare_userns.
    kwargs.setdefault("allocator", "malloc")
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
        test_kwargs["fb_deps"] = test_kwargs.get("fb_deps", []) + kwargs.get("fb_deps", [])
        _rust_common(shim.rust_unittest, **test_kwargs)

def _is_rust_test_key(key):
    return key.startswith("test_") or key.startswith("fb_test_")

def _rust_test_key_to_regular_key(key):
    if key.startswith("test_"):
        return key[5:]
    if key.startswith("fb_test_"):
        return "fb_" + key[8:]
    return key

def _split_rust_kwargs(kwargs):
    test_kwargs = {k: v for k, v in kwargs.items() if not _is_rust_test_key(k)}
    test_kwargs.update({_rust_test_key_to_regular_key(k): v for k, v in kwargs.items() if _is_rust_test_key(k)})
    kwargs = {k: v for k, v in kwargs.items() if not _is_rust_test_key(k)}
    return kwargs, test_kwargs

def _normalize_rust_dep(dep):
    if ":" in dep:
        return dep
    return shim.third_party.library(dep, platform = "rust")

cpp_binary = shim.cpp_binary
cpp_library = shim.cpp_library
cpp_unittest = shim.cpp_unittest
cxx_genrule = shim.cxx_genrule
python_binary = shim.python_binary
python_library = shim.python_library
python_unittest = shim.python_unittest
cpp_python_extension = shim.cpp_python_extension
buck_command_alias = shim.buck_command_alias
buck_filegroup = shim.buck_filegroup
buck_genrule = shim.buck_genrule
buck_sh_binary = shim.buck_sh_binary
buck_sh_test = shim.buck_sh_test
config = shim.config
export_file = shim.export_file
write_file = shim.write_file
get_visibility = shim.get_visibility
http_file = shim.http_file
http_archive = shim.http_archive
do_not_use_repo_cfg = shim.do_not_use_repo_cfg
target_utils = shim.target_utils
alias = shim.alias
toolchain_alias = shim.toolchain_alias
add_test_framework_label = shim.add_test_framework_label
third_party = struct(
    library = shim.third_party.library,
    source = shim.third_party.source,
    libraries = _third_party_libraries,
)
