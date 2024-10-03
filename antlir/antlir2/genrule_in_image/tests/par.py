# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys


def main():
    out = sys.argv[1]
    with open(out, "w") as f:
        f.write("From par\n")
