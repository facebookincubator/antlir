#!/usr/libexec/platform-python
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# NOTE: this must be run with system python, so cannot be a PAR file
# /usr/bin/dnf itself uses /usr/libexec/platform-python, so by using that we can
# ensure that we're using the same python that dnf itself is using

import json
import logging
import os.path
import platform
import shutil
from contextlib import contextmanager
from pathlib import Path
from typing import ContextManager, Dict, Mapping, Optional, Set

import dnf
import hawkey

log = logging.getLogger("antlir2_dnf_base")


class AntlirError(Exception):
    pass


@contextmanager
def base(
    *, install_root: Optional[str] = None, **configure_base_kwargs
) -> ContextManager[dnf.Base]:
    conf = dnf.conf.Conf()
    conf.read("/__antlir2__/dnf/dnf.conf")
    if install_root:
        conf.installroot = install_root
    with dnf.Base(conf) as base:
        configure_base(base=base, install_root=install_root, **configure_base_kwargs)
        yield base


def configure_base(
    *,
    base: dnf.Base,
    install_root: Optional[str] = None,
    arch: Optional[str] = None,
    set_persistdir_under_installroot: bool = True,
) -> None:
    base.conf.read("/__antlir2__/dnf/dnf.conf")
    if install_root:
        base.conf.installroot = install_root
    if set_persistdir_under_installroot:
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
        with open(basedir / "dnf_conf.json") as f:
            dnf_conf = json.load(f)
            for k, v in dnf_conf.items():
                if hasattr(conf, "set_or_append_opt_value"):
                    conf.set_or_append_opt_value(k, v)
                else:
                    conf._set_value(k, v)
        repo = dnf.repo.Repo(id, conf)
        repo.baseurl = [basedir.as_uri()]
        base.repos.add(repo)
        try:
            shutil.copyfile(
                basedir / "repodata" / f"{id}.solv",
                Path(base.conf.cachedir) / f"{id}.solv",
            )
            shutil.copyfile(
                basedir / "repodata" / f"{id}-filenames.solvx",
                Path(base.conf.cachedir) / f"{id}-filenames.solvx",
            )
            # copy repomd.xml and any other repodata files into dnf's cache dir
            # so that we can use `fill_sack_from_repos_in_cache` to force usage
            # of pre-built solv caches
            repo_cache_dir = Path(repo._repo.getCachedir())
            os.makedirs(repo_cache_dir / "repodata", exist_ok=True)
            for f in (basedir / "repodata").iterdir():
                shutil.copy(f, repo_cache_dir / "repodata" / f.name)
        except FileNotFoundError as e:
            log.warning(
                f"could not copy .solv files, dnf will be substantially slower! {e}"
            )
            # Forcibly crash internally since this is such a huge performance
            # regression - if it stops working we need to know immediately
            # @oss-disable[end= ]: raise e


def _versionlock_query(
    sack: dnf.sack.Sack, versionlock: Mapping[str, str]
) -> dnf.query.Query:
    locked_query = sack.query().filter(empty=True)
    for name, version in versionlock.items():
        pattern = name + "-" + version
        possible_nevras = dnf.subject.Subject(pattern).get_nevra_possibilities()
        for nevra in possible_nevras:
            locked_query = locked_query.union(nevra.to_query(sack))

    return locked_query


def locked_packages(
    *, sack: dnf.sack.Sack, versionlock: Mapping[str, str], hard_enforce: bool = True
) -> Dict[str, Optional[dnf.package.Package]]:
    """
    Turn a requested versionlock into a set of lockable packages, or report that
    the lock for that package cannot be satisfied.

    Historically, antlir2 used to be tolerant of versionlock information being
    inaccurate and it was just a best-effort mechanism, but now our data is of
    high enough quality to make a failure build-blocking.

    The underlying idea is now that a missing versionlock is almost always due
    to an image owner specifying their own versionlock override (because the
    global ones are now of sufficiently high quality), and they should receive a
    build failure if their lock cannot be satisfied.
    """
    if hard_enforce:
        lock = {k: None for k in versionlock.keys()}
        # TODO(vmagro): a single blocklisted RPM is ok, but this **must** be
        # properly solved if this list grows any larger. Even this single RPM
        # really should just be fixed by fixing its versionlock information, but
        # blocking it here is faster and easier and gets us safety across all
        # other RPMs.
        lock.pop("bnxtnvm", None)
    else:
        lock = {}
    for pkg in _versionlock_query(sack, versionlock):
        lock[pkg.name] = pkg
    return lock


def versionlock_sack(
    *,
    sack: dnf.sack.Sack,
    versionlock: Mapping[str, str],
    explicitly_installed_package_names: Set[str],
    excluded_rpms: Set[str],
    hard_enforce: bool = True,
) -> None:
    locked_query = _versionlock_query(sack, versionlock)

    if hard_enforce:
        locked_names = set(versionlock.keys())
    else:
        locked_names = {pkg.name for pkg in locked_query}

    # Explicitly installed package names are excluded from version lock queries.
    # Note that this is not the same as saying they are excluded from version
    # locking, since the version lock will have happened already. These packages
    # are excluded from the queries so that an image owner is able to specify an
    # exact NEVRA and be sure to get that installed, without this query being
    # able to interfere.
    locked_names = locked_names - explicitly_installed_package_names
    all_versions = sack.query().filter(name__glob=list(locked_names))
    disallowed_versions = all_versions.difference(locked_query)
    # ignore already-installed packages
    disallowed_versions = disallowed_versions.filterm(
        reponame__neq=hawkey.SYSTEM_REPO_NAME
    )
    sack.add_excludes(disallowed_versions)
    sack.add_excludes(sack.query().filter(name=list(excluded_rpms)))
