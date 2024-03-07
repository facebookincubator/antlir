# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/bzl:build_defs.bzl", "buck_genrule", "export_file", "get_visibility", "python_binary", "python_library")
load(":shell.bzl", "shell")

def template(
        name,
        srcs,
        root = None,
        deps = None,
        visibility = None):
    """
Compile a set of jinja2 template files to a library that can be imported by
other templates, as well as a `$name-render` binary that can render the given
root template from JSON data provided on stdin.
"""

    # attempt to determine a root template in some common cases
    if not root:
        if len(srcs) == 1:
            root = srcs[0]
        else:
            for src in srcs:
                if src == name or src == name + ".jinja2":
                    root = src
        if not root:
            fail("could not infer 'root' template, please set `root` kwarg", attr = "root")
    if not deps:
        deps = []

    compiled_srcs = {}

    for src in srcs:
        raw_src = "{}__{}".format(name, src)
        export_file(
            name = raw_src,
            src = src,
        )
        compiled_src = "{}__{}.py".format(name, src)
        python_file_name = "tmpl_{}".format(paths.replace_extension(src, ".py"))

        buck_genrule(
            name = compiled_src,
            cmd = "$(exe //antlir:compile-template) $(location :{}) {} > $OUT".format(raw_src, src),
            out = python_file_name,
        )
        compiled_srcs[":" + compiled_src] = python_file_name

    # The named output target of template() is a python_library so that it can
    # be easily used in `deps` of other templates.
    python_library(
        name = name,
        srcs = compiled_srcs,
        base_module = "antlir.__compiled_templates__",
        deps = deps,
        visibility = get_visibility(visibility),
    )

    # Compile the name of the root template into the python_binary for
    # rendering, so that it can be easily imported. It cannot be included in
    # the python_library target above, or there would be collisions with any
    # templates included in `deps`.
    root_template_target = name + "__root_template_name"
    buck_genrule(
        name = root_template_target,
        cmd = "printf {} > $OUT".format(shell.quote(paths.replace_extension(root, ""))),
        visibility = [],
    )

    python_binary(
        name = name + "-render",
        main_module = "antlir.render_template",
        deps = [
            ":" + name,
            "//antlir:render_template",
        ],
        base_module = "antlir",
        resources = {
            ":" + root_template_target: "__root_template_name__",
        },
        visibility = get_visibility(visibility),
    )
