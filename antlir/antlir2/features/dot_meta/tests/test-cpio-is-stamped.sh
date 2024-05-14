#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

target="$(zcat "$1" | cpio -i --to-stdout .meta/target)"
expected="$2"
if [ "$target" != "$expected" ]; then
    echo "bad target: $target"
    echo "expected: $expected"
    exit 1
fi
