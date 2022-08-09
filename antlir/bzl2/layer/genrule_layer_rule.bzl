# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT

"See the docs in antlir/website/docs/genrule-layer.md"

load("//antlir/bzl:dummy_rule.bzl", "dummy_rule")
load("//antlir/bzl:genrule_layer.shape.bzl", "genrule_layer_t")
load("//antlir/bzl:oss_shim.bzl", "is_buck2")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl2:generate_feature_target_name.bzl", "generate_feature_target_name")
load("//antlir/bzl2:providers.bzl", "ItemInfo")

def _genrule_layer_rule_impl(ctx):
    genrule_feature_dict = dict(ctx.attrs.feature_dict)
    genrule_feature_dict["cmd"] = native.cmd_args(ctx.attrs.cmd)

    return [
        native.DefaultInfo(),
        ItemInfo(items = struct(**{"genrule_layer": [genrule_feature_dict]})),
    ]

_genrule_layer_rule = native.rule(
    impl = _genrule_layer_rule_impl,
    attrs = {
        # `attrs.arg()` ensures that any macros in `cmd` get expanded
        "cmd": native.attrs.list(native.attrs.arg()),
        "deps": native.attrs.list(native.attrs.dep(), default = []),
        "feature_dict": native.attrs.dict(native.attrs.string(), native.attrs.any()),

        # for query
        "type": native.attrs.string(default = "image_feature"),
    },
) if is_buck2() else None

def maybe_add_genrule_layer_rule(
        cmd,
        user,
        container_opts,
        bind_repo_ro,
        boot,
        include_in_target_name = None,
        debug = False,
        is_buck2 = True):
    """
    Seperate rule from generic feature rule is needed because we need to pass
    the `cmd` field in separately from the rest of the shape to expand the
    macros contained within.
    """

    feature_shape = genrule_layer_t(
        cmd = cmd,
        user = user,
        container_opts = container_opts,
        bind_repo_ro = bind_repo_ro,
        boot = boot,
    )

    name = "genrule_layer"
    key = name
    target_name = generate_feature_target_name(
        name = name,
        key = key,
        feature_shape = feature_shape,
        include_in_name = include_in_target_name if debug else None,
    )

    genrule_feature_dict, extra_deps = shape.as_dict_collect_deps(feature_shape)
    cmd = genrule_feature_dict.pop("cmd")

    if not native.rule_exists(target_name):
        if is_buck2:
            _genrule_layer_rule(
                name = target_name,
                feature_dict = genrule_feature_dict,
                deps = extra_deps,
                cmd = cmd,
            )
        else:
            dummy_rule(
                target_name,
                deps = extra_deps,
            )

    return ":" + target_name
