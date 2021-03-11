# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:oss_shim.bzl", "get_visibility", "python_binary")

def template(
        name,
        main,
        includes = None,
        visibility = None):
    """
Create a template target from jinaj2 template files.
The named target is a python_binary that renders templates from JSON data
provided on stdin.
The `main` template is rendered with the input data, and has access to inherit
from / include any of the templates listed in `includes`.
"""
    if not includes:
        includes = {}
    if types.is_list(includes):
        includes = {inc: paths.basename(inc) for inc in includes}

    resources = {
        main: "main.jinja2",
    }
    resources.update(includes)

    python_binary(
        name = name,
        main_module = "antlir.render_template",
        deps = ["//antlir:render_template"],
        base_module = "antlir.templates",
        resources = resources,
        visibility = get_visibility(visibility, name),
    )
