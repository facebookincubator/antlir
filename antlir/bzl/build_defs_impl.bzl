# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT
# @lint-ignore-every BUCKRESTRICTEDSYNTAX

def _third_party_library(project, rule = None, platform = None):
    """
    In FB land we want to find the correct build for a third-party target
    based on the current platform.

    If the platform is a python libary, use the pyfi infrastructure so
    there is no need for `external_deps` in python targets.
    """

    if not rule:
        rule = project

    if platform == "python" or platform == "pypi":
        if not rule == project:
            fail("python dependencies must omit rule or be identical to project")
        return "antlir//third-party/python:" + project

    if platform == "rust":
        if not rule == project:
            fail("rust dependencies must omit rule or be identical to project")

        return "antlir//third-party/rust:" + project

    if platform == "antlir":
        return "//third-party/antlir/{project}:{rule}".format(
            project = project,
            rule = rule,
        )

    fail("unsupported in OSS")

def _third_party_source(project, rule = "tarball"):
    """
    For FB the there is only one target flavor for third party sources,
    imported using mgt in @fbsource//third-party/antlir-build.

    Return a target path based on that.
    """
    return "antlir//third-party/source/{}:{}".format(project, rule)

def _get_visibility(visibility = None):
    """
    Antlir build outputs should not be visible outside of antlir by default.
    This helps prevent our abstractions from leaking into other codebases as
    Antlir becomes more widely adopted.
    """
    package = native.package_name()

    # packages in antlir/staging are only allowed to be used by other targets in
    # antlir/staging
    if package == "antlir/staging" or package.startswith("antlir/staging/"):
        return ["//antlir/staging/...", "//bot_generated/antlir/staging/..."]

    if visibility:
        return visibility

    # if it's a consumer of antlir macros outside of antlir, default to public
    return ["PUBLIC"]

def _wrap_internal(fn, args, kwargs):
    """
    Wrap a build target rule with some default attributes.
    """

    label_arg = "labels"

    # Callers outside of this module can specify  `label_arg`, in which
    # case it's read-only, so generate a new list with its contents.
    # We pull off both `labels` and `tags` just to make sure that we get both
    # and then recombine them into the expected arg name.
    kwargs[label_arg] = kwargs.pop("labels", []) + kwargs.pop("tags", []) + ["antlir_macros"]
    # TODO: kill the 'antlir_macros' label - it means that all genrules get
    # forced to local-only. Unfortunately tons and tons of them actually need to
    # be run locally, but we can do better when more antlir1-isms are gone.

    # Antlir build outputs should not be visible outside of antlir by default. This
    # helps prevent our abstractions from leaking into other codebases as Antlir
    # becomes more widely adopted.
    kwargs["visibility"] = _get_visibility(kwargs.pop("visibility", []))

    fn(*args, **kwargs)

def _buck_command_alias(*args, **kwargs):
    _wrap_internal(native.command_alias, args, kwargs)

def _alias(*args, **kwargs):
    _wrap_internal(native.alias, args, kwargs)

def _buck_filegroup(*args, **kwargs):
    _wrap_internal(native.filegroup, args, kwargs)

def _buck_genrule(*args, **kwargs):
    # This is unused in FB
    kwargs.pop("flavor_config", None)
    if "out" not in kwargs:
        kwargs["out"] = "out"
    existing_labels = kwargs.get("labels", [])

    # This hides these targets from being built by Pyre, which is beneficial as
    # the majority of genrules in Antlir are related to image compilation and
    # thus require root, which Pyre builds do not have
    if "no_pyre" not in existing_labels:
        kwargs["labels"] = existing_labels + ["no_pyre"]
    _wrap_internal(native.genrule, args, kwargs)

def _buck_sh_binary(*args, **kwargs):
    _wrap_internal(native.sh_binary, args, kwargs)

def _buck_sh_test(*args, **kwargs):
    _wrap_internal(native.sh_test, args, kwargs)

def _cpp_binary(*args, **kwargs):
    _wrap_internal(native.cxx_binary, args, kwargs)

def _cpp_library(*args, **kwargs):
    _wrap_internal(native.cxx_library, args, kwargs)

def _cpp_unittest(*args, **kwargs):
    _wrap_internal(native.cxx_test, args, kwargs)

def _cxx_genrule(*args, **kwargs):
    _wrap_internal(native.cxx_genrule, args, kwargs)

def _export_file(*args, **kwargs):
    _wrap_internal(native.export_file, args, kwargs)

def _http_file(*args, **kwargs):
    native.http_file(*args, **kwargs)

def _http_archive(*args, **kwargs):
    native.http_archive(*args, **kwargs)

def _invert_dict(x):
    if type(x) != type({}):
        return x
    return {v: k for k, v in x.items()}

def _python_library(*, **kwargs):
    kwargs["srcs"] = _invert_dict(kwargs.pop("srcs", []))
    kwargs["resources"] = _invert_dict(kwargs.pop("resources", []))
    _wrap_internal(native.python_library, [], kwargs)

def _python_binary(
        *,
        name: str,
        main_function: str | None = None,
        main_module: str | None = None,
        **kwargs):
    _python_library(
        name = name + "-library",
        **kwargs
    )

    _wrap_internal(native.python_binary, [], {
        "deps": [":{}-library".format(name)],
        "main_function": main_function,
        "main_module": main_module,
        "name": name,
    })

