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

import copy
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from urllib.parse import urlparse

import antlir2_dnf_base

import dnf
import hawkey
import libdnf
import rpm as librpm
from antlir2_features_rpm_common import (
    AntlirError,
    compute_explicitly_installed_package_names,
    LockedOutput,
    package_struct,
)
from dnf.i18n import ucd
from dnf.module.module_base import ModuleBase


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
_RPMS_THAT_CAN_FAIL_SCRIPTS = {
    "antlir2-failing-postscripts": "TODO(T166162108)",
    "git-lfs": "TODO(T170621965)",
    "nsight-compute-2019.4.0": "TODO(T166170831)",
    "coreutils-common": "TODO(T182347179)",
    "mono-core": "TODO(T206575712)",
    "digilent.waveforms": "TODO(T248331993)",
    "digilent.adept.runtime": "TODO(T248331993)",
}


_SCRIPTLET_ERROR_RE = re.compile(r"^Error in (?:.*) scriptlet in rpm package (.*)$")


class TransactionProgress(dnf.callback.TransactionProgress):
    def __init__(self, out, ignore_scriptlet_errors: bool = False):
        self.out = out
        self._sent = defaultdict(set)
        self._ignore_scriptlet_errors = ignore_scriptlet_errors
        self.rpms_with_scriptlet_errors = set()

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
            match = _SCRIPTLET_ERROR_RE.match(message)
            if match:
                if self._ignore_scriptlet_errors:
                    key = "tx_warning"
                rpm_name = match.group(1)
                if rpm_name in _RPMS_THAT_CAN_FAIL_SCRIPTS:
                    key = "tx_warning"
                self.rpms_with_scriptlet_errors.add(rpm_name)
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


def base_init(spec):
    base = dnf_base(spec)
    antlir2_dnf_base.add_repos(base=base, repos_dir=spec["repos"])

    # Load .solv files to determine available repos and rpms. This will re-parse
    # repomd.xml, but does not require re-loading all the other large xml blobs,
    # since the .solv{x} files are copied into the cache dir immediately before
    # this. `fill_sack_from_repos_in_cache` will force dnf to use the cached
    # solv files.
    # @oss-disable[end= ]: base.fill_sack_from_repos_in_cache()
    base.fill_sack() # @oss-enable

    # Local rpm files must be added before anything is added to the transaction goal
    # They also don't appear in the recorded transaction resolution, so are
    # common to mode=resolve and mode=run
    local_rpms = {}
    for item in spec["items"]:
        rpm = item["rpm"]
        if "src" in rpm:
            packages = base.add_remote_rpms([os.path.realpath(rpm["src"])])
            local_rpms[rpm["src"]] = packages[0]

    return (base, local_rpms)


