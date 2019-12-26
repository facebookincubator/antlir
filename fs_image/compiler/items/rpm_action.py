#!/usr/bin/env python3
import base64
import enum
import shlex
import sys
import uuid

from typing import Iterable, List, Mapping, NamedTuple, Tuple, Union

from fs_image.fs_utils import Path
from nspawn_in_subvol import nspawn_in_subvol, \
    parse_opts as nspawn_in_subvol_parse_opts
from rpm.rpm_metadata import RpmMetadata, compare_rpm_versions
from subvol_utils import Subvol

from .common import ImageItem, LayerOpts, PhaseOrder, protected_path_set


class RpmAction(enum.Enum):
    install = 'install'
    # It would be sensible to have a 'remove' that fails if the package is
    # not already installed, but `yum` doesn't seem to support that, and
    # implementing it manually is a hassle.
    remove_if_exists = 'remove_if_exists'


# The values are valid `yum` / `dnf` commands.
class YumDnfCommand(enum.Enum):
    # We do NOT want people specifying package versions, releases, or
    # architectures via `image_feature`s.  That would be a sure-fire way to
    # get version conflicts.  For the cases where we need version pinning,
    # we'll add a per-layer "version picker" concept.
    install_name = 'install-n'
    # Unfortunately, `localinstall` is deprecated, so we have to take the
    # (small) risk that `yum` / `dnf` will misinterpret our path.  We cannot
    # pun `install-n` because `dnf` **only** accepts names with it.
    local_install = 'install'
    # `yum` will refuse to `install` a local package if it's a downgrade.
    local_downgrade = 'downgrade'
    # The way `yum` works, this is a no-op if the package is missing.
    remove_name_if_exists = 'remove-n'


# When several of the commands land in the same phase, we need to order them
# deterministically.  This is meant to be temporary, until this code can be
# re-tooled to use `yum/dnf shell` to run all operations in one transaction.
YUM_DNF_COMMAND_ORDER = {cmd: i for i, cmd in enumerate([
    # There's only one remove command, for now.
    YumDnfCommand.remove_name_if_exists,
    # This ordering of the install commands seems the least bad; TBD.  The
    # main concern is about us failing to do what the user asked because
    # `yum` / `dnf` overrode them due to the dependencies of packages
    # installed by the later commands.
    YumDnfCommand.local_downgrade,
    YumDnfCommand.local_install,
    YumDnfCommand.install_name,
])}

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
            raise RuntimeError(
                f'RPM action conflict for {rpm_name}: {actions}'
            )


class _LocalRpm(NamedTuple):
    path: Path
    metadata: RpmMetadata