def _antlir_buck_env():
    return "buck2"

def _python_unittest(*args, **kwargs):
    env = kwargs.get("env", {})
    env["ANTLIR_BUCK"] = _antlir_buck_env()
    kwargs["env"] = env

    # tests setting `resources` are very likely to be taking a dep on a layer, so just
    # unconditionally skip Pyre in this case. We can't query whether or not this is the
    # case from this context, and having an Antlir target take a dep on a layer
    # otherwise breaks Pyre.
    if "resources" in kwargs and "no_pyre" not in kwargs.setdefault("labels", []):
        kwargs["labels"].append("no_pyre")

    kwargs["srcs"] = _invert_dict(kwargs.pop("srcs", []))
    kwargs["resources"] = _invert_dict(kwargs.pop("resources", []))

    kwargs.pop("supports_static_listing", None)

    _wrap_internal(native.python_test, args, kwargs)

def _rust_unittest(*args, **kwargs):
    kwargs.pop("nodefaultlibs", None)
    _wrap_internal(native.rust_test, args, kwargs)

def _rust_binary(*, name: str, **kwargs):
    unittests = kwargs.pop("unittests", True)
    if unittests:
        _rust_unittest(name = name + "-unittests", **kwargs)
    kwargs["name"] = name
    kwargs.pop("nodefaultlibs", None)
    _wrap_internal(native.rust_binary, [], kwargs)

def _rust_library(*, name: str, **kwargs):
    unittests = kwargs.pop("unittests", True)
    if unittests:
        _rust_unittest(name = name + "-unittests", **kwargs)
    kwargs["name"] = name
    kwargs.pop("autocargo", None)
    kwargs.pop("link_style", None)
    _wrap_internal(native.rust_library, [], kwargs)

def _rust_bindgen_library(name: str, header: str, **kwargs):
    _buck_genrule(
        name = name + "--bindings.rs",
        out = "bindings.rs",
        bash = """
            $(exe antlir//third-party/rust/bindgen:bindgen) --header "$SRCS" --out $OUT
        """,
        srcs = [header],
        visibility = [],
    )
    _rust_library(
        name = name,
        mapped_srcs = {
            ":{}--bindings.rs".format(name): "src/lib.rs",
        },
        deps = kwargs.pop("cpp_deps", []),
        visibility = kwargs.pop("visibility", []),
    )

def _rust_python_extension(name: str, **kwargs):
    native.alias(
        name = name,
        actual = "//antlir:empty",
    )
    print("TODO: rust_python_extension")

def _thrift_library(**kwargs):
    fail("not implemented")

### BEGIN COPY-PASTA (@fbcode_macros//build_defs/lib:target_utils.bzl)
def _parse_target(target, default_repo = None, default_base_path = None):
    if target.count(":") != 1:
        fail('rule name must contain exactly one ":": "{}"'.format(target))

    repo_and_base_path, name = target.split(":")

    # Parse relative target.
    if not repo_and_base_path:
        return struct(repo = default_repo, base_path = default_base_path, name = name)

    # Parse absolute targets.
    if repo_and_base_path.count("//") != 1:
        fail('absolute rule name must contain one "//" before ":": "{}"'.format(target))
    repo, base_path = repo_and_base_path.split("//", 1)
    repo = repo or default_repo

    return struct(repo = repo, base_path = base_path, name = name)

def _to_label(repo, path, name):
    return "{}//{}:{}".format(repo, path, name)

### END COPY-PASTA

# Please keep each section lexicographically sorted.
shim = struct(
    #
    # Rules -- IMPORTANT -- wrap **ALL** rules with `_wrap_internal`
    #
    alias = _alias,
    buck_command_alias = _buck_command_alias,
    buck_filegroup = _buck_filegroup,
    buck_genrule = _buck_genrule,
    buck_sh_binary = _buck_sh_binary,
    buck_sh_test = _buck_sh_test,
    cpp_binary = _cpp_binary,
    cpp_library = _cpp_library,
    cpp_unittest = _cpp_unittest,
    is_buck2 = lambda: True,
    is_facebook = False,
    cxx_genrule = _cxx_genrule,
    export_file = _export_file,
    http_file = _http_file,
    http_archive = _http_archive,
    python_binary = _python_binary,
    python_library = _python_library,
    python_unittest = _python_unittest,
    rust_binary = _rust_binary,
    rust_bindgen_library = _rust_bindgen_library,
    rust_library = _rust_library,
    rust_python_extension = _rust_python_extension,
    rust_unittest = _rust_unittest,
    thrift_library = _thrift_library,
    #
    # Utility functions
    #
    antlir_buck_env = _antlir_buck_env,
    config = struct(
        get_platform_for_current_buildfile = lambda: struct(target_platform = None),
    ),
    get_visibility = _get_visibility,
    target_utils = struct(
        parse_target = _parse_target,
        to_label = _to_label,
    ),
    third_party = struct(
        library = _third_party_library,
        source = _third_party_source,
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
        "host_mounts_for_repo_artifacts": " ".join([
            "/mnt/gvfs",
        ]),
    },
    add_test_framework_label = lambda labels, add: labels + [add],
)
