#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

if rpm -q rpm-test-cheese ; then
    echo "rpm-test-cheese should have been removed"
    exit 2
fi

if rpm -q dracut-squash ; then
    echo "dracut-squash should have been removed when we removed dracut"
    exit 2
fi
