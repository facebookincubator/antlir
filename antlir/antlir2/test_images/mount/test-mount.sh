#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

echo "Checking /mount/in/parent"
mountpoint /mount/in/parent
test -f /mount/in/parent/empty

# Nested layer mounts
echo "Checking /mount/in/child/mount/in/parent"
mountpoint /mount/in/child/mount/in/parent
test -f /mount/in/child/mount/in/parent/empty
