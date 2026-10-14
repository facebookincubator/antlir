#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test ! -e /var/lib/dpkg/info/antlir2-scripts.prerm
test ! -e /var/lib/dpkg/info/antlir2-scripts.postrm

# Each script records its arguments in the image root
test "$(cat /antlir2-prerm-ran)" = "remove"
test "$(cat /antlir2-postrm-ran)" = "remove"
