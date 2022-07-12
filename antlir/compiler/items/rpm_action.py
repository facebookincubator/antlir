#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import enum
import pwd
import shlex
import tempfile
from contextlib import contextmanager
from typing import (
    Any,
    Dict,
    Iterable,
    List,
    Mapping,
    NamedTuple,
    Optional,
    Tuple,
    Union,
)

from antlir.bzl.image.feature.rpms import rpm_action_item_t
from antlir.bzl_const import BZL_CONST
from antlir.common import get_logger, not_none
from antlir.config import repo_config
from antlir.fs_utils import generate_work_dir, Path
from antlir.nspawn_in_subvol.args import (
    _new_nspawn_debug_only_not_for_prod_opts,
    new_nspawn_opts,
    PopenArgs,
)
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.nspawn_in_subvol.plugins.yum_dnf_versionlock import (
    YumDnfVersionlock,
)
from antlir.rpm.rpm_metadata import compare_rpm_versions, RpmMetadata
from antlir.subvol_utils import Subvol
from pydantic import root_validator

from .common import ImageItem, LayerOpts, PhaseOrder, protected_path_set


log = get_logger()


class RpmAction(enum.Enum):
    install = "install"
    # It would be sensible to have a 'remove' that fails if the package is
    # not already installed, but `yum` doesn't seem to support that, and
    # implementing it manually is a hassle.
    remove_if_exists = "remove_if_exists"


# The values are valid `yum` / `dnf` commands.
class YumDnfCommand(enum.Enum):
    # We do NOT want people specifying package versions, releases, or
    # architectures via `feature`s.  That would be a sure-fire way to
    # get version conflicts.  For the cases where we need version pinning,
    # we'll add a per-layer "version picker" concept.
    install_name = "install-n"
    # Unfortunately, `localinstall` is deprecated, so we have to take the
    # (small) risk that `yum` / `dnf` will misinterpret our path.  We cannot
    # pun `install-n` because `dnf` **only** accepts names with it.
    local_install = "install"
    # `yum` will refuse to `install` a local package if it's a downgrade.
    local_downgrade = "downgrade"
    # The way `yum` works, this is a no-op if the package is missing.
    remove_name_if_exists = "remove-n"
    # Yum will refuse to re-install a package that is already installed, so
    # allow some actions that do nothing
    noop = "noop"


# When several of the commands land in the same phase, we need to order them
# deterministically.  This is meant to be temporary, until this code can be
# re-tooled to use `yum/dnf shell` to run all operations in one transaction.
YUM_DNF_COMMAND_ORDER = {
    cmd: i
    for i, cmd in enumerate(
        [
            # There's only one remove command, for now.
            YumDnfCommand.remove_name_if_exists,
            # This ordering of the install commands seems the least bad; TBD.
            # The main concern is about us failing to do what the user asked
            # because `yum` / `dnf` overrode them due to the dependencies of
            # packages installed by the later commands.
            YumDnfCommand.local_downgrade,
            YumDnfCommand.local_install,
            YumDnfCommand.install_name,
            YumDnfCommand.noop,
        ]
    )
}

assert len(YUM_DNF_COMMAND_ORDER) == len(YumDnfCommand)


# The actual resolution is more complicated, see `_action_to_command()`
ACTION_TO_DEFAULT_CMD = {
    RpmAction.install: YumDnfCommand.install_name,
    RpmAction.remove_if_exists: YumDnfCommand.remove_name_if_exists,
}


class _RpmActionConflictDetector:
    def __init__(self):
        self.name_to_actions = {}

    def add(self, rpm_name, item):
        actions = self.name_to_actions.setdefault(rpm_name, [])
        actions.append((item.action, item.from_target))
        # Raise when a layer has multiple actions for one RPM -- even
        # when all actions are the same.  This can be relaxed if needed.
        if len(actions) != 1:
            raise RuntimeError(f"RPM action conflict for {rpm_name}: {actions}")


class _LocalRpm(NamedTuple):
    path: Path
    metadata: RpmMetadata


