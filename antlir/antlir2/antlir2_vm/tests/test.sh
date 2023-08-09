#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

if [ "$UID" != "0" ]; then
    echo "Not root!"
    exit 1
fi

if [ "${ANTLIR2_TEST}" != "1" ]; then
    echo "Env var ANTLIR2_TEST is wrong or missing!"
    exit 2
fi
