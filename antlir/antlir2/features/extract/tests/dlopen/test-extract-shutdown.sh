#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

echo "=== checking systemd-shutdown exists ==="
test -f /usr/lib/systemd/systemd-shutdown
echo "shutdown binary present"

echo "=== checking direct deps via DT_NEEDED are extracted ==="
# libsystemd-shared should be extracted as direct DT_NEEDED
ls /usr/lib64/systemd/libsystemd-shared* 2>&1 | head
test -f /usr/lib64/libc.so.6 || test -f /usr/lib64/libc.so.6  # at least some lib
echo "direct deps present"

echo "=== checking .note.dlopen transitive deps are extracted ==="
# libsystemd-shared has .note.dlopen that declares many optional libs
# that are NOT in its DT_NEEDED and NOT in systemd-shutdown's DT_NEEDED.
# Examples from objdump of libsystemd-shared: libacl.so.1, libblkid.so.1,
# libseccomp.so.2, libselinux.so.1, etc. Those should be extracted when
# .note.dlopen handling is working.

missing=0
for lib in libacl.so.1 libblkid.so.1 libseccomp.so.2 libselinux.so.1 libcap.so.2; do
  if test -f /usr/lib64/$lib || test -f /lib64/$lib; then
    echo "found dlopen dep $lib"
  else
    echo "MISSING dlopen dep $lib (expected via .note.dlopen)"
    missing=$((missing+1))
  fi
done

if [ $missing -ne 0 ]; then
  echo "FAILED: $missing dlopen deps missing, .note.dlopen handling not working"
  echo "Listing /usr/lib64:"
  ls /usr/lib64/libacl* /usr/lib64/libblkid* /usr/lib64/libseccomp* /usr/lib64/libselinux* 2>&1 || true
  exit 1
fi

echo "PASS: all dlopen deps extracted"
