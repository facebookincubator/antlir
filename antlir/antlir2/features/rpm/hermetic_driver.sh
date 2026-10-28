#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Runs the dnf driver under Hermit (see hermetic.bzl).
#
# usage: hermetic_driver.sh HERMIT SOURCE_DATE_EPOCH [HERMIT_RUN_ARGS...] -- DRIVER [ARGS...]
#
# driver.py bind mounts /dev, /proc, /sys and /tmp into the install root. They
# are done here instead, before Hermit starts, so the traced process tree never
# calls mount(2); ANTLIR2_HERMETIC_RPM_PREMOUNTED tells driver.py to skip them.
# rpm.rs always isolates the install root at /__antlir2__/root.
set -eu

hermit="$1"
# Tools that follow the reproducible-builds SOURCE_DATE_EPOCH convention (for
# example p11-kit writing the Java cacerts keystore from an RPM scriptlet)
# record it instead of the current time. Hermit's virtual clock starts at the
# same epoch but advances during the transaction, so export the fixed value.
export SOURCE_DATE_EPOCH="$2"
shift 2
hermit_args=""
while [ "$1" != "--" ]; do
    hermit_args="$hermit_args $1"
    shift
done
shift

# Hermit's host-no-rdrand CPU profile needs CPUID faulting (Intel, or AMD with
# Linux 6.17+). Check before touching the install root, and fail with the
# reason instead of falling back to a non-hermetic install.
# Exit 122 is Hermit's policy refusal; anything else means Hermit itself
# could not run (for example a missing shared library), which must not be
# reported as a host limitation.
set +e
qualification=$("$hermit" host-capabilities --cpu-profile host-no-rdrand 2>&1)
status=$?
set -e
if [ "$status" -eq 122 ]; then
    echo "hermetic RPM install refused on this host: $qualification" >&2
    echo "build on a host with CPUID faulting, or drop the hermetic_rpm[enabled] modifier" >&2
    exit 1
elif [ "$status" -ne 0 ]; then
    echo "hermetic RPM install: hermit failed to run (exit $status): $qualification" >&2
    exit 1
fi

root=/__antlir2__/root
for spec in /dev:755 /proc:555 /sys:555 /tmp:1777; do
    src="${spec%%:*}"
    dst="$root$src"
    if [ ! -e "$dst" ]; then
        mkdir -p "$dst"
        chmod "${spec#*:}" "$dst"
    fi
    mount --rbind "$src" "$dst"
done
export ANTLIR2_HERMETIC_RPM_PREMOUNTED="$root"

# The transaction gets exactly this environment, not whatever reached this
# script. Otherwise the Buck daemon's TERM (passed through by antlir2_isolate
# when set), this shell's `_` (the path of the program it runs, which names
# the Buck output directory) or caller additions would reach dnf/scriptlets
# through Hermit, and builds on different hosts would differ. PATH is
# antlir2_isolate's default; the rest are what rpm.rs sets on purpose (keep in
# sync with its setenv calls) plus the two variables above.
# antlir2_isolate already gives the transaction its own mount and pid
# namespaces, so Hermit does not create its own.
# container and USER come from antlir2_isolate, the others from rpm.rs.
# shellcheck disable=SC2086,SC2154
exec /usr/bin/env -i \
    PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    LANG=C.UTF-8 \
    TERM=dumb \
    container="$container" \
    USER="$USER" \
    TMPDIR="$TMPDIR" \
    PYTHONDONTWRITEBYTECODE="$PYTHONDONTWRITEBYTECODE" \
    SYSTEMD_TMPFILES_FORCE_SUBVOL="$SYSTEMD_TMPFILES_FORCE_SUBVOL" \
    SYSTEMD_OFFLINE="$SYSTEMD_OFFLINE" \
    SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" \
    ANTLIR2_HERMETIC_RPM_PREMOUNTED="$ANTLIR2_HERMETIC_RPM_PREMOUNTED" \
    "$hermit" run --no-namespace $hermit_args -- "$@"
