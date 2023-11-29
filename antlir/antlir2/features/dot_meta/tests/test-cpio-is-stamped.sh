#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e
target="$(zcat "$1" | cpio -i --to-stdout .meta/target)"
if [ "$target" != "fbcode//antlir/antlir2/features/dot_meta/tests:stamped.cpio.gz" ]; then
    echo "bad target: $target"
    exit 1
fi