def _get_action_to_names_or_rpms(
    items: Iterable["RpmActionItem"],
) -> Mapping[RpmAction, Union[str, _LocalRpm]]:
    conflict_detector = _RpmActionConflictDetector()
    action_to_names_or_rpms = {action: set() for action in RpmAction}
    for item in items:
        assert isinstance(item, RpmActionItem), item

        # Eagerly resolve paths & metadata for local RPMs to avoid
        # repeating the required costly IO (or bug-prone implicit
        # memoization).
        if item.source is not None:
            rpm_path = item.source
            name_or_rpm = _LocalRpm(
                path=rpm_path,
                metadata=RpmMetadata.from_file(rpm_path),
            )
            conflict_detector.add(name_or_rpm.metadata.name, item)
        else:
            name_or_rpm = item.name
            conflict_detector.add(item.name, item)

        action_to_names_or_rpms[item.action].add(name_or_rpm)
    # pyre-fixme[7]: Expected `Mapping[RpmAction, Union[_LocalRpm, str]]` but
    # got `Dict[RpmAction, typing.Set[typing.Any]]`.
    return action_to_names_or_rpms


def _action_to_command(
    subvol: Subvol,
    build_appliance: Subvol,
    action: RpmAction,
    nor: Union[str, _LocalRpm],
) -> Tuple[YumDnfCommand, Union[str, _LocalRpm]]:
    # Vanilla RPM name?
    if not isinstance(nor, _LocalRpm):
        return ACTION_TO_DEFAULT_CMD[action], nor
    # Local RPM?
    if action == RpmAction.install:
        try:
            old = RpmMetadata.from_subvol(
                subvol, build_appliance, nor.metadata.name
            )
        except (RuntimeError, ValueError) as ex:
            # This can happen if the RPM DB does not exist in the
            # subvolume or the package is not installed.
            old = None
            # Log the error since this can also mask real problems.
            log.debug(f"Did not find {nor.metadata.name} in image: {ex}")
        if old is not None and compare_rpm_versions(nor.metadata, old) < 0:
            return YumDnfCommand.local_downgrade, nor
        if old is not None and compare_rpm_versions(nor.metadata, old) == 0:
            return YumDnfCommand.noop, nor
        else:
            return YumDnfCommand.local_install, nor
    elif action == RpmAction.remove_if_exists:
        # This is a bit of an edge-case but we support it because this
        # means that image declarations don't need to redundantly know
        # the RPM name for a "local RPM" target.
        #
        # We need to resolve the RPM target path to a name here because `yum
        # remove` does not accept RPM paths.
        return YumDnfCommand.remove_name_if_exists, nor.metadata.name
    # Bad RpmAction?
    # pyre-fixme[7]: Expected `Tuple[YumDnfCommand, Union[_LocalRpm, str]]` but
    # got `Tuple[None, None]`.
    return None, None  # pragma: no cover


def _convert_actions_to_commands(
    subvol: Subvol,
    build_appliance: Subvol,
    action_to_names_or_rpms: Mapping[RpmAction, Union[str, _LocalRpm]],
) -> Mapping[YumDnfCommand, Union[str, _LocalRpm]]:
    """
    Go through the list of RPMs to install and change the action to
    downgrade if it is a local RPM with a lower version than what is
    installed.

    Also use `local_install` and `local_remove` for _LocalRpm.

    See the docs in `YumDnfCommand` for the rationale.
    """
    cmd_to_names_or_rpms = {}
    for action, names_or_rpms in action_to_names_or_rpms.items():
        for nor in names_or_rpms:
            cmd, new_nor = _action_to_command(
                subvol, build_appliance, action, nor
            )
            if cmd == YumDnfCommand.noop:
                continue
            if cmd is None:  # pragma: no cover
                raise AssertionError(f"Unsupported {action}, {nor}")
            cmd_to_names_or_rpms.setdefault(cmd, set()).add(new_nor)
    return cmd_to_names_or_rpms


