#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

# Verify bash is not installed according to dpkg
# dpkg-query exits non-zero if the package is not known
! dpkg-query -W bash 2>/dev/null

# Verify the binary is gone
! test -x /usr/bin/bash
