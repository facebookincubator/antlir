# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:anon_helpers.bzl", "anon_helpers")

def _anon_impl(ctx):
    return [DefaultInfo(ctx.actions.write_json("out.json", ctx.attrs))]

_anon, _anon_rule = anon_helpers.new_rule(
    impl = _anon_impl,
    attrs = {
        "cpu": attrs.string(default = select(
            {
                "DEFAULT": "unknown",
                "ovr_config//cpu:arm64": "arm64",
                "ovr_config//cpu:x86_64": "x86_64",
            },
        )),
        "int": attrs.int(default = 42),
        "str": attrs.string(default = "hello"),
    },
)

def _outer_impl(ctx):
    def _map(x):
        return [x[DefaultInfo]]

    kwargs = {}
    if ctx.attrs.cpu:
        kwargs["cpu"] = ctx.attrs.cpu

    return _anon.anon_target(
        ctx = ctx,
        **kwargs
    ).promise.map(_map)

outer = rule(
    impl = _outer_impl,
    attrs = {
        "cpu": attrs.option(attrs.string(), default = None),
    } | _anon.default_outer_attrs,
)
