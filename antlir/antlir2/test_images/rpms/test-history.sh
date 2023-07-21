#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

# 'foo' should only be touched in transaction 1
if [ "$(dnf history list foo | tail -n +3 | awk '{ print $1 }')" != "1" ]; then
    echo "foo was altered outside of transaction 1!"
    dnf history list foo
    exit 1
fi

tx1comment="$(dnf history info 1)"
if [[ "$tx1comment" != *"//antlir/antlir2/test_images/rpms:simple--layer"* ]]; then
    echo "comment did not have label"
    echo "$tx1comment"
    exit 1
fi

# 'foobar' should have been touched in transaction 1 and 2 because the reason
# needed to be changed
if [ "$(dnf history list foobar | tail -n +3 | awk '{ print $1 }')" != $'2\n1' ]; then
    echo "foobar should have been changed in tx 1 and 2"
    dnf history list foobar
    exit 1
fi
