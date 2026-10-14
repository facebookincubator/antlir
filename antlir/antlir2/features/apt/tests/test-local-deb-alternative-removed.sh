#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test ! -e /usr/bin/antlir2-greeter-hello

# prerm removed the only provider, so the alternative is gone entirely
for link in \
    /usr/bin/antlir2-greeter \
    /usr/bin/antlir2-greet \
    /etc/alternatives/antlir2-greeter \
    /etc/alternatives/antlir2-greet \
    /var/lib/dpkg/alternatives/antlir2-greeter; do
    test ! -e "$link" && test ! -L "$link"
done
