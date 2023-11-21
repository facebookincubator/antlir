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

import base64
import importlib.util
import json
import os
import re
import subprocess
import sys
import threading
from collections import defaultdict
from typing import List
from urllib.parse import urlparse

import dnf
import hawkey
import libdnf
import rpm as librpm
from dnf.i18n import ucd

spec = importlib.util.spec_from_file_location(
    "antlir2_dnf_base", "/__antlir2__/dnf/base.py"
)
antlir2_dnf_base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(antlir2_dnf_base)


class AntlirError(Exception):
    pass


class LockedOutput:
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

# Poorly packaged rpms that have failing postscripts.
#
# When adding RPMs to this list, create a task and model it after T166170831
# (which also has an example of how to investigate _why_ a script is broken)
# Use TODO(Txxxx) so that this entry can be easily tracked in the tasks tool and
# removed when the task is fixed.
_RPMS_THAT_CAN_FAIL_POST_SCRIPTS = {
    "antlir2-failing-postscripts": "TODO(T166162108)",
    "git-lfs": "TODO(T170621965)",
    "nsight-compute-2019.4.0": "TODO(T166170831)",
}


class TransactionProgress(dnf.callback.TransactionProgress):
    def __init__(self, out, ignore_postin_script_error: bool = False):
        self.out = out
        self._sent = defaultdict(set)
        self._ignore_postin_script_error = ignore_postin_script_error

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
            key = "tx_error"
            if self._ignore_postin_script_error and message.startswith(
                "Error in POSTIN scriptlet"
            ):
                key = "tx_warning"
            match = re.match(
                r"^Error in (?:.*) scriptlet in rpm package (.*)$", message
            )
            if match and match.group(1):
                rpm_name = match.group(1)
                if rpm_name in _RPMS_THAT_CAN_FAIL_POST_SCRIPTS:
                    key = "tx_warning"
            json.dump({key: message}, out)
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
    antlir2_dnf_base.configure_base(
        base=base, install_root=spec["install_root"], arch=spec["arch"]
    )
    return base


REASON_TO_STRING = {
    getattr(libdnf.transaction, r): libdnf.transaction.TransactionItemReasonToString(
        getattr(libdnf.transaction, r)
    )
    for r in dir(libdnf.transaction)
    if r.startswith("TransactionItemReason_")
}
REASON_FROM_STRING = {s: r for r, s in REASON_TO_STRING.items()}


def _explicitly_installed_package_names(spec, local_rpms):
    explicitly_installed_package_names = set()
    for item in spec["items"]:
        action = item["action"]
        rpm = item["rpm"]
        if "subject" in rpm:
            source = rpm["subject"]
        elif "src" in rpm:
            source = local_rpms[rpm["src"]]
        else:
            raise AntlirError(f"none of {{subject,src}} not found in: {rpm}")

        if action == "install":
            if isinstance(source, dnf.package.Package):
                explicitly_installed_package_names.add(source.name)
            else:
                explicitly_installed_package_names.update(
                    {
                        nevra.name
                        for nevra in dnf.subject.Subject(
                            source
                        ).get_nevra_possibilities()
                    }
                )

    return explicitly_installed_package_names


