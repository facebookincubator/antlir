#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

# dpkg's triggers database exists, as after any dpkg run: libc-bin's interest
# in the ldconfig trigger is recorded, so dpkg-trigger records activations
grep -qx libc-bin /var/lib/dpkg/triggers/ldconfig
test -e /var/lib/dpkg/triggers/Unincorp

# antlir2-trigger's postinst activated the ldconfig trigger, which, as at the
# end of a dpkg run, has been processed: nothing is left unincorporated or
# pending
test ! -s /var/lib/dpkg/triggers/Unincorp
if grep -E '^Triggers-(Pending|Awaited):' /var/lib/dpkg/status; then
    exit 1
fi
test "$(dpkg-query -W -f '${db:Status-Status}' libc-bin)" = installed
