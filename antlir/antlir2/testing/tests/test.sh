#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

if [ "$(id -nu)" != "$TEST_USER" ]; then
    echo "expected to run as $TEST_USER but am $(id -nu)"
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
    # systemctl is-system-running may still report 'activating' if other
    # services have started but not yet activated, but we really only care
    # about waiting on default.target (and everything it brings in directly)
    if ! systemctl is-active default.target; then
        # Run these commands to dump information that might be useful to debug a
        # test failure
        systemctl list-jobs
        systemctl --failed
        journalctl
        exit 1
    fi
else
    echo "unrecognized BOOT=${BOOT} - update this test for new behavior"
    exit 100
fi
