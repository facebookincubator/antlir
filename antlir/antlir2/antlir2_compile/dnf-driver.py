#!/usr/libexec/platform-python
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
# /usr/bin/dnf itself uses /usr/libexec/platform-python, so by using that we can
# ensure that we're using the same python that dnf itself is using

import json
import os
import shutil
import sys
import threading
from collections import defaultdict
from pathlib import Path

import dnf
import hawkey
import libdnf
from dnf.i18n import ucd


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

    def scriptout(self, msgs):
        """Hook for reporting an rpm scriptlet output.

        :param msgs: the scriptlet output
        """
        if msgs:
            with self.out as out:
                json.dump(
                    {"scriptlet_output": ucd(msgs)},
                    out,
                )
                out.write("\n")

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


def dnf_base(spec) -> dnf.Base:
    base = dnf.Base()
    base.conf.installroot = spec["install_root"]
    base.conf.persistdir = os.path.join(
        spec["install_root"], base.conf.persistdir.lstrip("/")
    )
    os.makedirs("/antlir/dnf-cache", exist_ok=True)
    base.conf.cachedir = "/antlir/dnf-cache"
    base.conf.ignorearch = True
    base.conf.arch = spec["arch"]
    # Transactions might not resolve to the newest version of every package.
    # That is fine and normal, allow the depsolver to do its thing. This is the
    # default behavior of dnf already, but let's be explicit.
    base.conf.best = False
    # Image authors should be explicit about what packages they want to install,
    # and we will not bloat their image with weak dependencies that they didn't
    # ask for
    base.conf.install_weak_deps = False
    base.conf.gpgcheck = True
    base.conf.localpkg_gpgcheck = True
    base.conf.assumeyes = True
    return base


def main():
    out = LockedOutput(sys.stdout)
    spec = json.loads(sys.argv[1])
    mode = spec["mode"]
    versionlock = spec["versionlock"] or {}

    base = dnf_base(spec)

    for repomd in Path(spec["repos"]).glob("**/*/repodata/repomd.xml"):
        basedir = repomd.parent.parent.resolve()
        id = str(repomd.parent.parent.relative_to(spec["repos"]))
        conf = dnf.conf.RepoConf(base.conf)
        conf.cacheonly = False
        conf.substitutions = {}
        conf.check_config_file_age = True
        if (basedir / "gpg-keys").exists():
            uris = []
            for key in (basedir / "gpg-keys").iterdir():
                uris.append(key.as_uri())
            if hasattr(conf, "set_or_append_opt_value"):
                conf.set_or_append_opt_value("gpgcheck", "1")
                conf.set_or_append_opt_value("gpgkey", "\n".join(uris))
            else:
                conf._set_value("gpgcheck", "1")
                conf._set_value("gpgkey", "\n".join(uris))
        base.repos.add_new_repo(id, conf, [basedir.as_uri()])
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
    explicitly_removed_package_names = set()

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
            explicitly_removed_package_names.add(rpm["name"])
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
    base.sack.add_excludes(base.sack.query().filter(name=spec.get("excluded_rpms")))

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

    # We never want to remove an rpm that an image author explicitly installed
    # with a `feature.rpms_install`, unless the author explicitly removes it
    # with `feature.rpms_remove(_if_exists)`. Transaction resolution can
    # potentially end up wanting to remove one of these
    # explicitly-user-installed packages if the user does request the removal of
    # a package that that depends on. If this is the case, we should refuse to
    # perform the transaction.

    # First, find the packages that the user explicitly installed, excluding any
    # dependencies of those packages
    user_installed_packages = {
        pkg for pkg in base.sack.query().installed() if pkg.reason == "user"
    }

    all_removed_packages = set()
    for tx_item in base.transaction:
        # Only track packages that are being _removed_, not upgraded or
        # downgraded
        if tx_item.action == dnf.transaction.PKG_REMOVE:
            all_removed_packages.add(tx_item.pkg)
    # As a safety check, make sure that we were able to discover at least one
    # user-installed package. If not, the guarantees about not silently removing
    # user-installed rpms obviously cannot be ensured.
    if all_removed_packages:
        assert (
            user_installed_packages
        ), "did not find any user-installed packages, refusing to continue"
    # Second, find the packages being implicitly removed in this transaction
    implicitly_removed = {
        pkg
        for pkg in all_removed_packages
        if pkg.name not in explicitly_removed_package_names
    }

    # Lastly, if any of these implicitly removed packages were originally
    # installed by explicit user intention, fail the transaction
    implicitly_removed_user_packages = implicitly_removed & user_installed_packages
    if implicitly_removed_user_packages:
        with out as out:
            json.dump(
                {
                    "tx_error": "This transaction would remove some explicitly installed packages. "
                    + "Modify the image features to explicitly remove these packages: "
                    + ", ".join(p.name for p in implicitly_removed_user_packages)
                },
                out,
            )
            out.write("\n")
        sys.exit(1)

    if mode == "resolve-only":
        return

    assert mode == "run"

    # Check the GPG signatures for all the to-be-installed packages before doing
    # the transaction
    gpg_errors = {}
    for pkg in base.transaction.install_set:
        # If the package comes from a repo without a GPG key, don't bother
        # checking its signature. If the repo is @commandline (aka, a local
        # file), skip gpg checking (the author is assumed to know what they're
        # doing).
        if pkg.reponame == hawkey.CMDLINE_REPO_NAME or not pkg.repo.gpgkey:
            continue

        code, error = base.package_signature_check(pkg)
        if code == 0:
            continue  # gpg check is ok!
        elif code == 1:
            # If the key(s) for the repo aren't installed, install them now,
            # which also checks the signature on this package
            try:
                base.package_import_key(pkg)
            except Exception as e:
                gpg_errors[pkg] = str(e)
        else:
            gpg_errors[pkg] = error

    if gpg_errors:
        with out as out:
            for pkg, error in gpg_errors.items():
                json.dump(
                    {"gpg_error": {"package": package_struct(pkg), "error": error}},
                    out,
                )
                out.write("\n")
        sys.exit(1)

    # dnf go brrr
    base.do_transaction(TransactionProgress(out))
    base.close()

    # After doing the transaction, go through and (re)mark all the
    # explicitly packages as explicitly installed. Otherwise a reinstall of
    # a package that had previously been brought in as a dependency will not
    # be recorded with "user' as the install reason
    base = dnf_base(spec)
    base.fill_sack()
    explicitly_installed_packages = list(
        base.sack.query()
        .installed()
        .filter(name__eq=explicitly_installed_package_names)
    )
    if explicitly_installed_package_names:
        assert (
            explicitly_installed_package_names
        ), "installing packages, but they were not found"

    for pkg in explicitly_installed_packages:
        base.history.set_reason(pkg, libdnf.transaction.TransactionItemReason_USER)
    # commit that change to the db
    rpmdb_version = base.history.last().end_rpmdb_version
    base.history.beg(rpmdb_version, [], [])
    base.history.end(rpmdb_version)


if __name__ == "__main__":
    main()