def driver(spec) -> None:
    assert spec["mode"] == "run"
    assert "resolved_transaction" in spec

    spec_backup = copy.deepcopy(spec)
    out = LockedOutput(sys.stdout)
    base, local_rpms = base_init(spec)
    explicitly_installed_package_names = compute_explicitly_installed_package_names(
        spec, local_rpms
    )

    # If we have a request to install an already installed rpm, that becomes a
    # no-op within this transaction. At the end of this transaction, we'll make
    # sure to mark the rpm as user installed to prevent it from being
    # implicitly uninstalled by a future rpm removal. But if that rpm was
    # implicitly installed as a dependency of another rpm which is removed in
    # this trasaction, then we'll remove that rpm in this transaction even
    # though it's been explicitly requsested. To avoid this we'll mark any
    # pre-installed rpms as user requested at the start of this trasaction.
    set_user_installed = [
        pkg
        for name in explicitly_installed_package_names
        for pkg in dnf.subject.Subject(name)
        .get_best_query(base.sack, forms=[hawkey.FORM_NAME])
        .installed()
        if REASON_FROM_STRING[pkg.reason]
        != libdnf.transaction.TransactionItemReason_USER
    ]
    if set_user_installed:
        old = base.history.last()
        if old is None:
            rpmdb_version = base._ts.dbCookie()
        else:
            rpmdb_version = old.end_rpmdb_version
        for pkg in set_user_installed:
            base.history.set_reason(pkg, libdnf.transaction.TransactionItemReason_USER)
        base.history.beg(
            rpmdb_version, [], [], "antlir2: correct installed reasons, pre-install"
        )
        base.history.end(rpmdb_version)
        base.close()
        spec = spec_backup
        base, local_rpms = base_init(spec)

    for path, mode in [("/tmp", 0o1777), ("/proc", 0o555), ("/dev", 0o755)]:
        dst = os.path.join(spec["install_root"], path.lstrip("/"))
        # Normally I don't like to implicitly create things in the image, there
        # is a lot of baggage that one must get when they use `rpm` - plus, the
        # 'filesystem' rpm which is transitively required by basically
        # everything would end up creating these mountpoints anyway
        existed = os.path.exists(dst)
        os.makedirs(dst, exist_ok=True, mode=mode)
        if not existed:
            os.chmod(dst, mode)
        subprocess.run(
            ["mount", "--rbind", path, dst],
            check=True,
        )

    # Even though the transaction has been pre-resolved, we need to make sure
    # the versionlock still applies, to prevent dnf from going haywire and
    # resolving the transaction differently the second time.
    # Here, all packages we expect to install are considered "explicitly
    # installed package names" since they are already fully-specificed NEVRAs
    installed_names = {
        p["package"]["name"] for p in spec["resolved_transaction"]["install"]
    }
    antlir2_dnf_base.versionlock_sack(
        sack=base.sack,
        versionlock=spec["versionlock"] or {},
        explicitly_installed_package_names=installed_names,
        # A user explicitly installing an rpm overrides the exclusion policy.
        # In other words, excluded_rpms only applies to RPMs installed as
        # dependencies, where it's not obvious that they were intended.
        excluded_rpms=set(spec.get("excluded_rpms", [])) - installed_names,
    )

    module_base = ModuleBase(base)
    for module_spec in spec["resolved_transaction"]["module_enable"]:
        module_base.enable([module_spec])
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
    gpg_warnings = defaultdict(list)

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
        import_result = subprocess.run(
            [
                "rpmkeys",
                "--import",
                "--verbose",
                "--root",
                spec["install_root"],
                keyfile,
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            encoding="utf8",
            universal_newlines=True,
            check=False,
        )

        if import_result.returncode != 0:
            for pkg in pkgs:
                # It's not necessarily a hard failure if we failed to import a
                # key (CentOS 10 has more aggressive key requirements), but it
                # might be the cause of a later signature validation failure, so
                # make sure that we record it appropriately.
                gpg_warnings[pkg].append(
                    f"failed to import gpg key ({keyfile}): {import_result.stderr.lower()}"
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
                raise AntlirError(f"failed to read {pkg.localPkg()}") from e

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
            # the interface of this is truly awful, but do the best we can to
            # determine if the rpm is signed or not
            if ("key id" not in stdout and "key fingerprint" not in stdout) or (
                "signature" not in stdout
            ):
                gpg_errors[pkg].append("RPM is not signed")

    if gpg_warnings:
        with out as out:
            for pkg, errors in gpg_warnings.items():
                for error in errors:
                    json.dump(
                        {
                            "gpg_warning": {
                                "package": package_struct(pkg),
                                "error": error,
                            }
                        },
                        out,
                    )
                    out.write("\n")
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
    cb = TransactionProgress(
        out, ignore_scriptlet_errors=spec["ignore_scriptlet_errors"]
    )
    try:
        base.do_transaction(cb)
    except dnf.exceptions.Error as e:
        with out as out:
            message = str(e)
            if cb.rpms_with_scriptlet_errors:
                # If we encounter RPMs that must be allowed to have failing
                # scriptlets while using a version of DNF that forcibly fails
                # the transaction if that happens, we can swallow the error here
                # and hope the rest of the installation worked...
                message += " There were scriptlet errors in the following rpms. "
                message += "Newer versions of dnf can consider this a hard failure. "
                message += str(cb.rpms_with_scriptlet_errors)
            json.dump({"tx_error": message}, out)
            out.write("\n")
        sys.exit(1)
    finally:
        base.close()

    # After doing the transaction, ensure that all the package history entries
    # match the actual reason for installation.
    # Otherwise one of two bad things will happen:
    # 1) reinstallation of a package that had previously been brought in as a
    #    dependency will not be recorded with "user' as the install reason
    # 2) installation of a dependency in a pre-resolved transaction will be
    #    marked as "user" installed rather than "dependency"
    base = dnf_base(spec)
    # @oss-disable[end= ]: base.fill_sack_from_repos_in_cache()
    base.fill_sack() # @oss-enable

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
    base.history.beg(
        rpmdb_version, [], [], "antlir2: correct installed reasons, post-install"
    )
    base.history.end(rpmdb_version)


def main():
    spec = json.load(sys.stdin)
    driver(spec)


if __name__ == "__main__":
    main()
