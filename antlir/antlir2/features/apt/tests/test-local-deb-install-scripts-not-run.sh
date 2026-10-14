#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

# The maintainer scripts were packaged and registered with dpkg
for script in preinst postinst prerm postrm; do
    grep -qF "touch /antlir2-$script-ran" "/var/lib/dpkg/info/antlir2-scripts.$script"
done

# Maintainer scripts are not executed yet. Once they are, this should assert
# that the preinst and postinst side effects exist.
for script in preinst postinst prerm postrm; do
    test ! -e "/antlir2-$script-ran"
done
