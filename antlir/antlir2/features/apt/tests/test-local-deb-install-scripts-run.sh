#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

# The maintainer scripts were packaged and registered with dpkg
for script in preinst postinst prerm postrm; do
    grep -qF "/antlir2-$script-ran" "/var/lib/dpkg/info/antlir2-scripts.$script"
done

# Each script records its arguments in the image root
test "$(cat /antlir2-preinst-ran)" = "install"
test "$(cat /antlir2-postinst-ran)" = "configure"

test ! -e /antlir2-prerm-ran
test ! -e /antlir2-postrm-ran
