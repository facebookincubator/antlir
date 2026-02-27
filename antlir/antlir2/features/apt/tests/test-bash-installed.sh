#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

# Verify bash is installed according to dpkg
dpkg -s bash
# dpkg-query exits 0 only if the package is installed
dpkg-query -W bash

# Verify the binary is present and executable
test -x /usr/bin/bash
/usr/bin/bash --version
