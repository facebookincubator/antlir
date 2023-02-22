#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# The dnf shell interface kind of sucks for how we really want to drive it as
# part of an image build tool.
# By using the api directly, we can handle errors much more reasonably (instead
# of, to name a totally reasonable operation of dnf, silently ignoring packages
# that don't exist)

# NOTE: this must be run with system python, so cannot be a PAR file

import json
import sys
import threading
from collections import defaultdict

import dnf


class LockedOutput(object):
    def __init__(self, file):
        self._file = file
        self._lock = threading.Lock()

    def __enter__(self):
        self._lock.acquire()
        return self._file

    def __exit__(self, exc_type, exc_value, traceback):
        self._lock.release()


_DL_STATUS_TO_EVENT = {
    dnf.callback.STATUS_OK: "ok",
    dnf.callback.STATUS_ALREADY_EXISTS: "already_exists",
    dnf.callback.STATUS_FAILED: "err",
    dnf.callback.STATUS_MIRROR: "err",
}


class DownloadProgress(dnf.callback.DownloadProgress):
    def __init__(self, out):
        self.out = out

    def start(self, total_files, total_size, total_drpms=0):
        with self.out as out:
            json.dump(
                {
                    "download_started": {
                        "total_files": total_files,
                        "total_bytes": total_size,
                    }
                },
                out,
            )
            out.write("\n")

    def end(self, payload, status, msg):
        if status == dnf.callback.STATUS_ALREADY_EXISTS:
            msg = None
        with self.out as out:
            json.dump(
                {
                    "package_downloaded": {
                        "package": {
                            "name": payload.pkg.name,
                            "evr": payload.pkg.evr,
                            "arch": payload.pkg.arch,
                        },
                        "status": {_DL_STATUS_TO_EVENT[status]: msg},
                    }
                },
                out,
            )
            out.write("\n")


_TX_ACTION_TO_JSON = {
    dnf.callback.PKG_DOWNGRADE: "downgrade",
    dnf.callback.PKG_DOWNGRADED: "downgraded",
    dnf.callback.PKG_INSTALL: "install",
    dnf.callback.PKG_OBSOLETE: "obsolete",
    dnf.callback.PKG_OBSOLETED: "obsoleted",
    dnf.callback.PKG_REINSTALL: "reinstall",
    dnf.callback.PKG_REINSTALLED: "reinstalled",
    dnf.callback.PKG_REMOVE: "remove",
    dnf.callback.PKG_UPGRADE: "upgrade",
    dnf.callback.PKG_UPGRADED: "upgraded",
    dnf.callback.PKG_CLEANUP: "cleanup",
    dnf.callback.PKG_VERIFY: "verify",
    dnf.callback.PKG_SCRIPTLET: "scriptlet",
}


class TransactionProgress(dnf.callback.TransactionProgress):
    def __init__(self, out):
        self.out = out
        self._sent = defaultdict(set)

    def error(self, message):
        with self.out as out:
            json.dump(
                {"tx_error": message},
                out,
            )
            out.write("\n")

    def progress(self, package, action, ti_done, ti_total, ts_done, ts_total):
        with self.out as out:
            if (
                action == dnf.callback.TRANS_POST
                or action == dnf.callback.TRANS_PREPARATION
            ):
                return

            json.dump(
                {
                    "tx_item": {
                        "package": {
                            "name": package.name,
                            "evr": package.evr,
                            "arch": package.arch,
                        },
                        "operation": _TX_ACTION_TO_JSON[action],
                    }
                },
                out,
            )
            out.write("\n")
        self._sent[package].add(action)


def main():
    out = LockedOutput(sys.stdout)
    spec = json.loads(sys.argv[1])
    base = dnf.Base()
    base.conf.installroot = spec["install_root"]
    for id, urls in spec["repos"].items():
        base.repos.add_new_repo(id, dnf.conf.Conf(), urls)

    # read: download repodata
    base.fill_sack()

    for item in spec["items"]:
        action = item["action"]
        source = item["source"]
        source = source["name"] if "name" in source else source["source"]
        if action == "install":
            base.install_specs([source], strict=True)
        elif action == "remove_if_exists":
            # cannot remove by file path, so let's do this to be extra safe
            base.remove(item["source"]["name"])
        else:
            raise RuntimeError(f"unknown action '{action}'")

    base.resolve()
    with out as o:
        json.dump(
            {
                "transaction_resolved": {
                    "install": [
                        {"name": p.name, "evr": p.evr, "arch": p.arch}
                        for p in base.transaction.install_set
                    ],
                    "remove": [
                        {"name": p.name, "evr": p.evr, "arch": p.arch}
                        for p in base.transaction.remove_set
                    ],
                }
            },
            o,
        )
        o.write("\n")

    base.download_packages(base.transaction.install_set, DownloadProgress(out))
    base.do_transaction(TransactionProgress(out))


if __name__ == "__main__":
    main()
