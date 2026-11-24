#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test "$(cat /antlir2-diverted)" = "diverter"
test "$(cat /antlir2-diverted.distrib)" = "owner"
test "$(dpkg-divert --truename /antlir2-diverted)" = "/antlir2-diverted.distrib"

# both list the path they ship, not where it went
dpkg-query -L antlir2-diverted-owner | grep -qxF /antlir2-diverted
dpkg-query -L antlir2-diverter | grep -qxF /antlir2-diverted

# a hard link to the diverted file links to where the owner's file went
test "$(cat /antlir2-diverted-link)" = "owner"
test "$(stat -c %i /antlir2-diverted-link)" = "$(stat -c %i /antlir2-diverted.distrib)"

# the conffile's md5sum is of the owner's file, at its diversion
test "$(cat /etc/antlir2-diverted.conf)" = "diverter"
test "$(dpkg-query -W -f '${Conffiles}' antlir2-diverted-owner | awk '{print $2}')" = \
    "$(md5sum < /etc/antlir2-diverted.conf.distrib | cut -d' ' -f1)"