def resolve(out, spec, base, local_rpms, explicitly_installed_package_names):
    explicitly_removed_package_names = set()

    versionlock = spec["versionlock"] or {}
    locked_packages = antlir2_dnf_base.locked_packages(
        sack=base.sack, versionlock=versionlock
    )

    for item in spec["items"]:
        action = item["action"]
        rpm = item["rpm"]
        if "subject" in rpm:
            source = rpm["subject"]
            # If the versionlock contains a match for this name, install that
            # package.  If an image author specifies an exact NEVRA, this
            # condition will be false, which is our versionlock opt-out
            # mechanism.
            if source in locked_packages:
                source = locked_packages[source]
        else:
            source = local_rpms[rpm["src"]]

        if action == "install":
            if isinstance(source, dnf.package.Package):
                base.package_install(source, strict=True)
            else:
                try:
                    base.install(source, strict=True)
                except dnf.exceptions.PackageNotFoundError as e:
                    with out as o:
                        json.dump({"package_not_found": e.pkg_spec}, o)
        elif action == "upgrade":
            if isinstance(source, dnf.package.Package):
                base.package_upgrade(source)
            else:
                try:
                    base.upgrade(source)
                except dnf.exceptions.PackageNotFoundError as e:
                    with out as o:
                        json.dump({"package_not_found": e.pkg_spec}, o)
        elif action == "remove":
            # cannot remove by file path, so let's do this to be extra safe
            try:
                base.remove(rpm["subject"])
            except dnf.exceptions.PackagesNotInstalledError:
                with out as o:
                    json.dump({"package_not_installed": rpm["subject"]}, o)
            explicitly_removed_package_names.add(rpm["subject"])
        elif action == "remove_if_exists":
            # cannot remove by file path, so let's do this to be extra safe
            try:
                base.remove(rpm["subject"])
            except dnf.exceptions.PackagesNotInstalledError:
                # The action is 'remove_if_exists'...
                # We should probably have a 'remove' version as well to
                # force users to clean up features that are no longer doing
                # anything
                pass
            explicitly_removed_package_names.add(rpm["subject"])
        else:
            raise RuntimeError(f"unknown action '{action}'")

    antlir2_dnf_base.versionlock_sack(
        sack=base.sack,
        versionlock=versionlock,
        explicitly_installed_package_names=explicitly_installed_package_names,
        excluded_rpms=spec.get("excluded_rpms", []),
    )

    base.resolve(allow_erasing=True)

    def _try_get_repoid(p):
        try:
            return p.pkg.repo.id
        except KeyError:
            return None

    with out as o:
        json.dump(
            {
                "transaction_resolved": {
                    "install": [
                        {
                            "package": package_struct(p.pkg),
                            "repo": _try_get_repoid(p),
                            "reason": REASON_TO_STRING[p.reason],
                        }
                        for p in base.transaction
                        if p.action
                        # See some documentation of the different actions here
                        # https://github.com/rpm-software-management/libdnf/blob/3fca06e8b1037f117ba57b5e824ea59a343b44ed/libdnf/transaction/Types.hpp#L60
                        in {
                            libdnf.transaction.TransactionItemAction_INSTALL,
                            libdnf.transaction.TransactionItemAction_REINSTALL,
                            libdnf.transaction.TransactionItemAction_DOWNGRADE,
                            libdnf.transaction.TransactionItemAction_OBSOLETE,
                            libdnf.transaction.TransactionItemAction_UPGRADE,
                            libdnf.transaction.TransactionItemAction_REASON_CHANGE,
                        }
                    ],
                    "remove": [package_struct(p) for p in base.transaction.remove_set],
                }
            },
            o,
        )
        o.write("\n")

    try:
        antlir2_dnf_base.ensure_no_implicit_removes(
            base=base,
            explicitly_removed_package_names=explicitly_removed_package_names,
        )
    except Exception as e:
        with out as o:
            json.dump({"tx_error": str(e)}, o)


def _parse_gpg_keys(rawkey: str) -> List[bytes]:
    rawkey = rawkey.replace("\r\n", "\n").replace("\r", "\n")
    keys = []
    inblock = False
    block = ""
    for line in rawkey.splitlines():
        if line == "-----BEGIN PGP PUBLIC KEY BLOCK-----":
            inblock = True
            block = ""
        elif inblock:
            if line == "-----END PGP PUBLIC KEY BLOCK-----":
                inblock = False
                keys.append(base64.b64decode(block))
                continue
            if line.startswith("Version:"):
                continue
            if line.startswith("="):
                continue
            block += line.strip()
    return keys


