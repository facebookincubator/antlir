#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

systemctl --version

# Simple exit status is not enough because on cross-compiled dev builds, LSAN
# pitches a fit
test-binary-installed | grep "Hello world!"
