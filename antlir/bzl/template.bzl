# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/bzl:build_defs.bzl", "get_visibility")

TemplateInfo = provider(fields = {
    "compiled_srcs": Artifact,
    "root": str,
    "templates": dict[str, Artifact],
})

def _impl(ctx: AnalysisContext) -> list[Provider]:
    # attempt to determine a root template in some common cases
    root = ctx.attrs.root
    if not root:
        if len(ctx.attrs.srcs) == 1:
            root = ctx.attrs.srcs[0]
        else:
            for src in ctx.attrs.srcs:
                if src.basename == ctx.label.name or src.basename == ctx.label.name + ".jinja2":
                    root = src
        if not root:
            fail("could not infer 'root' template, please set `root` attr", attr = "root")

    compiled_srcs = {}
    for src in ctx.attrs.srcs:
        compiled = ctx.actions.declare_output(src.short_path + ".py")
        ctx.actions.run(
            cmd_args(
                ctx.attrs._compile_template[RunInfo],
                cmd_args(src, format = "--template={}"),
                cmd_args(src.short_path, format = "--name={}"),
                cmd_args(compiled.as_output(), format = "--out={}"),
            ),
            category = "compile_template",
            identifier = src.short_path,
        )
        compiled_srcs[src.short_path + ".py"] = compiled

    for dep in ctx.attrs.deps:
        for name, t in dep[TemplateInfo].templates.items():
            compiled_srcs[paths.join(dep.label.package, dep.label.name + ":" + name)] = t

    templates = compiled_srcs

    return [
        DefaultInfo(),
        TemplateInfo(
            root = root.short_path,
            templates = templates,
            compiled_srcs = ctx.actions.copied_dir("compiled", templates),
        ),
    ]

_template = rule(
    impl = _impl,
    attrs = {
        "deps": attrs.list(attrs.dep(providers = [TemplateInfo]), default = []),
        "root": attrs.option(attrs.source(), default = None),
        "srcs": attrs.list(attrs.source()),
        "_compile_template": attrs.default_only(attrs.exec_dep(
            providers = [RunInfo],
            default = "antlir//antlir:compile-template",
        )),
    },
    doc = """
        Compile a set of jinja2 template files to a target that can be run to
        render the given root template from JSON data provided on stdin.
    """,
)

_template_macro = rule_with_default_target_platform(_template)

def template(visibility = None, **kwargs):
    _template_macro(
        visibility = get_visibility(visibility),
        **kwargs
    )

def _render_impl(ctx: AnalysisContext) -> list[Provider]:
    data_json = ctx.actions.write("data.json", ctx.attrs.data_json)
    rendered = ctx.actions.declare_output("rendered")
    tmpl = ctx.attrs.template[TemplateInfo]

    ctx.actions.run(
        cmd_args(
            ctx.attrs._render_template[RunInfo],
            cmd_args(tmpl.root, format = "--root={}"),
            cmd_args(tmpl.compiled_srcs, format = "--compiled-templates={}"),
            cmd_args(data_json, format = "--json-file={}"),
            cmd_args(rendered.as_output(), format = "--out={}"),
        ),
        category = "render",
    )
    return [
        DefaultInfo(rendered),
    ]

_render = rule(
    impl = _render_impl,
    attrs = {
        "data_json": attrs.string(),
        "template": attrs.dep(providers = [TemplateInfo]),
        "_render_template": attrs.default_only(attrs.exec_dep(
            providers = [RunInfo],
            default = "antlir//antlir:render-template",
        )),
    },
    doc = """
        Render a template to a file.
    """,
)

_render_macro = rule_with_default_target_platform(_render)

def render(*, instance: typing.Any, visibility = None, **kwargs):
    _render_macro(
        data_json = selects.apply(instance, json.encode),
        visibility = get_visibility(visibility),
        **kwargs
    )
