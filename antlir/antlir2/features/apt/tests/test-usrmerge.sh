#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Installing a .deb that ships legacy (non-usrmerged) paths must not revert
# the merged-/usr alias symlinks created by base-files: the files have to
# land through the symlinks, the way dpkg installs them.
set -e

test -L /bin || { echo "FAIL: /bin is not a symlink"; exit 1; }
test -L /sbin || { echo "FAIL: /sbin is not a symlink"; exit 1; }
test -L /lib || { echo "FAIL: /lib is not a symlink"; exit 1; }

test "$(readlink /bin)" = "usr/bin"
test "$(readlink /sbin)" = "usr/sbin"
test "$(readlink /lib)" = "usr/lib"

# iproute2 ships /sbin/* and /bin/* entries; they must resolve here.
test -x /usr/sbin/ip
test -x /usr/bin/ip
