#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

# Ensure that this test rpm is installed
rpm -q rpm-test-cheese-3-1
# THis older version of it should not be installed
if rpm -q rpm-test-cheese-2-1 ; then
    echo "rpm-test-cheese-2-1 should not have been installed"
    exit 2
fi
