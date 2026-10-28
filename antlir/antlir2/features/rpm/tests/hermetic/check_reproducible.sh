#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Two clean, uncached builds of one layer (default :layer-a, CentOS 10; pass
# layer-a-centos9 for CentOS 9) with hermetic RPM installs; its RPM
# database and dnf history must be byte-identical. Keep the isolation-dir
# name fixed: the transaction can see paths in the Buck output directory.
set -euo pipefail
iso=${ISO:-hermetic-rpm-check}
target=fbcode//antlir/antlir2/features/rpm/tests/hermetic:${1:-layer-a}
out=$(mktemp -d)
# The commit time of the revision being built
epoch=$(sl log -r . -T '{date|hgdate}' | cut -d' ' -f1)
for run in a b; do
    buck2 --isolation-dir "$iso" clean
    layer=$(buck2 --isolation-dir "$iso" build --no-remote-cache --modifier 'antlir//antlir/antlir2/cfg:hermetic_rpm[enabled]' \
        -c antlir2.source_date_epoch="$epoch" \
        --show-full-output "$target" | awk '{print $2}')
    mkdir -p "$out/$run"
    for f in usr/lib/sysimage/rpm var/lib/rpm var/lib/dnf; do
        [ -d "$layer/$f" ] && cp -r "$layer/$f" "$out/$run/${f//\//_}"
    done
done
diff -r "$out/a" "$out/b"
echo "identical: $(cd "$out/a" && find . -type f | sort | tr '\n' ' ')"
