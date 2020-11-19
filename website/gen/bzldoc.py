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
import os
from dataclasses import dataclass
from typing import Iterable, Mapping, Optional

from antlir.artifacts_dir import find_buck_cell_root
from antlir.common import get_logger
from antlir.fs_utils import Path


log = get_logger()


# Global mapping to track all the parse modules to resolve references between
# files, since antlir apis heavily employ redirection in API exports.
all_modules: Mapping[Path, "BzlFile"] = {}


@dataclass(frozen=True)
class BzlFile(object):
    path: Path
    module: ast.Module

    @property
    def name(self) -> str:
        return self.path.basename().decode()

    @property
    def docblock(self) -> Optional[str]:
        return ast.get_docstring(self.module)

    @property
    def body(self) -> Iterable[ast.AST]:
        return self.module.body

    @property
    def export_struct(self) -> Optional[Mapping[str, ast.AST]]:
        """Look for a struct that exports the 'public api' of this module"""
        assignments = [
            node for node in self.body if isinstance(node, ast.Assign)
        ]
        # typically this is at the end of the module, so iterate backwards
        for e in reversed(assignments):
            # For easy matching, it is assumed that the name of the struct
            # matches the module name
            if len(e.targets) == 1 and e.targets[0].id == self.name:
                return {kw.arg: kw.value for kw in e.value.keywords}
        return None

    @property
    def functions(self) -> Mapping[str, ast.FunctionDef]:
        return {
            node.name: node
            for node in self.body
            if isinstance(node, ast.FunctionDef)
        }

    @property
    def loaded_symbols(self) -> Mapping[str, str]:
        """Returns map of symbol -> source file target"""
        loads = [
            node.value
            for node in self.body
            if isinstance(node, ast.Expr)
            and isinstance(node.value, ast.Call)
            and isinstance(node.value.func, ast.Name)
            and node.value.func.id == "load"
        ]
        symbols = {}
        for load in loads:
            file = load.args[0].s.lstrip("/").encode()
            if file.startswith(b":"):
                file = self.path.dirname() / file.lstrip(b":")
            file = Path(file.replace(b":", b"/")[:-4])

            file_symbols = [a.s for a in load.args[1:]]
            for s in file_symbols:
                symbols[s] = file
        return symbols

    def resolve_function(self, name: str) -> Optional[ast.FunctionDef]:
        """
        Attempt to resolve the given function name, traversing load()
        calls if it is not defined locally.
        """
        f = self.functions.get(name, None)
        if f:
            return f
        src = self.loaded_symbols.get(name, None)
        if src:
            if src not in all_modules:
                log.warning(
                    f"{name} is loaded from {src}, which was not parsed"
                )
                return None
            return all_modules[src].resolve_function(name)
        log.warning(f"{self.path}: '{name}' not defined locally or loaded")
        return None

    @property
    def header(self) -> str:
        return (
            f"""---
id: {self.path.basename().decode()}
title: {self.path.basename().decode().capitalize()}
generated: """
            + "'@"
            + "generated'"
            + "\n---\n"
        )

    def generate_md(self) -> Optional[str]:
        """
        Generate a .md doc describing the exported API of this module, or
        None if there is no export struct.
        This MUST be called after parsing every module, since it does
        cross-module docstring resolution.
        """
        if not self.export_struct:
            log.warning(f"{self.path}: missing export struct, not documenting")
            return None

        md = self.header
        md += self.docblock or ""
        md += "\n\n"
        md += "API\n===\n"
        for name, node in self.export_struct.items():
            if not isinstance(node, ast.Name):
                log.warning(f"not documenting non-name '{name}: {node}'")
                continue
            func = self.resolve_function(node.id)
            if not func:
                log.warning(f"not documenting unresolved func '{name}'")
                continue

            args = [a.arg for a in func.args.args]
            if func.args.vararg:
                args.append("*" + func.args.vararg.arg)
            if func.args.kwarg:
                args.append("**" + func.args.kwarg.arg)
            args = ", ".join(args)

            md += f"`{name}`\n---\n"
            md += f"`{name}({args})`\n"
            md += ast.get_docstring(func) or "No docstring available.\n"
            md += "\n\n"

        return md


def bzldoc():
    parser = argparse.ArgumentParser()
    parser.add_argument("bzls", type=Path.from_argparse, nargs="+")
    parser.add_argument("outdir", type=Path.from_argparse)

    args = parser.parse_args()

    bzls = args.bzls
    outdir = args.outdir
    assert outdir.exists()

    repo_root = find_buck_cell_root()

    for bzl in bzls:
        # always deal with relative paths from repo root
        parsed = ast.parse(bzl.read_text())
        bzl = bzl.abspath().relpath(repo_root)
        assert bzl.endswith(b".bzl")
        module_path = Path(bzl[:-4])
        module = BzlFile(module_path, parsed)
        all_modules[module_path] = module

    for mod in all_modules.values():
        md = mod.generate_md()
        if not md:
            continue
        dstdir = outdir / mod.path.relpath("antlir/bzl").dirname()
        dst = dstdir / f"gen-{mod.path.basename()}.md"
        if not dstdir.exists():
            os.makedirs(dstdir, exist_ok=True)

        # avoid rewriting the file if the contents are the same to avoid
        # endlessly recompiling in `yarn watch`
        if dst.exists() and dst.read_text() == md:
            log.debug(f"{dst} is unchanged")
        else:
            log.info(f"updating generated docs {dst}")
            with open(dst, "w") as out:
                out.write(md)


if __name__ == "__main__":
    bzldoc()
