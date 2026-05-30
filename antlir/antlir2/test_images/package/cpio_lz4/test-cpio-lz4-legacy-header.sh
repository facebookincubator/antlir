#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -eu

actual="$(head -c 4 "$1" | od -An -tx1 | tr -d ' \n')"
expected="02214c18"

if [ "$actual" != "$expected" ]; then
    echo "expected lz4 legacy magic $expected, got $actual" >&2
    exit 1
fi
