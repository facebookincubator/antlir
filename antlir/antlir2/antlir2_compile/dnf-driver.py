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
import os
import shutil
import sys
import threading
from collections import defaultdict
from pathlib import Path

import dnf
import hawkey


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


def package_struct(pkg):
    return {
        "name": pkg.name,
        "epoch": pkg.epoch,
        "version": pkg.version,
        "release": pkg.release,
        "arch": pkg.arch,
    }


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
        if action in self._sent[package]:
            return
        with self.out as out:
            if (
                action == dnf.callback.TRANS_POST
                or action == dnf.callback.TRANS_PREPARATION
            ):
                return

            json.dump(
                {
                    "tx_item": {
                        "package": package_struct(package),
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
    mode = spec["mode"]
    base = dnf.Base()
    base.conf.installroot = spec["install_root"]
    base.conf.persistdir = os.path.join(
        spec["install_root"], base.conf.persistdir.lstrip("/")
    )
    os.makedirs("/antlir/dnf-cache", exist_ok=True)
    base.conf.cachedir = "/antlir/dnf-cache"
    base.conf.ignorearch = True
    base.conf.arch = spec["arch"]
    # Image authors should be explicit about what packages they want to install,
    # and we will not bloat their image with weak dependencies that they didn't
    # ask for
    base.conf.install_weak_deps = False
    versionlock = spec["versionlock"] or {}

    for repomd in Path(spec["repos"]).glob("**/*/repodata/repomd.xml"):
        basedir = repomd.parent.parent.resolve()
        id = str(repomd.parent.parent.relative_to(spec["repos"]))
        base.repos.add_new_repo(id, dnf.conf.Conf(), [basedir.as_uri()])
        shutil.copyfile(
            basedir / "repodata" / f"{id}.solv", f"/antlir/dnf-cache/{id}.solv"
        )
        shutil.copyfile(
            basedir / "repodata" / f"{id}-filenames.solvx",
            f"/antlir/dnf-cache/{id}-filenames.solvx",
        )

    # Load .solv files to determine available repos and rpms. This will re-parse
    # repomd.xml, but does not require re-loading all the other large xml blobs,
    # since the .solv{x} files are copied into the cache dir immediately before
    # this. Ideally we could use `fill_sack_from_repos_in_cache`, but that
    # requires knowing the dnf cache key (like /antlir/dnf-cache/repo-HEXSTRING)
    # which is based on the base url. We don't have a persistent baseurl, but
    # this is incredibly fast anyway.
    base.fill_sack()

    # local rpm files must be added before anything is added to the transaction goal
    local_rpms = {}
    for item in spec["items"]:
        rpm = item["rpm"]
        if "source" in rpm:
            packages = base.add_remote_rpms([os.path.realpath(rpm["source"])])
            local_rpms[rpm["source"]] = packages[0]

    explicitly_installed_package_names = set()

    for item in spec["items"]:
        action = item["action"]
        rpm = item["rpm"]
        if "name" in rpm:
            source = rpm["name"]
            # If the versionlock specifies an exact version, construct a NEVRA
            # from it instead of using just name. If an image owner specifies an
            # exact NEVRA, this condition will be false, which is our
            # versionlock opt-out mechanism.
            if source in versionlock:
                source = source + "-" + versionlock[source]
        else:
            source = local_rpms[rpm["source"]]

        if action == "install":
            if isinstance(source, dnf.package.Package):
                base.package_install(source, strict=True)
                explicitly_installed_package_names.add(source.name)
            else:
                base.install(source, strict=True)
                explicitly_installed_package_names.update(
                    {
                        nevra.name
                        for nevra in dnf.subject.Subject(
                            source
                        ).get_nevra_possibilities()
                    }
                )
        elif action == "remove_if_exists":
            # cannot remove by file path, so let's do this to be extra safe
            try:
                base.remove(rpm["name"])
            except dnf.exceptions.PackagesNotInstalledError:
                # The action is 'remove_if_exists'...
                # We should probably have a 'remove' version as well to
                # force users to clean up features that are no longer doing
                # anything
                pass
        else:
            raise RuntimeError(f"unknown action '{action}'")

    # Explicitly installed package names are excluded from version lock queries.
    # Note that this is not the same as saying they are excluded from version
    # locking, since the version lock will have happened already. These packages
    # are excluded from the queries so that an image owner is able to specify an
    # exact NEVRA and be sure to get that installed, without this query being
    # able to interfere.
    locked_query = base.sack.query().filter(empty=True)
    for name, version in versionlock.items():
        pattern = name + "-" + version
        possible_nevras = dnf.subject.Subject(pattern).get_nevra_possibilities()
        for nevra in possible_nevras:
            locked_query = locked_query.union(nevra.to_query(base.sack))
    # locked_query now has the exact versions of all the packages that should be
    # locked, excluding packages that have been explicitly installed
    locked_names = set(versionlock.keys()) - explicitly_installed_package_names
    all_versions = base.sack.query().filter(name__glob=list(locked_names))
    disallowed_versions = all_versions.difference(locked_query)
    # ignore already-installed packages
    disallowed_versions = disallowed_versions.filterm(
        reponame__neq=hawkey.SYSTEM_REPO_NAME
    )
    base.sack.add_excludes(disallowed_versions)

    base.resolve(allow_erasing=True)
    with out as o:
        json.dump(
            {
                "transaction_resolved": {
                    "install": [
                        {
                            "package": package_struct(p),
                            "repo": p.repo.id,
                        }
                        for p in base.transaction.install_set
                        # local rpm files get this "repo" which doesn't actually
                        # exist, and it's a local file so we don't need to push
                        # it back up into buck2 since it's already available as
                        # a dep on this feature
                        if p.reponame != hawkey.CMDLINE_REPO_NAME
                    ],
                    "remove": [package_struct(p) for p in base.transaction.remove_set],
                }
            },
            o,
        )
        o.write("\n")

    if mode == "resolve-only":
        return
    elif mode == "run":
        base.do_transaction(TransactionProgress(out))
    else:
        raise RuntimeError(f"unknown mode '{mode}'")


if __name__ == "__main__":
    main()
