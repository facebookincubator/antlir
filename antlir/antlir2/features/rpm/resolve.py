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
import sys

import antlir2_dnf_base

import dnf
import libdnf

from antlir2_features_rpm_common import (
    AntlirError,
    compute_explicitly_installed_package_names,
    enable_modules,
    LockedOutput,
    package_struct,
)


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


def resolve(out, spec, base, local_rpms, explicitly_installed_package_names):
    explicitly_removed_package_names = set()

    versionlock = spec["versionlock"] or {}
    locked_packages = antlir2_dnf_base.locked_packages(
        sack=base.sack,
        versionlock=versionlock,
        hard_enforce=spec["versionlock_hard_enforce"],
    )

    module_enable = enable_modules(spec["items"], base)

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
                if (
                    not source
                    and spec["versionlock_hard_enforce"]
                    and action in {"install", "upgrade"}
                ):
                    raise AntlirError(
                        f"{rpm['subject']} is locked to version {versionlock[rpm['subject']]}, but that version was not found in any available repository"
                    )
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
                try:
                    base.package_upgrade(source)
                except dnf.exceptions.MarkingError:
                    # If it's not installed, upgrade should behave the same as install
                    base.package_install(source, strict=True)
            else:
                try:
                    base.upgrade(source)
                except dnf.exceptions.PackageNotFoundError as e:
                    with out as o:
                        json.dump({"package_not_found": e.pkg_spec}, o)
                except dnf.exceptions.PackagesNotInstalledError:
                    # If it's not installed, upgrade should behave the same as install
                    base.install(source, strict=True)
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
        elif action == "module_enable":
            # The modules have already been enabled at this point
            pass
        else:
            raise RuntimeError(f"unknown action '{action}'")

    excluded_rpms = set(spec.get("excluded_rpms", []))
    # A user explicitly installing an rpm overrides the exclusion policy.
    # In other words, excluded_rpms only applies to RPMs installed as
    # dependencies, where it's not obvious that they were intended.
    excluded_rpms = excluded_rpms - explicitly_installed_package_names

    antlir2_dnf_base.versionlock_sack(
        sack=base.sack,
        versionlock=versionlock,
        explicitly_installed_package_names=explicitly_installed_package_names,
        excluded_rpms=excluded_rpms,
        hard_enforce=spec["versionlock_hard_enforce"],
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
                    "module_enable": module_enable,
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
    assert spec["mode"] == "resolve"
    out = LockedOutput(sys.stdout)
    base, local_rpms = base_init(spec)
    explicitly_installed_package_names = compute_explicitly_installed_package_names(
        spec, local_rpms
    )

    return resolve(out, spec, base, local_rpms, explicitly_installed_package_names)


def main():
    spec = json.load(sys.stdin)
    driver(spec)


if __name__ == "__main__":
    main()
