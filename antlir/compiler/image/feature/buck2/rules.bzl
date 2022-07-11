# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":generate_feature_target_name.bzl", "generate_feature_target_name")
load(":providers.bzl", "feature_provider", "rpm_provider")

def _feature_rule_impl(ctx: "context") -> ["provider"]:
    return feature_provider(
        ctx.attr.key,
        ctx.attr.shape,
    )

_feature_rule = rule(
    implementation = _feature_rule_impl,
    attrs = {
        "deps": attr.list(attr.dep(), default = []),

        # corresponds to keys in `ItemFactory` in items_for_features.py
        "key": attr.string(),

        # gets serialized to json when `feature.new` is called and used as
        # kwargs in compiler `ItemFactory`
        "shape": attr.dict(attr.string(), attr.any()),

        # for query
        "type": attr.string(default = "image_feature"),
    },
)

def maybe_add_feature_rule(
        name,
        feature_shape,
        include_in_target_name = None,
        key = None,
        deps = []):
    # if `key` is not provided, then it is assumed that `key` is same as `name`
    key = key or name

    target_name = generate_feature_target_name(
        name = name,
        key = key,
        feature_shape = feature_shape,
        include_in_name = include_in_target_name,
    )

    if not native.rule_exists(target_name):
        _feature_rule(
            name = target_name,
            key = key,
            shape = shape.as_serializable_dict(feature_shape),
            deps = deps,
        )

    return ":" + target_name

def _rpm_rule_impl(ctx: "context") -> ["provider"]:
    return rpm_provider(
        ctx.attr.rpm_items,
        ctx.attr.action,
        ctx.attr.flavors,
    )

_rpm_rule = rule(
    implementation = _rpm_rule_impl,
    attrs = {
        "action": attr.string(),
        "deps": attr.list(attr.dep(), default = []),

        # flavors specified in call to `image.rpms_{install,remove_if_exists}`
        "flavors": attr.list(attr.string(), default = []),

        # gets serialized to json when `feature.new` is called and used as
        # kwargs in compiler `ItemFactory`
        "rpm_items": attr.list(attr.dict(attr.string(), attr.any())),

        # for query
        "type": attr.string(default = "image_feature"),
    },
)

def maybe_add_rpm_rule(
        name,
        rpm_items,
        flavors,
        include_in_target_name = None,
        deps = []):
    key = "rpms"

    target_name = generate_feature_target_name(
        name = name,
        key = key,
        feature_shape = rpm_items,
        include_in_name = include_in_target_name,
    )

    if not native.rule_exists(target_name):
        _rpm_rule(
            name = target_name,
            action = name,
            rpm_items = shape.as_serializable_dict(rpm_items),
            flavors = flavors,
            deps = deps,
        )

    return ":" + target_name

def _wrap_executable_target_rule_impl(ctx: "context") -> ["provider"]:
    if not ctx.attr.target[RunInfo]:
        return [DefaultInfo()]

    path_in_output = \
        "/" + ctx.attr.path_in_output if ctx.attr.path_in_output else ""

    create_wrapper_script = ctx.actions.declare_output("create_wrapper.sh")
    output = ctx.actions.declare_output("out")

    script = """
set -exo pipefail
echo "#!/bin/bash
REPO_ROOT=`$repo_root`
{unquoted_heredoc_preamble}
$literal_preamble
exec \\$REPO_ROOT/$runnable$path_in_output {args}" > $OUT
chmod +x $OUT
    """.format(
        # Necessary because script generated here differs from that generated in
        # `exec_wrapper.bzl`, which uses the same thing
        unquoted_heredoc_preamble = ctx.attr.unquoted_heredoc_preamble.replace(
            "\\$(date)",
            "$(date)",
        ),
        args = '"\\$@"',
    )
    ctx.actions.write(
        create_wrapper_script,
        script,
    )

    ctx.actions.run(
        cmd_args(["/bin/bash", create_wrapper_script]),
        env = {
            "OUT": output.as_output(),
            "literal_preamble": ctx.attr.literal_preamble,
            "path_in_output": path_in_output,
            "repo_root": ctx.attr.repo_root[RunInfo],
            "runnable": ctx.attr.target[RunInfo],
        },
        # See comment at https://fburl.com/code/3pj7exvp
        local_only = True,
        category = "wrap_executable_target",
        identifier = "create_wrapper",
    )

    return [DefaultInfo(default_outputs = [output])]

_wrap_executable_target_rule = rule(
    implementation = _wrap_executable_target_rule_impl,
    attrs = {
        "literal_preamble": attr.arg(),
        "path_in_output": attr.string(default = ""),
        "repo_root": attr.dep(),
        "target": attr.dep(),
        "unquoted_heredoc_preamble": attr.string(),
    },
)

def maybe_wrap_executable_target_rule(**kwargs):
    if not native.rule_exists(kwargs.get("name")):
        _wrap_executable_target_rule(
            repo_root = antlir_dep(":repo-root"),
            **kwargs
        )

    return ":" + kwargs.get("name")

def _install_rule_impl(ctx: "context") -> ["provider"]:
    if ctx.attr.is_executable and ctx.attr.unwrapped_target[RunInfo]:
        install_shape = ctx.attr.wrapped_shape
    else:
        install_shape = ctx.attr.unwrapped_shape

    return feature_provider(
        ctx.attr.key,
        install_shape,
    )

_install_rule = rule(
    implementation = _install_rule_impl,
    attrs = {
        "is_executable": attr.bool(),
        "key": attr.string(),

        # for query
        "type": attr.string(default = "image_feature"),
        "unwrapped_shape": attr.dict(attr.string(), attr.any()),
        "unwrapped_target": attr.dep(),
        "wrapped_shape": attr.dict(attr.string(), attr.any()),
        "wrapped_target": attr.dep(),
    },
)

def maybe_add_install_rule(
        unwrapped_shape,
        wrapped_shape,
        unwrapped_target,
        wrapped_target,
        is_executable,
        include_in_target_name = None):
    name = "install"
    key = "install_files"

    target_name = generate_feature_target_name(
        name = name,
        key = key,
        feature_shape = unwrapped_shape,
        include_in_name = include_in_target_name,
    )

    if not native.rule_exists(target_name):
        _install_rule(
            name = target_name,
            key = key,
            unwrapped_shape = shape.as_serializable_dict(unwrapped_shape),
            wrapped_shape = shape.as_serializable_dict(wrapped_shape),
            unwrapped_target = unwrapped_target,
            wrapped_target = wrapped_target,
            is_executable = is_executable,
        )

    return ":" + target_name
