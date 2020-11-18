#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
bzldoc.py is a simple documentation extractor that parses docstrings out of
.bzl files and converts them to .md

bzldoc will look for a top-level struct that serves as the public API of the
.bzl file and expose that accordingly. If not present, "public" functions
(without a leading _) are documented in the output file.

There is currently no additional parsing done on the docstrings themselves
(for example, to highlight function arguments).
"""
import argparse
import ast

from antlir.common import get_logger
from antlir.fs_utils import Path


log = get_logger()


def generate_md(module: ast.Module, export_id: str):
    functions = [
        node for node in module.body if isinstance(node, ast.FunctionDef)
    ]
    functions = {fdef.name: fdef for fdef in functions}

    # Look for a public top-level struct definition that is the exported
    # API of this file.
    assignments = [node for node in module.body if isinstance(node, ast.Assign)]
    api_struct = None
    for e in reversed(assignments):
        # For easy matching, it is assumed that the name of the struct
        # matches the module name
        if len(e.targets) == 1 and e.targets[0].id == export_id:
            api_struct = {kw.arg: kw.value for kw in e.value.keywords}

    assert api_struct, f"expected a struct named '{export_id}'"

    api_functions = {}
    for key, val in api_struct.items():
        if not isinstance(val, ast.Name):
            log.warning(f"Not documenting non-name '{key}: {val}'")
            continue
        if val.id not in functions:
            log.warning(
                f"Not documenting non-locally defined function '{val.id}'"
            )
            continue
        api_functions[key] = functions[val.id]

    md = ast.get_docstring(module)
    md += "\n\n"
    md += "API\n===\n"
    for name, func in api_functions.items():
        args = [a.arg for a in func.args.args]
        if func.args.vararg:
            args.append("*" + func.args.vararg.arg)
        if func.args.kwarg:
            args.append("**" + func.args.kwarg.arg)
        args = ", ".join(args)
        md += f"`{name}({args})`\n---\n"
        md += ast.get_docstring(func) or "No docstring available.\n"
        md += "\n\n"
    return md


def bzldoc():
    parser = argparse.ArgumentParser()
    parser.add_argument("bzldir", type=Path.from_argparse)
    parser.add_argument("outdir", type=Path.from_argparse)

    args = parser.parse_args()

    bzldir = args.bzldir
    outdir = args.outdir
    assert bzldir.exists()
    assert outdir.exists()

    for bzl in bzldir.listdir():
        if not bzl.endswith(b".bzl"):
            continue

        bzl = bzldir / bzl

        bzlid = bzl.basename().replace(b".bzl", b"").decode()
        title = bzlid.capitalize()

        module = ast.parse(bzl.read_text())

        with open(outdir / bzlid + b".md", "w") as md:
            md.write("---\n")
            md.write(f"id: {bzlid}\n")
            md.write(f"title: {title}\n")
            md.write('generated: "@')
            md.write('generated"\n')
            md.write("---\n")

            md.write(generate_md(module, bzlid))


if __name__ == "__main__":
    bzldoc()
