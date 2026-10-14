#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test -d /antlir2-files/dir
test -d /antlir2-files/empty-dir
test "$(cat /antlir2-files/dir/file)" = "hello"
test -x /antlir2-files/dir/executable
test -L /antlir2-files/symlink
test "$(readlink -f /antlir2-files/symlink)" = "/antlir2-files/dir/file"

dpkg-query -L antlir2-files > /tmp/antlir2-files.list
for path in \
    /antlir2-files/dir \
    /antlir2-files/empty-dir \
    /antlir2-files/dir/file \
    /antlir2-files/dir/executable \
    /antlir2-files/symlink; do
    grep -qxF "$path" /tmp/antlir2-files.list
done
