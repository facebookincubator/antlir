#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

echo "this is some stdout of a test that is expected to fail"
echo "this is some stderr of a test that is expected to fail" > /dev/stderr
exit 1
