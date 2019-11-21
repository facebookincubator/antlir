load("@fbcode_macros//build_defs:config.bzl", "config")
load("@fbcode_macros//build_defs:cpp_unittest.bzl", "cpp_unittest")
load("@fbcode_macros//build_defs:custom_rule.bzl", "get_project_root_from_gen_dir")
load("@fbcode_macros//build_defs:native_rules.bzl", "buck_command_alias", "buck_genrule")
load("@fbcode_macros//build_defs:python_binary.bzl", "python_binary")
load("@fbcode_macros//build_defs:python_library.bzl", "python_library")
load("@fbcode_macros//build_defs:python_unittest.bzl", "python_unittest")
load("@fbcode_macros//build_defs/lib:target_utils.bzl", "target_utils")
load("@fbcode_macros//build_defs/lib:visibility.bzl", "get_visibility")

shim = struct(
    buck_command_alias = buck_command_alias,
    buck_genrule = buck_genrule,
    config = struct(
        get_current_repo_name = config.get_current_repo_name,
        get_project_root_from_gen_dir = get_project_root_from_gen_dir,
    ),
    cpp_unittest = cpp_unittest,
    get_visibility = get_visibility,
    python_binary = python_binary,
    python_library = python_library,
    python_unittest = python_unittest,
    target_utils = struct(
        parse_target = target_utils.parse_target,
        to_label = target_utils.to_label,
    ),
)
