#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys

# print only the first arg passed to the binary to test for proper
# quote handling in wrap_executable_target
print(sys.argv[1])
