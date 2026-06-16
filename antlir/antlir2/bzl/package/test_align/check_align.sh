#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -euo pipefail

path="$1"
size=$(stat -c%s "$path")
if (( size % 2097152 != 0 )); then
    echo "size $size not aligned to 2097152" >&2
    exit 1
fi
echo "ok aligned size $size"
