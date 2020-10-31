#!/bin/bash -ue
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -o pipefail

# $BUCK_DEFAULT_RUNTIME_RESOURCES is populated with symlinks to (or copies of)
# the `resources` specified in the target for this buck_sh_binary. Since it's
# determined at runtime it cannot be shellcheck-ed.
# shellcheck source=/dev/null
source "$BUCK_DEFAULT_RUNTIME_RESOURCES/rpm_sign_test_functions.sh"

sign_with_test_key "$1"
