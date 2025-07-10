# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import json
import sys

import rpm

parser = argparse.ArgumentParser()
parser.add_argument("--installroot", required=True)
args = parser.parse_args()


def _info_from_hdr(hdr):
    return {
        "name": hdr[rpm.RPMTAG_NAME],
        "epoch": hdr[rpm.RPMTAG_EPOCH] or 0,
        "version": hdr[rpm.RPMTAG_VERSION],
        "release": hdr[rpm.RPMTAG_RELEASE],
        "arch": hdr[rpm.RPMTAG_ARCH] or "noarch",
        "changelog": "\n".join(hdr[rpm.RPMTAG_CHANGELOGTEXT])
        if hdr[rpm.RPMTAG_CHANGELOGTEXT]
        else None,
        "size": hdr[rpm.RPMTAG_SIZE],
        "os": hdr[rpm.RPMTAG_OS],
        "source_rpm": hdr[rpm.RPMTAG_SOURCERPM],
    }


ts = rpm.TransactionSet(args.installroot)

output = [_info_from_hdr(hdr) for hdr in ts.dbMatch()]


json.dump(output, sys.stdout)
