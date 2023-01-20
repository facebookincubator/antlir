#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
XXX
"""
import argparse
import asyncio
import glob
import json
import os
import sqlite3
import sys
import textwrap
from contextlib import contextmanager, ExitStack
from typing import (  # This is a 3.8+ feature
    Any,
    Callable,
    FrozenSet,
    Iterable,
    List,
    Literal,
    Mapping,
    NamedTuple,
    Set,
    TextIO,
    Tuple,
    Union,
)

from antlir.common import get_logger, init_logging, set_new_key
from antlir.fs_utils import create_ro, Path, populate_temp_dir_and_rename

from antlir.rpm.allowed_versions.envra import SortableENVRA, SortableEVRA
from antlir.rpm.allowed_versions.package_group import PackageGroup
from antlir.rpm.allowed_versions.version_policy import VersionPolicy
from antlir.rpm.common import readonly_snapshot_db


# XXX just accept Pluggable in place of a Union?
# Dirty hack... fix this later?
def Literalish(x):
    return Literal[x]


log = get_logger()

# Must match `rpm_vset` in `oss_shim*` Starlark files.
_EMPTY_VSET_PATH = "empty=rpm=vset"

_PLUGGABLE_TO_DIR_NAME = {
    PackageGroup: "package_group",
    VersionPolicy: "version_policy",
}
# Not sure how to make the next 2 derive from _PLUGGABLE_TO_DIR_NAME
LiteralKnownPluggable = Union[Literalish(PackageGroup), Literalish(VersionPolicy)]
KnownPluggable = Union[PackageGroup, VersionPolicy]
# pyre-fixme[6]: Expected `Tuple[typing.Any,
#  typing.Type[Variable[$synthetic_attribute_resolution_variable]]]` for 1st
#  param but got `Tuple[typing.Any, object]`.
LoadConfigFns = Mapping[LiteralKnownPluggable, Mapping[str, Callable[..., Any]]]


# FB's TARGETS file auto-formatting requires double-quotes, while Python
# (much more reasonably) prefers single quotes.
def _drepr(s) -> str:
    return '"' + s.encode("unicode_escape").decode("ascii").replace('"', '\\"') + '"'


class _PluginRef(NamedTuple):
    # NB: This could be inferred from plugin, but being explicit is cleaner
    # pyre-fixme[11]: Annotation `LiteralKnownPluggable` is not defined as a
    # type.
    pluggable: LiteralKnownPluggable
    plugin: KnownPluggable

    def dir(self, snapshot_root: Path) -> Path:
        if hasattr(self.plugin, "SNAPSHOT_DIR"):
            # pyre-fixme[16]: `PackageGroup` has no attribute `SNAPSHOT_DIR`.
            snapshot_dir = self.plugin.SNAPSHOT_DIR
        else:
            snapshot_dir = Path(_PLUGGABLE_TO_DIR_NAME[self.pluggable]) / self.kind()
        return snapshot_root / snapshot_dir

    def kind(self) -> str:
        return self.plugin._pluggable_kind


class _PluginDriver:
    def __init__(self, snapshot_root: Path, flavor: str) -> None:
        self._snapshot_root = snapshot_root
        self._flavor = flavor

        # Our plugins don't take arguments, yet. If we needed to take
        # arguments, we could optionally take plugin overrides on the
        # command line like so:
        #     for plugin_key in KEY_TO_PLUGGABLE:
        #         KEY_TO_PLUGGABLE[plugin_key].add_argparse_arg(
        #             parser, f'--plugin-{plugin_key.replace("_", "-")}',
        #             action='append',
        #             help='Snapshot data needed to run this plugin. '
        #                 'May be repeated. ',
        #         )
        self._plugins = [
            _PluginRef(pluggable=pluggable, plugin=cls())
            for pluggable in _PLUGGABLE_TO_DIR_NAME
            for cls in pluggable._pluggable_kind_to_cls.values()
        ]

    async def _update_snapshot(self, plugin) -> None:
        if hasattr(plugin.plugin, "snapshot"):
            plugin_dir = plugin.dir(self._snapshot_root)
            os.makedirs(plugin_dir.dirname(), exist_ok=True)
            with populate_temp_dir_and_rename(plugin_dir, overwrite=True) as td:
                await plugin.plugin.snapshot(td)

    async def update_snapshots(self) -> None:
        "Runs the snapshots in parallel for a modest speedup"
        await asyncio.gather(
            *(self._update_snapshot(plugin) for plugin in self._plugins)
        )

    # XXX abstraction lets each plugin precompute some stuff before quickly
    # serving point queries for its configs.
    @contextmanager
    # pyre-fixme[11]: Annotation `LoadConfigFns` is not defined as a type.
    def prepare_load_config_fns(self) -> LoadConfigFns:
        with ExitStack() as stack:
            pluggable_to_kind_to_load_config = {}
            for plugin in self._plugins:
                kind_to_lc = pluggable_to_kind_to_load_config.setdefault(
                    plugin.pluggable, {}
                )
                set_new_key(
                    kind_to_lc,
                    plugin.kind(),
                    stack.enter_context(
                        plugin.plugin.load_config_fn(
                            plugin.dir(self._snapshot_root),
                            self._flavor,
                        )
                    ),
                )
            log.info(f"XXXplugins {pluggable_to_kind_to_load_config}")
            yield pluggable_to_kind_to_load_config


def _load_package_names(
    cfg: Union[str, Mapping[str, Any]],
    group_to_packages_fn: Mapping[str, Callable[..., Iterable[str]]],
) -> Iterable[str]:
    cfg = (
        {"source": "manual", "names": cfg}
        if isinstance(cfg, list)
        # pyre-fixme[16]: `Mapping` has no attribute `copy`.
        else cfg.copy()
    )
    pg_src = cfg.pop("source")
    try:
        return group_to_packages_fn[pg_src](**cfg)
    except Exception:
        raise RuntimeError(f"Loading package_group {pg_src}")


def _load_policy_versions_for_packages(
    packages: Iterable[str],
    policy_cfg: Union[str, Mapping[str, Any]],
    policy_to_versions_fn: Mapping[str, Callable[..., FrozenSet[SortableEVRA]]],
) -> FrozenSet[SortableEVRA]:
    policy_cfg = (
        {"policy": policy_cfg}
        if isinstance(policy_cfg, str)
        # pyre-fixme[16]: `Mapping` has no attribute `copy`.
        else policy_cfg.copy()
    )
    policy = policy_cfg.pop("policy")
    get_versions_fn = policy_to_versions_fn[policy]

    # The policy is responsible for picking versions that can be applied
    # uniformly to all packages in the package group.  We don't want to
    # support heterogeneous versions within a package group because
    # package group members normally have strict interdependencies, and
    # locking one without locking the others will tend to confuse the
    # package manager into choosing unresolvable dependenceis.
    evras = get_versions_fn(packages=packages, **policy_cfg)
    # Fixme: this EVRA is implemented as an ENVRA
    assert all(evra.name is None for evra in evras), evras
    return evras


class VersionedPackageGroup(NamedTuple):
    group_id: str  # Path written to the TARGETS file for each package
    oncall: str
    packages: FrozenSet[str]
    evras: FrozenSet[SortableEVRA]


def _load_version_sets(
    config_paths: Iterable[Path], load_config_fns, version_sets: Set[str]
) -> Mapping[str, Iterable[VersionedPackageGroup]]:
    log.info(f"XXX Known version sets: {version_sets}")
    load_config_fns = load_config_fns.copy()
    group_fn = load_config_fns.pop(PackageGroup)
    policy_fn = load_config_fns.pop(VersionPolicy)
    assert not load_config_fns, f"Unused pluggables: {load_config_fns}"

    # Below, we do a duplicate package name check.  Segment it by version
    # set to allow splitting package group configs for different version
    # sets into different files.
    vset_to_pkg_to_vpgroup = {}
    vset_to_vpgroups = {vset: [] for vset in version_sets}
    known_group_ids = set()

    for group_path in set(config_paths):
        try:
            json_suffix = b".json"
            assert group_path.endswith(json_suffix), group_path
            group_id = os.path.basename(group_path[: -len(json_suffix)]).decode()
            assert group_id not in known_group_ids, group_id

            with open(group_path) as f:
                group_cfg = json.load(f)

            oncall = group_cfg.pop("oncall")

            packages_cfg = group_cfg.pop("packages")
            packages = frozenset(_load_package_names(packages_cfg, group_fn))
            log.info(f"XXX1 {packages_cfg} {packages}")

            for vset, policy_cfg in group_cfg.pop("version_set_to_policy").items():
                vpgroups = vset_to_vpgroups.setdefault(vset, [])

                try:
                    vpgroup = VersionedPackageGroup(
                        group_id=group_id,
                        oncall=oncall,
                        packages=packages,
                        evras=_load_policy_versions_for_packages(
                            packages, policy_cfg, policy_fn
                        ),
                    )
                    vpgroups.append(vpgroup)
                except Exception:
                    raise RuntimeError(
                        f"Loading policy {vset}: {policy_cfg} for {packages}"
                    )

                for pkg in packages:
                    prev = vset_to_pkg_to_vpgroup.setdefault(vset, {}).setdefault(
                        pkg, vpgroup
                    )
                    if prev is not vpgroup:
                        raise RuntimeError(
                            f"{pkg} was already added to this version set by "
                            f"another package group config: {prev.group_id}"
                        )

        except Exception:
            raise RuntimeError(f"Loading config {group_path}")
        assert (
            not group_cfg
        ), f"{group_path}: Bad package group config keys: {group_cfg}"

    return vset_to_vpgroups


def _resolve_envras_for_package_group(
    snapshot_db: sqlite3.Connection, vpgroup: VersionedPackageGroup
) -> Set[Tuple[str, str, str, str, str]]:
    """
    Resolve `None` (wildcard) epochs to concrete epochs using our RPM repo
    snapshot DBs.  This is necessary because `versionlock` plugin
    implementations require fully resolved RPM IDs.
    """
    # Our SQL query composition needs both to be nonempty.
    if not vpgroup.evras:
        return set()
    assert vpgroup.packages, vpgroup  # Empty groups should be removed earlier

    evra_subs = []
    evra_queries = []
    for evra in vpgroup.evras:
        if evra.epoch is None:
            maybe_epoch_and = ""
        else:
            maybe_epoch_and = "epoch = ? AND "
            evra_subs.append(evra.epoch)
        if evra.arch is None:
            maybe_arch_and = ""
        else:
            maybe_arch_and = "arch = ? AND "
            evra_subs.append(evra.arch)
        evra_queries.append(
            f"({maybe_epoch_and}{maybe_arch_and}version = ? AND release = ?)"
        )
        evra_subs.extend([evra.version, evra.release])

    n_to_vra_to_e = {}
    query_sql = (
        "SELECT name, epoch, version, release, arch FROM rpm WHERE "
        f"(name IN ({', '.join('?' for _ in vpgroup.packages)}))"
        f" AND ({' OR '.join(evra_queries)})"
    )
    # pyre-fixme[60]: Concatenation not yet support for multiple variadic
    #  tuples: `*vpgroup.packages, *evra_subs`.
    query_args = (*vpgroup.packages, *evra_subs)
    log.debug(f"Running SQL query {query_sql} with args {query_args}")
    for n, e, v, r, a in snapshot_db.execute(query_sql, query_args):
        old_e = n_to_vra_to_e.setdefault(n, {}).setdefault((v, r, a), e)
        # XXX make a diff error: epoch for NVRA couldn't be inferred uniquely
        assert old_e == e, (e, old_e, n, v, r, a)

    # Find EVRs that are valid for all packages in the group.
    #
    # We treat architecture separately from EVR. In the `systemd` group:
    #  - `systemd-libs` has EVRA (0, '246.1', '1.fb2', 'x86_64')
    #  - `systemd-rpm-macros` has EVRA (0, '246.1', '1.fb2', 'noarch')
    # These are compatible, even though they have different architectures.
    #
    # Rather than try to duplicate the package manager's complex architecture
    # selection logic, we just track "arch + package" sets for each EVR.
    #
    # This means that an EVR will accepted for a version set even if all the
    # packages present this EVR with different architectures.
    #
    # In my understanding of real-world usage of RPM architectures, this
    # should be OK, but if it's not, we may have to revisit this heuristic.
    evr_to_a_to_pkgs = None
    for pkg in vpgroup.packages:
        # Skip packages that does not exist in snapshot
        if pkg not in n_to_vra_to_e:
            continue
        cur_evr_to_a_to_pkgs = {
            (e, v, r): {a: {pkg}} for (v, r, a), e in n_to_vra_to_e.get(pkg, {}).items()
        }
        if evr_to_a_to_pkgs is None:
            evr_to_a_to_pkgs = cur_evr_to_a_to_pkgs
        else:
            del_evrs = []
            for evr, a_to_pkgs in evr_to_a_to_pkgs.items():
                cur_a_to_pkgs = cur_evr_to_a_to_pkgs.get(evr)
                if cur_a_to_pkgs is None:
                    del_evrs.append(evr)  # EVR doesn't work current package
                else:
                    for a, pkgs in cur_a_to_pkgs.items():
                        a_to_pkgs.setdefault(a, set()).update(pkgs)
            for evr in del_evrs:
                del evr_to_a_to_pkgs[evr]
        log.debug(
            f"After merging {pkg} with EVRAs {cur_evr_to_a_to_pkgs}, the "
            f"group was left with {evr_to_a_to_pkgs}."
        )
    if evr_to_a_to_pkgs is None:
        return set()
    # pyre-fixme[7]: Expected `Set[Tuple[str, str, str, str, str]]` but got
    #  `Set[SortableENVRA]`.
    return {
        SortableENVRA(e, n, v, r, a)
        for (e, v, r), a_to_pkgs in evr_to_a_to_pkgs.items()
        for a, pkgs in a_to_pkgs.items()
        for n in pkgs
    }


def _save_allowed_versions(
    vpgroups: Iterable[VersionedPackageGroup],
    rpm_snapshot_dirs: Iterable[Path],
    dest_dir: Path,
) -> None:
    os.makedirs(dest_dir, exist_ok=True)

    # FIXME: Next time this is refactored, use the DBs as context managers.
    snapshot_dbs = [readonly_snapshot_db(d) for d in rpm_snapshot_dirs]
    snapshot_paths_and_dbs = list(zip(rpm_snapshot_dirs, snapshot_dbs))

    # Default every known package to use "empty vset".
    with create_ro(dest_dir / _EMPTY_VSET_PATH, "w"):
        pass
    pkg_to_src_path = {}
    for db in snapshot_dbs:
        (pkgs,) = zip(*db.execute("select distinct name from rpm;").fetchall())
        for pkg in pkgs:
            pkg_to_src_path[pkg] = _EMPTY_VSET_PATH

    # Populate a vset file for each group; record the packages that use it.
    for vpgroup in vpgroups:
        # Doing epoch resolution independently for each snapshot means that
        # different snapshots can resolve to different epochs.
        envras = set()
        for snapshot_path, snapshot_db in snapshot_paths_and_dbs:
            log.debug(f"Resolving {vpgroup} via snapshot {snapshot_path}")
            envras |= _resolve_envras_for_package_group(snapshot_db, vpgroup)
        # XXX It can happen that the package group has no lock in any
        # snapshot -- make this a diff description error.
        #
        # Note that we're not asserting that **all** snapshots produce a
        # lock.  The reason is that in our usage certain groups can contain
        # snapshot-specific packages.  In this case, we expect only one
        # snapshot to produce locks.
        if not envras:
            log.error(f"XXX group has no lock in any snapshot {vpgroup}")
            continue

        os.makedirs(dest_dir / vpgroup.oncall, exist_ok=True)
        src_path = vpgroup.oncall + "/" + vpgroup.group_id
        with create_ro(dest_dir / src_path, "w") as outfile:
            # Sorted in the RPM update order for diff stability.
            for envra in sorted(envras):
                print(envra.to_versionlock_line(), file=outfile)

        for pkg in vpgroup.packages:
            # This can happen if a snapshot starts to omit a package
            # that previously needed version-pinning.  we shouldn't fail
            # hard, but just emit an error in the diff description.
            if pkg not in pkg_to_src_path:
                log.info(
                    f"The package {pkg} from vpgroup {vpgroup.group_id}"
                    " does not exist in snapshot"
                )
                continue
            assert pkg_to_src_path[pkg] == _EMPTY_VSET_PATH, pkg  # impossible
            pkg_to_src_path[pkg] = src_path

    # Output a Buck target for each package, pointing at the vset file that
    # it uses.  XXX OSS / FB: BUCK vs TARGETS
    with create_ro(dest_dir / "TARGETS", "w") as buckfile:
        _populate_vset_buck_file(buckfile, pkg_to_src_path)


def _populate_vset_buck_file(
    buckfile: TextIO, pkg_to_src_path: Mapping[str, str]
) -> None:
    buckfile.write(
        f"""\