def _rpms_and_bind_ros(
    names_or_rpms: List[Union[str, _LocalRpm]]
) -> Tuple[List[str], List[str]]:
    rpms = []
    bind_ros = []
    for idx, nor in enumerate(names_or_rpms):
        if isinstance(nor, _LocalRpm):
            # For custom bind mount destinations, nspawn is strict on
            # destinations where the parent directories don't exist.
            # Because of that, we bind all the local RPMs in "/" with
            # uniquely prefix-ed names.
            dest = f"/localhostrpm_{idx}_{nor.path.basename()}"
            bind_ros.append((nor.path, dest))
            rpms.append(dest)
        else:
            rpms.append(nor)
    return rpms, bind_ros


@contextmanager
def _prepare_versionlock(
    version_sets: Iterable[Path], version_set_override: Optional[str]
) -> Path:
    with tempfile.NamedTemporaryFile() as outfile:
        # Calculate overridden_rpm_names -- the names of rpms listed in
        # `version_set_override`. Copy this file to `outfile`.
        overridden_rpm_names = set()
        if version_set_override:
            with open(version_set_override, "rb") as infile:
                for l in infile:
                    overridden_rpm_names.add(l.split()[1])
                    outfile.write(l)
                    if not l.endswith(b"\n"):
                        outfile.write(b"\n")
        # Blindly concatenate the files in the supplied paths dropping lines
        # with rpm names defined by `overridden_rpm_names`; some modest
        # error-checking will happen in `yum_dnf_versionlock.py`.
        for vs_path in version_sets:
            with open(vs_path, "rb") as infile:
                for l in infile:
                    if not l.split()[1] in overridden_rpm_names:
                        outfile.write(l)
                        if not l.endswith(b"\n"):
                            outfile.write(b"\n")
        outfile.flush()
        # pyre-fixme[7]: Expected `Path` but got `Generator[str, None, None]`.
        yield outfile.name


# These items are part of a phase, so they don't get dependency-sorted, so
# there is no `requires()` or `provides()` or `build()` method.
# pyre-fixme[13]: Attribute `action` is never initialized.
class RpmActionItem(rpm_action_item_t, ImageItem):
    # pyre-fixme[15]: `action` overrides attribute defined in
    # `rpm_action_item_t` inconsistently.
    action: RpmAction
    flavor_to_version_set: Dict[str, Union[str, Dict[str, str]]]
    flavors_specified: bool = False
    name: Optional[str] = None
    source: Optional[Path] = None

    def __init__(self, *args: Any, **kwargs: Any):
        rpm_action_item_t.__init__(self, *args, **kwargs)
        ImageItem.__init__(self, from_target=kwargs.get("from_target"))

    @root_validator
    def check_name_or_source_exclusive(
        cls, values: Mapping[str, Any]
    ) -> Mapping[str, Any]:  # noqa B902
        assert (values.get("name") is None) ^ (
            values.get("source") is None
        ), f"Exactly one of `name` or `source` must be set in {values}"
        return values

    def phase_order(self):
        return {
            RpmAction.install: PhaseOrder.RPM_INSTALL,
            RpmAction.remove_if_exists: PhaseOrder.RPM_REMOVE,
        }[self.action]

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["RpmActionItem"], layer_opts: LayerOpts
    ):
        # THIS IGNORES RPMS THAT DON'T HAVE A MATCHING FLAVOR
        # IN LAYER_OPTS. WE NEED THIS TO ENABLE CROSS FLAVOR MIGRATIONS.
        matching_flavor_items = []
        for item in items or []:
            if layer_opts.flavor in item.flavor_to_version_set:
                matching_flavor_items.append(item)
            elif (
                not item.flavors_specified
                and layer_opts.flavor not in repo_config().stable_flavors
            ):
                unspecified_rpms = ",".join(
                    [
                        f"{{{item}}}"
                        for item in items
                        if not item.flavors_specified
                    ]
                )
                raise RuntimeError(
                    "You must specify the flavor on rpms "
                    f"`{unspecified_rpms}` as "
                    f"your image `{layer_opts.layer_target}` "
                    f"has flavor `{layer_opts.flavor}` which "
                    "is not a stable flavor."
                )
            else:
                log.info(
                    f"Rpm {item.name} does not match flavor "
                    f"{layer_opts.flavor}. Skipping..."
                )
        items = matching_flavor_items

        # Do as much validation as possible outside of the builder to give
        # fast feedback to the user.
        build_appliance = layer_opts.requires_build_appliance()

        # This Mapping[RpmAction, Union[str, _LocalRpm]] powers builder() below.
        action_to_names_or_rpms = _get_action_to_names_or_rpms(items)

        # Future: when we add per-layer version set overrides, they will
        # need apply on top of the repo-wide version set we are using.
        version_sets = set()
        for item in items:
            version_sets.update(
                [
                    Path(version_set)
                    for flavor, version_set in (
                        item.flavor_to_version_set.items()
                    )
                    if flavor == layer_opts.flavor
                    and version_set != BZL_CONST.version_set_allow_all_versions
                ]
            )

        def builder(subvol: Subvol) -> None:
            # pyre-fixme[16]: `Path` has no attribute `__enter__`.
            with _prepare_versionlock(
                version_sets, layer_opts.version_set_override
            ) as versionlock_path:
                # Convert porcelain RpmAction to plumbing YumDnfCommands.  This
                # is done in the builder because we need access to the subvol.
                #
                # Sort by command for determinism and clearer behaivor.
                for cmd, nors in sorted(
                    _convert_actions_to_commands(
                        subvol, build_appliance, action_to_names_or_rpms
                    ).items(),
                    key=lambda cn: YUM_DNF_COMMAND_ORDER[cn[0]],
                ):
                    # pyre-fixme[6]: Expected `List[Union[_LocalRpm, str]]` for
                    #  1st param but got `Union[_LocalRpm, str]`.
                    rpms, bind_ros = _rpms_and_bind_ros(nors)
                    _yum_dnf_using_build_appliance(
                        build_appliance=build_appliance,
                        # pyre-fixme[6]: Expected `List[Tuple[str, str]]` for
                        #  2nd param but got `List[str]`.
                        bind_ros=bind_ros,
                        install_root=subvol.path(),
                        protected_paths=protected_path_set(subvol),
                        versionlock_list=versionlock_path,
                        yum_dnf_args=[
                            cmd.value,
                            "--assumeyes",
                            # Sort ensures determinism even if `yum` or
                            # `dnf` is order-dependent
                            *sorted(rpms),
                        ],
                        layer_opts=layer_opts,
                    )

        return builder


