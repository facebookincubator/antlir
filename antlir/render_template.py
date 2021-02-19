#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import sys

from jinja2 import Environment, PackageLoader


def main():
    env = Environment(
        loader=PackageLoader(__package__, "templates"),
        trim_blocks=True,
        lstrip_blocks=True,
    )
    data = json.load(sys.stdin)

    template = env.get_template("main.jinja2")
    print(template.render(**data), end="")


if __name__ == "__main__":
    main()
