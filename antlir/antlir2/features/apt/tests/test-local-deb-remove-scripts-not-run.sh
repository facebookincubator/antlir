#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test ! -e /var/lib/dpkg/info/antlir2-scripts.prerm
test ! -e /var/lib/dpkg/info/antlir2-scripts.postrm

# Maintainer scripts are not executed yet. Once they are, this should assert
# that the prerm and postrm side effects exist.
for script in preinst postinst prerm postrm; do
    test ! -e "/antlir2-$script-ran"
done
