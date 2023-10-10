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
    echo "Env var ANTLIR2_TEST is wrong or missing!"
    exit 2
fi

if [ -z "${ENV_ARTIFACT}" ]; then
    echo "Env var ENV_ARTIFACT is missing!"
    exit 3
elif ! [ -f "${ENV_ARTIFACT}" ]; then
    echo "Env var ENV_ARTIFACT does not point to a valid target!"
    exit 3
fi
