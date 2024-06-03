#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import importlib
import importlib.resources
import json
from pathlib import Path

from jinja2 import BaseLoader, Environment, TemplateNotFound


class PrecompiledLoader(BaseLoader):
    has_source_access = False

    def __init__(self, compiled_dir: Path) -> None:
        self.compiled_dir = compiled_dir

    def load(self, environment, name, globals=None):
        path = self.compiled_dir / (name + ".py")
        try:
            spec = importlib.util.spec_from_file_location(name, path)
            if not spec.loader:
                raise TemplateNotFound(name)
            mod = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(mod)
        except ImportError:
            raise TemplateNotFound(name)

        return environment.template_class.from_module_dict(
            environment, mod.__dict__, globals
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root")
    parser.add_argument("--compiled-templates", type=Path)
    parser.add_argument("--json-file", type=argparse.FileType("r"))
    parser.add_argument("--output", type=argparse.FileType("w"))
    args = parser.parse_args()

    env = Environment(
        loader=PrecompiledLoader(args.compiled_templates),
        trim_blocks=True,
        lstrip_blocks=True,
    )
    data = json.load(args.json_file)

    template = env.get_template(args.root)
    print(template.render(**data), end="", file=args.output)


if __name__ == "__main__":
    main()
