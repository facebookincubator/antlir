#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test ! -e /usr/share/doc/antlir2-docs/README
test "$(cat /usr/share/doc/antlir2-docs/copyright)" = "copyright"
test ! -e /usr/share/man/man1/antlir2-docs.1

# excluded paths are still the package's
dpkg-query -L antlir2-docs > /tmp/antlir2-docs.list
grep -qxF /usr/share/doc/antlir2-docs/README /tmp/antlir2-docs.list
grep -qxF /usr/share/man/man1/antlir2-docs.1 /tmp/antlir2-docs.list
