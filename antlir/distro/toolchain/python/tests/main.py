# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import sys


def main():
    json.dump(
        {"python_interpreter": sys.executable}, sys.stdout, indent=2, sort_keys=True
    )
    print()
