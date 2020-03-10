#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys

from btrfs_diff.tests.demo_sendstreams import gold_demo_sendstreams

# This could be made to run against `make_demo_sendstreams`, which would
# (redundantly with `btrfs_diff` tests) build a send-stream from scratch
# instead of using a committed one.  However, this would be fidgety since
# I'd have to pass a repo path here, which is hard in @mode/opt -- I'd need
# to pass it in from a unit-test PAR (which gets run in-tree).  On the other
# hand, the coverage benefit of doing the work is not that great, since we
# will receive & send this stream with live btrfs, anyway.
with open(sys.argv[1], 'wb') as outfile:
    outfile.write(gold_demo_sendstreams()[sys.argv[2]]['sendstream'])
