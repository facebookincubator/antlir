#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

if [ "$UID" != "0" ]; then
    echo "Not root!"
    exit 1
fi

if [ "${ANTLIR2_TEST}" != "1" ]; then
    echo "Env var missing!"
    exit 2
fi

if [ "${BOOT}" == "False" ]; then
    if systemctl is-active "sysinit.target"; then
        echo "systemd should not be booted"
        exit 3
    fi
elif [ "${BOOT}" == "True" ]; then
    # Easy way to check that the systemd manager is running as pid1
    systemctl is-active "sysinit.target"
elif [ "${BOOT}" == "wait-default" ]; then
    # This should report true by now because the test waits on default.target
    if ! systemctl is-system-running; then
        systemctl --failed
        journalctl
        exit 1
    fi
else
    echo "unrecognized BOOT=${BOOT} - update this test for new behavior"
    exit 100
fi
