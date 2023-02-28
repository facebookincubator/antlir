#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

if /antlir2-simply-installed ; then
    echo "This should not work!"
fi

# This binary should work
/usr/bin/antlir2 --help

# It should keep working even if we unmount the deps provided by the test
# environment
umount /usr/local/fbcode
umount /mnt/gvfs
/usr/bin/antlir2 --help
