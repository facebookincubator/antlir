#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Two erofs images built from the same layer with fixed_metadata = True must be
# byte-identical. Without it, mkfs.erofs picks a random filesystem UUID and
# stamps the current time into the superblock, so every build produces
# different bytes -- and a different content hash for anything that stores
# these images by digest.
set -uo pipefail

if cmp "$1" "$2"; then
    echo "erofs images are byte-identical"
    exit 0
fi
echo "erofs images differ: fixed_metadata did not produce a reproducible image" >&2
exit 1