def _yum_dnf_using_build_appliance(
    *,
    build_appliance: Subvol,
    bind_ros: List[Tuple[str, str]],
    install_root: Path,
    protected_paths: Iterable[Path],
    versionlock_list: Path,
    yum_dnf_args: List[str],
    layer_opts: LayerOpts,
) -> None:
    work_dir = generate_work_dir()
    prog_name = not_none(layer_opts.rpm_installer).value
    snapshot_dir = not_none(layer_opts.rpm_repo_snapshot)
    opts = new_nspawn_opts(
        cmd=[
            "sh",
            "-uec",
            f"""\
            {
                (snapshot_dir / 'yum-dnf-from-snapshot').shell_quote()
            } \
            --snapshot-dir={snapshot_dir} \
            {
                shlex.quote(prog_name)
            } {
                ' '.join(
                    '--protected-path=' + p.shell_quote()
                        for p in protected_paths
                )
            } {
                '--debug' if layer_opts.debug else ''
            } \
            -- \
            --installroot={work_dir.decode()} {
                ' '.join(shlex.quote(arg) for arg in yum_dnf_args)
            }
            """,
        ],
        layer=build_appliance,
        bindmount_ro=bind_ros,
        bindmount_rw=[(install_root, work_dir)],
        user=pwd.getpwnam("root"),
        debug_only_opts=_new_nspawn_debug_only_not_for_prod_opts(
            # This needs to be set to public so that the rpm repo server
            # launched by the outer BA container is reachable from the
            # below nspawn.
            private_network=False,
        ),
    )
    run_nspawn(
        opts,
        PopenArgs(),
        plugins=[
            YumDnfVersionlock(
                [(snapshot_dir, versionlock_list)],
                [snapshot_dir],
            )
        ],
    )
