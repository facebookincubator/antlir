#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os.path
import sys

length = int(sys.argv[1])

for idx in range(length):
    assert os.path.exists(str(idx)), f"file {idx} did not exist"
