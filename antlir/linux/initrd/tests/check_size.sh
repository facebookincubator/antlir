#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e
size="$(stat --printf="%s" "$1")"

# Fail if the compressed package size is >9M
# This is important for PXE booting and we should aim to keep this as small as
# possible
if [ "$size" -gt 9000000 ]; then
    echo "cpio archive is larger than 9M"
    echo "$1 is $size bytes"
    exit 1
fi
if [ "$size" -lt 8000000 ]; then
    echo "cpio archive is smaller than 8M"
    echo "Congrats! Now update this test so that we can keep it small :)"
    echo "$1 is $size bytes"
    exit 1
fi
