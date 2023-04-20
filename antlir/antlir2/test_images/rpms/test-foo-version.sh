#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

EVRA="$1"

# Ensure that this test rpm is installed
if ! rpm -q foo-"$EVRA"; then
    echo "checking if any other version is installed:"
    rpm -q foo
    exit 1
fi