# {'@'}generated via //antlir/rpm/allowed_versions:update-allowed-versions
#
# This file was generated automatically, in course of automatic update of RPM
# snapshot and build_appliance. You can see an RPM mentioned below even if you
# removed it from repos. If this is the case, please wait for the next update
# (no need to clean up it from this file manually).

load("//antlir/bzl:build_defs.bzl", "rpm_vset")

"""
    )

    for pkg, src_path in sorted(
        pkg_to_src_path.items(),
        # Put all the non-empty RPMs at the top, sort lexicographically
        # within each section.
        key=lambda pg: (pg[1] == _EMPTY_VSET_PATH, pg[0]),
    ):
        buckfile.write(
            # Let `src` to be defaulted here to reduce the output size.
            f"rpm_vset(name = {_drepr(pkg)})\n\n"
            if src_path == _EMPTY_VSET_PATH
            else textwrap.dedent(
                f"""\
                rpm_vset(
                    name = {_drepr(pkg)},
                    src = {_drepr(src_path)},
                )

                """
            )
        )


def update_allowed_versions(args: argparse.Namespace) -> None:
    plugin_driver = _PluginDriver(args.data_snapshot_dir, args.flavor)

    if args.update_data_snapshot:
        asyncio.run(plugin_driver.update_snapshots())

    with plugin_driver.prepare_load_config_fns() as load_config_fns:
        version_sets: Set[str] = {
            vs.decode()
            for vs in (
                os.listdir(args.version_sets_dir)
                if args.version_sets_dir.exists()
                else []
            )
        }
        vset_to_vpgroups = _load_version_sets(
            map(
                lambda x: Path(x),
                sum(
                    [glob.glob(dir / "*.json") for dir in args.package_groups_dir],
                    [],
                ),
            ),
            load_config_fns,
            version_sets,
        )
    log.info(f"XXXvsets {vset_to_vpgroups}")
    with populate_temp_dir_and_rename(args.version_sets_dir, overwrite=True) as td:
        for vset, vpgroups in vset_to_vpgroups.items():
            _save_allowed_versions(
                vpgroups=vpgroups,
                rpm_snapshot_dirs=args.rpm_repo_snapshot,
                dest_dir=td / vset / "rpm",
            )
        # XXX diff description should flag if not ALL of the listed packages
        # were locked, explain why and what to do.
        #
        # For the OSS version, we should just return the "changelog" here.
        #
        # XXX Then, in a separate, non-opensource script: generate diffs,
        # grabbing all changes per oncall across version sets.


def parse_args(argv: List[str]):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    # XXX default all these to ./<whatevs>, use this as common pattern for
    #  - fb prod
    #  - fb test (--no-update-data-snapshot with real but fixed snapshots,
    #    probably)
    #  - oss test (without --no-update-data-snapshot)
    parser.add_argument(
        "--data-snapshot-dir",
        required=True,
        type=Path.from_argparse,
        help="Base directory for per-plugin snapshot directories.",
    )
    parser.add_argument(
        "--package-groups-dir",
        required=True,
        action="append",
        type=Path.from_argparse,
        help="Directories with package group definitions. It can be helpful for"
        " each flavor to combine multiple sets of package groups, one per RPM "
        "universe (https://facebookincubator.github.io/antlir/docs/concepts/"
        "rpms/overview/). For example, one dir might contain OS-release "
        "specific groups, and another might have OS-independent packages.",
    )
    parser.add_argument(
        "--version-sets-dir",
        required=True,
        type=Path.from_argparse,
        help="XXX read for list, write to update",
    )
    parser.add_argument(
        "--rpm-repo-snapshot",
        required=True,
        action="append",
        type=Path.from_argparse,
        help="Path to `rpm_repo_snapshot` build output. Can be repeated if "
        "your allowable version sets need to span multiple snapshots.",
    )
    parser.add_argument(
        "--flavor",
        required=True,
        type=str,
        help="The flavor for version selection.",
    )
    parser.add_argument(
        "--no-update-data-snapshot",
        action="store_false",
        dest="update_data_snapshot",
        help="XXX for faster iteration",
    )
    parser.add_argument("--debug", action="store_true", help="Log more?")
    return Path.parse_args(parser, argv)


# XXX: Do the source-only package group source
if __name__ == "__main__":  # pragma: no cover
    args = parse_args(sys.argv[1:])
    init_logging(debug=args.debug)
    update_allowed_versions(args)
