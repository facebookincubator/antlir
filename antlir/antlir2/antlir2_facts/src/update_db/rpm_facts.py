# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import json
import sys

import dnf
import rpm

parser = argparse.ArgumentParser()
parser.add_argument("--installroot", required=True)
args = parser.parse_args()

output = []

conf = dnf.conf.Conf()
conf.installroot = args.installroot
base = dnf.Base(conf)
sack = dnf.sack.rpmdb_sack(base)


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
        "pkgid": hdr[rpm.RPMTAG_PKGID].hex() if hdr[rpm.RPMTAG_PKGID] else None,
    }


for pkg in sack.query().installed():
    hdr = pkg.get_header()
    info = _info_from_hdr(hdr)
    info.update(
        {
            "from_repo": pkg.from_repo.replace("__", "\xff")
            .replace("_", "/")
            .replace("\xff", "_")
            # I hate this, but I can't figure out a cleaner way right now
            .replace("x86/64", "x86_64"),
        }
    )
    output.append(info)

# the dnf query does not show gpg-pubkey "packages", so query rpm for that
ts = rpm.TransactionSet(args.installroot)
for hdr in ts.dbMatch("name", "gpg-pubkey"):
    output.append(_info_from_hdr(hdr))


json.dump(output, sys.stdout)