def _get_action_to_names_or_rpms(
    items: Iterable['RpmActionItem']
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
    return action_to_names_or_rpms


def _action_to_command(
    subvol: Subvol, action: RpmAction, nor: Union[str, _LocalRpm],
) -> Tuple[YumDnfCommand, Union[str, _LocalRpm]]:
    # Vanilla RPM name?
    if not isinstance(nor, _LocalRpm):
        return ACTION_TO_DEFAULT_CMD[action], nor
    # Local RPM?
    if action == RpmAction.install:
        try:
            old = RpmMetadata.from_subvol(subvol, nor.metadata.name)
        except (RuntimeError, ValueError):
            # This can happen if the RPM DB does not exist in the
            # subvolume or the package is not installed.
            old = None
        if old is not None and compare_rpm_versions(nor.metadata, old) <= 0:
            return YumDnfCommand.local_downgrade, nor
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
    return None, None  # pragma: no cover


def _convert_actions_to_commands(
    subvol: Subvol,
    action_to_names_or_rpms: Mapping[RpmAction, Union[str, _LocalRpm]],
) -> Mapping[YumDnfCommand, Union[str, _LocalRpm]]:
    '''
    Go through the list of RPMs to install and change the action to
    downgrade if it is a local RPM with a lower version than what is
    installed.

    Also use `local_install` and `local_remove` for _LocalRpm.

    See the docs in `YumDnfCommand` for the rationale.
    '''
    cmd_to_names_or_rpms = {}
    for action, names_or_rpms in action_to_names_or_rpms.items():
        for nor in names_or_rpms:
            cmd, new_nor = _action_to_command(subvol, action, nor)
            if cmd is None:  # pragma: no cover
                raise AssertionError(f'Unsupported {action}, {nor}')
            cmd_to_names_or_rpms.setdefault(cmd, set()).add(new_nor)
    return cmd_to_names_or_rpms


def _rpms_and_bind_ro_args(
    names_or_rpms: List[Union[str, _LocalRpm]],
) -> Tuple[List[str], List[str]]:
    rpms = []
    bind_ro_args = []
    for idx, nor in enumerate(names_or_rpms):
        if isinstance(nor, _LocalRpm):
            # For custom bind mount destinations, nspawn is strict on
            # destinations where the parent directories don't exist.
            # Because of that, we bind all the local RPMs in "/" with
            # uniquely prefix-ed names.
            dest = f'/localhostrpm_{idx}_{nor.path.basename().decode()}'
            bind_ro_args.extend(['--bindmount-ro', nor.path.decode(), dest])
            rpms.append(dest)
        else:
            rpms.append(nor)
    return rpms, bind_ro_args


# These items are part of a phase, so they don't get dependency-sorted, so
# there is no `requires()` or `provides()` or `build()` method.
class RpmActionItem(metaclass=ImageItem):
    fields = [
        ('name', None),
        ('source', None),
        'action',
    ]

    def customize_fields(kwargs):  # noqa: B902
        assert (kwargs.get('name') is None) ^ (kwargs.get('source') is None), \
            f'Exactly one of `name` or `source` must be set in {kwargs}'
        kwargs['action'] = RpmAction(kwargs['action'])

    def phase_order(self):
        return {
            RpmAction.install: PhaseOrder.RPM_INSTALL,
            RpmAction.remove_if_exists: PhaseOrder.RPM_REMOVE,
        }[self.action]

    @classmethod
    def get_phase_builder(
        cls, items: Iterable['RpmActionItem'], layer_opts: LayerOpts,
    ):
        # Do as much validation as possible outside of the builder to give
        # fast feedback to the user.
        assert (layer_opts.build_appliance is not None), (
            f'`image_layer` {layer_opts.layer_target} must set '
            '`build_appliance`'
        )

        # This Mapping[RpmAction, Union[str, _LocalRpm]] powers builder() below.
        action_to_names_or_rpms = _get_action_to_names_or_rpms(items)

        def builder(subvol: Subvol) -> None:
            # Convert porcelain RpmAction to plumbing YumDnfCommands.  This
            # is done in the builder because we need access to the subvol.
            #
            # Sort by command for determinism and (hopefully) better behaivor.
            for cmd, nors in sorted(_convert_actions_to_commands(
                subvol, action_to_names_or_rpms,
            ).items(), key=lambda cn: YUM_DNF_COMMAND_ORDER[cn[0]]):
                rpms, bind_ro_args = _rpms_and_bind_ro_args(nors)
                _yum_dnf_using_build_appliance(
                    build_appliance=Subvol(
                        layer_opts.build_appliance, already_exists=True,
                    ),
                    nspawn_args=bind_ro_args,
                    install_root=subvol.path(),
                    protected_paths=protected_path_set(subvol),
                    yum_dnf_args=[
                        cmd.value,
                        '--assumeyes',
                        # Sort ensures determinism even if `yum` or `dnf` is
                        # order-dependent
                        *sorted(rpms),
                    ],
                    layer_opts=layer_opts,
                )
        return builder


def _yum_dnf_using_build_appliance(
    *, build_appliance: Subvol,
    nspawn_args: List[str],
    install_root: Path,
    protected_paths: Iterable[str],
    yum_dnf_args: List[str],
    layer_opts: LayerOpts,
) -> None:
    work_dir = '/work' + base64.urlsafe_b64encode(
        uuid.uuid4().bytes  # base64 instead of hex saves 10 bytes
    ).decode().strip('=')
    prog_name = 'yum'  # `dnf` support coming soon
    mount_cache = '' if layer_opts.preserve_yum_dnf_cache else f'''
        mkdir -p {work_dir}/var/cache/{prog_name} ; \
        mount --bind /var/cache/{prog_name} {work_dir}/var/cache/{prog_name} ;
    '''
    opts = nspawn_in_subvol_parse_opts([
        '--layer', 'UNUSED',
        '--user', 'root',
        # You can see below --no-private-network in conjunction with
        # --cap-net-admin.  CAP_NET_ADMIN is not intended to administer the
        # host's network stack.  See how yum_dnf_from_snapshot() brings
        # loopback interface up under protection of "unshare --net".
        '--cap-net-admin',
        '--no-private-network',
        '--bindmount-rw', install_root.decode(), work_dir,
        *nspawn_args,
        '--', 'sh', '-uec',
        f'''
        {mount_cache}
        /rpm-repo-snapshot/default/bin/{prog_name} \
            {' '.join(
                '--protected-path=' + shlex.quote(p) for p in protected_paths
            )} \
            --install-root {work_dir} \
            -- {' '.join(shlex.quote(arg) for arg in yum_dnf_args)}
        ''',
    ])
    # Don't pollute stdout with logging
    nspawn_in_subvol(build_appliance, opts, stdout=sys.stderr)
