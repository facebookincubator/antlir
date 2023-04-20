#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

if rpm -q foo; then
    echo "foo should have been removed"
    exit 2
fi

if rpm -q foobar; then
    echo "foobar should have been removed"
    exit 2
fi

if rpm -q foobarbaz; then
    echo "foobarbaz should have been removed when we removed foobar"
    exit 2
fi
