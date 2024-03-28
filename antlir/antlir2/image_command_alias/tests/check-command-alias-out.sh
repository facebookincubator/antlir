#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ue -o pipefail
data=$(cat "$1")
if [[ "$data" != "hello world goodbye world" ]]; then
    echo "Unexpected data in $1" >&2
    exit 1
fi
