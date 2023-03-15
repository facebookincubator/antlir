#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

echo "Testing installed directory ownership"

find /installed-dir -print0 | while IFS= read -r -d '' d
do
    owner=$(stat --format '%U:%G' "$d")
    if [ "$owner" != "root:root" ]; then
        echo "Unexpected ownership of $d: $owner"
        exit 1
    fi
done
