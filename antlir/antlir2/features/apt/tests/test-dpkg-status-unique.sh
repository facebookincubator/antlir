#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# A child layer's apt transaction must not leave a second status entry for a
# package the parent already installed: dpkg rejects the whole database with
# "multiple non-coinstallable package instances present".
set -e

dups="$(grep '^Package: ' /var/lib/dpkg/status | sort | uniq -d)"
if [ -n "$dups" ]; then
    echo "FAIL: duplicate dpkg status entries:"
    echo "$dups"
    exit 1
fi

dpkg --audit
dpkg-query -W base-files