def driver(spec) -> None:
    out = LockedOutput(sys.stdout)
    mode = spec["mode"]

    base = dnf_base(spec)
    antlir2_dnf_base.add_repos(base=base, repos_dir=spec["repos"])

    # Load .solv files to determine available repos and rpms. This will re-parse
    # repomd.xml, but does not require re-loading all the other large xml blobs,
    # since the .solv{x} files are copied into the cache dir immediately before
    # this. Ideally we could use `fill_sack_from_repos_in_cache`, but that
    # requires knowing the dnf cache key (like /antlir/dnf-cache/repo-HEXSTRING)
    # which is based on the base url. We don't have a persistent baseurl, but
    # this is incredibly fast anyway.
    base.fill_sack()

    # Local rpm files must be added before anything is added to the transaction goal
    # They also don't appear in the recorded transaction resolution, so are
    # common to mode=resolve and mode=run
    local_rpms = {}
    for item in spec["items"]:
        rpm = item["rpm"]
        if "src" in rpm:
            packages = base.add_remote_rpms([os.path.realpath(rpm["src"])])
            local_rpms[rpm["src"]] = packages[0]

    explicitly_installed_package_names = _explicitly_installed_package_names(
        spec, local_rpms
    )

    if mode == "resolve":
        return resolve(out, spec, base, local_rpms, explicitly_installed_package_names)

    assert mode == "run"
    assert "resolved_transaction" in spec

    for install in spec["resolved_transaction"]["install"]:
        base.install(
            install["nevra"],
            forms=[hawkey.FORM_NEVRA],
            reponame=install["repo"],
        )
    for nevra in spec["resolved_transaction"]["remove"]:
        base.remove(nevra, forms=[hawkey.FORM_NEVRA])

    # We actually do need to resolve again, but we've explicitly told dnf every
    # single package to install and remove
    base.resolve()

    # Check the GPG signatures for all the to-be-installed packages before doing
    # the transaction
    gpg_errors = defaultdict(list)

    # Import all the GPG keys for repos that we're installing packages from
    import_keys = defaultdict(list)
    for pkg in base.transaction.install_set:
        # @commandline repo (local files) does not have a config and so an
        # attempt to access 'pkg.repo' raises a KeyError
        if pkg.reponame == hawkey.CMDLINE_REPO_NAME:
            continue
        # Import all the GPG keys for this repo
        for keyfile in pkg.repo.gpgkey:
            import_keys[keyfile].append(pkg)
    for keyfile, pkgs in import_keys.items():
        uri = urlparse(keyfile)
        keyfile = os.path.abspath(os.path.join(uri.netloc, uri.path))
        with open(keyfile, "r") as f:
            keytext = f.read()
            keys = _parse_gpg_keys(keytext)
        for key in keys:
            ret = base._ts.pgpImportPubkey(key)
            if ret != 0:
                for pkg in pkgs:
                    gpg_errors[pkg].append(
                        f"failed to import pubkey ({keyfile}): {ret}"
                    )

    for pkg in base.transaction.install_set:
        # If the package comes from a repo without a GPG key, don't bother
        # checking its signature. If the repo is @commandline (aka, a local
        # file), skip gpg checking (the author is assumed to know what they're
        # doing).
        if pkg.reponame == hawkey.CMDLINE_REPO_NAME or not pkg.repo.gpgkey:
            continue

        # reading the header will cause rpm to do a gpg check
        try:
            with open(pkg.localPkg(), "rb") as f:
                base._ts.hdrFromFdno(f.fileno())
        except librpm.error as e:
            msg = str(e)
            if "key" in msg:
                gpg_errors[pkg].append(msg)
            else:
                raise e

        # If the rpm is unsigned but there are gpg keys for the repo, block the installation
        if pkg.repo.gpgkey:
            stdout = subprocess.run(
                [
                    "rpmkeys",
                    "--checksig",
                    "--verbose",
                    "--define=_pkgverify_level signature",
                    pkg.localPkg(),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                encoding="utf8",
                universal_newlines=True,
                check=False,
            ).stdout.lower()
            if ("key id" not in stdout) or ("signature" not in stdout):
                gpg_errors[pkg].append("RPM is not signed")

    if gpg_errors:
        with out as out:
            for pkg, errors in gpg_errors.items():
                for error in errors:
                    json.dump(
                        {"gpg_error": {"package": package_struct(pkg), "error": error}},
                        out,
                    )
                    out.write("\n")
        sys.exit(1)

    # setting base.args will record a comment in the history db
    base.args = [
        "antlir2",
        spec["layer_label"],
        json.dumps(spec["resolved_transaction"], sort_keys=True),
    ]

    # dnf go brrr
    base.do_transaction(
        TransactionProgress(
            out, ignore_postin_script_error=spec["ignore_postin_script_error"]
        )
    )
    base.close()

    # After doing the transaction, ensure that all the package history entries
    # match the actual reason for installation.
    # Otherwise one of two bad things will happen:
    # 1) reinstallation of a package that had previously been brought in as a
    #    dependency will not be recorded with "user' as the install reason
    # 2) installation of a dependency in a pre-resolved transaction will be
    #    marked as "user" installed rather than "dependency"
    base = dnf_base(spec)
    base.fill_sack()

    set_reasons = []
    for install in spec["resolved_transaction"]["install"]:
        subject = dnf.subject.Subject(install["nevra"])
        set_reasons.extend(
            [
                (pkg, REASON_FROM_STRING[install["reason"]])
                for pkg in subject.get_best_query(
                    base.sack, forms=[hawkey.FORM_NEVRA]
                ).installed()
            ]
        )
    # The above queries will not pick up any re-installed packages because dnf
    # treats that as a no-op. This query looks for currently (after the
    # transaction has been run) installed packages that have the same name as
    # the packages that are being explicitly installed in this transaction.
    for name in explicitly_installed_package_names:
        subject = dnf.subject.Subject(name)
        set_reasons.extend(
            [
                (pkg, libdnf.transaction.TransactionItemReason_USER)
                for pkg in subject.get_best_query(
                    base.sack, forms=[hawkey.FORM_NAME]
                ).installed()
            ]
        )

    if spec["resolved_transaction"]["install"] and not set_reasons:
        json.dump(
            {
                "tx_error": "installed packages, but history marking query returned nothing"
            },
            out,
        )
        sys.exit(1)

    for pkg, reason in set_reasons:
        if REASON_FROM_STRING[pkg.reason] != reason:
            base.history.set_reason(pkg, reason)
    # commit that change to the db
    rpmdb_version = base.history.last().end_rpmdb_version
    base.history.beg(rpmdb_version, [], [], "antlir2: correct installed reasons")
    base.history.end(rpmdb_version)


def main():
    spec = json.load(sys.stdin)
    driver(spec)


if __name__ == "__main__":
    main()
