#!/usr/libexec/platform-python
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# NOTE: this must be run with system python, so cannot be a PAR file
# /usr/bin/dnf itself uses /usr/libexec/platform-python, so by using that we can
# ensure that we're using the same python that dnf itself is using

import os.path
import platform
import shutil
from pathlib import Path
from typing import Mapping, Optional, Set

import dnf
import hawkey


class AntlirError(Exception):
    pass


def configure_base(
    *, base: dnf.Base, install_root: Optional[str] = None, arch: Optional[str] = None
) -> None:
    base.conf.read("/__antlir2__/dnf/dnf.conf")
    if install_root:
        base.conf.installroot = install_root
    base.conf.persistdir = os.path.join(
        base.conf.installroot, base.conf.persistdir.lstrip("/")
    )
    base.conf.arch = arch or platform.uname().machine


def ensure_no_implicit_removes(
    *,
    base: dnf.Base,
    explicitly_removed_package_names: Set[str],
) -> None:
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
        raise AntlirError(
            "This transaction would remove some explicitly installed packages. "
            + "Modify the image features to explicitly remove these packages: "
            + ", ".join(p.name for p in implicitly_removed_user_packages)
        )


def add_repos(*, base: dnf.Base, repos_dir: Path) -> None:
    for repomd in Path(repos_dir).glob("**/*/repodata/repomd.xml"):
        basedir = repomd.parent.parent.resolve()
        id = str(repomd.parent.parent.relative_to(repos_dir))
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
        else:
            if hasattr(conf, "set_or_append_opt_value"):
                conf.set_or_append_opt_value("gpgcheck", "0")
            else:
                conf._set_value("gpgcheck", "0")
        repo = dnf.repo.Repo(id, conf)
        repo.baseurl = [basedir.as_uri()]
        base.repos.add(repo)
        shutil.copyfile(
            basedir / "repodata" / f"{id}.solv",
            Path(base.conf.cachedir) / f"{id}.solv",
        )
        shutil.copyfile(
            basedir / "repodata" / f"{id}-filenames.solvx",
            Path(base.conf.cachedir) / f"{id}-filenames.solvx",
        )


def versionlock_sack(
    *,
    sack: dnf.sack.Sack,
    versionlock: Mapping[str, str],
    explicitly_installed_package_names: Set[str],
    excluded_rpms: Set[str],
) -> None:
    # Explicitly installed package names are excluded from version lock queries.
    # Note that this is not the same as saying they are excluded from version
    # locking, since the version lock will have happened already. These packages
    # are excluded from the queries so that an image owner is able to specify an
    # exact NEVRA and be sure to get that installed, without this query being
    # able to interfere.
    locked_query = sack.query().filter(empty=True)
    for name, version in versionlock.items():
        pattern = name + "-" + version
        possible_nevras = dnf.subject.Subject(pattern).get_nevra_possibilities()
        for nevra in possible_nevras:
            locked_query = locked_query.union(nevra.to_query(sack))
    # locked_query now has the exact versions of all the packages that should be
    # locked, excluding packages that have been explicitly installed
    locked_names = set(versionlock.keys()) - explicitly_installed_package_names
    all_versions = sack.query().filter(name__glob=list(locked_names))
    disallowed_versions = all_versions.difference(locked_query)
    # ignore already-installed packages
    disallowed_versions = disallowed_versions.filterm(
        reponame__neq=hawkey.SYSTEM_REPO_NAME
    )
    sack.add_excludes(disallowed_versions)
    sack.add_excludes(sack.query().filter(name=list(excluded_rpms)))
