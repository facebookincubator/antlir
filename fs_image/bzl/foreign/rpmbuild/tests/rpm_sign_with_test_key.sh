#!/bin/bash -ue
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -o pipefail

# Since we're using a test key, create a temporary directory to house the gpg
# configuration and trust data so as not to pollute the user's host data.
GNUPGHOME=$(mktemp -d)
export GNUPGHOME

trap 'rm -rf "$GNUPGHOME"' EXIT

signing_key="$BUCK_PROJECT_ROOT/fs_image/rpm/tests/gpg_test_keypair/private.key"
gpg -q --import "$signing_key"
rpmsign --addsign --define='_gpg_name Test Key' --define='_gpg_digest_algo sha256' "$1"
