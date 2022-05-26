#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib
import importlib.resources
import json
import sys

from jinja2 import BaseLoader, Environment, TemplateNotFound


class PrecompiledLoader(BaseLoader):
    has_source_access = False

    def __init__(self, base: str) -> None:
        self.base = base

    @staticmethod
    def get_template_key(name) -> str:
        if name.endswith(".jinja2"):
            name = name[: -len(".jinja2")]
        return "tmpl_" + name

    def load(self, environment, name, globals=None):
        key = self.get_template_key(name)
        try:
            mod = importlib.import_module(self.base + "." + key)
        except ImportError:
            raise TemplateNotFound(name)

        return environment.template_class.from_module_dict(
            environment, mod.__dict__, globals
        )


def main() -> None:
    env = Environment(
        loader=PrecompiledLoader("antlir.__compiled_templates__"),
        trim_blocks=True,
        lstrip_blocks=True,
    )
    data = json.load(sys.stdin)

    root = importlib.resources.read_text("antlir", "__root_template_name__")

    template = env.get_template(root)
    print(template.render(**data), end="")


if __name__ == "__main__":
    main()
