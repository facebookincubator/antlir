#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

if [ "$UID" != "0" ]; then
    echo "Not root!"
    exit 1
fi

if [ "${ANTLIR2_TEST}" != "1" ]; then
    echo "Env var missing!"
    exit 1
fi

if [ "${BOOT}" != "False" ]; then
    # Easy way to check that the systemd manager is running as pid1
    systemctl is-active "sysinit.target"
fi
