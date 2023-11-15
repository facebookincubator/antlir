#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

# This binary should work. If we cross compiled though, LSAN will pitch a fit,
# so we really just need to make sure that the binary ran, not that it exited 0
test-binary-extracted | grep "Hello world!"

# It should keep working even if we unmount the deps provided by the test
# environment
umount /usr/local/fbcode
umount /mnt/gvfs
test-binary-extracted | grep "Hello world!"
