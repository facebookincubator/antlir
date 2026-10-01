#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

# /usr/bin/sudo is mode 4111 (setuid, execute-only), so extracting it
# exercises reading files without read permission in unprivileged builds.
sudo --version
test -x /usr/bin/sudo
test -u /usr/bin/sudo
