#!/usr/bin/env python3
import base64
import enum
import shlex
import sys
import uuid

from typing import Iterable, List, NamedTuple, Tuple, Union

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
    downgrade = 'downgrade'


RPM_ACTION_TYPE_TO_YUM_CMD = {
    # We do NOT want people specifying package versions, releases, or
    # architectures via `image_feature`s.  That would be a sure-fire way to
    # get version conflicts.  For the cases where we need version pinning,
    # we'll add a per-layer "version picker" concept.
    RpmAction.install: 'install-n',
    # The way `yum` works, this is a no-op if the package is missing.
    RpmAction.remove_if_exists: 'remove-n',
    RpmAction.downgrade: 'downgrade',
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
        assert kwargs['action'] != RpmAction.downgrade, \
            '\'downgrade\' cannot be passed'

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

        conflict_detector = _RpmActionConflictDetector()

        # This Map[RpmAction, Union[str, _LocalRpm]] powers builder() below.
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

        def builder(subvol: Subvol) -> None:
            # Go through the list of RPMs to install and change the action to
            # downgrade if it is a local RPM with a lower version than what is
            # installed.
            # This is done in the builder because we need access to the subvol.
            for nor in action_to_names_or_rpms[RpmAction.install].copy():
                if isinstance(nor, _LocalRpm):
                    try:
                        old = RpmMetadata.from_subvol(subvol, nor.metadata.name)
                    except (RuntimeError, ValueError):
                        # This can happen if the RPM DB does not exist in the
                        # subvolume or the package is not installed.
                        continue
                    if compare_rpm_versions(nor.metadata, old) <= 0:
                        action_to_names_or_rpms[RpmAction.install].remove(nor)
                        action_to_names_or_rpms[RpmAction.downgrade].add(nor)

            for action, nors in action_to_names_or_rpms.items():
                if not nors:
                    continue

                rpms, bind_ro_args = _rpms_and_bind_ro_args(nors)
                _yum_using_build_appliance(
                    build_appliance=Subvol(
                        layer_opts.build_appliance, already_exists=True,
                    ),
                    nspawn_args=bind_ro_args,
                    install_root=subvol.path(),
                    protected_paths=protected_path_set(subvol),
                    yum_args=[
                        RPM_ACTION_TYPE_TO_YUM_CMD[action],
                        '--assumeyes',
                        # Sort ensures determinism even if `yum` is
                        # order-dependent
                        *sorted(rpms),
                    ],
                    preserve_yum_cache=layer_opts.preserve_yum_cache,
                )
        return builder


def _yum_using_build_appliance(
    *, build_appliance: Subvol,
    nspawn_args: List[str],
    install_root: Path,
    protected_paths: Iterable[str],
    yum_args: List[str],
    preserve_yum_cache: bool,
) -> None:
    work_dir = '/work' + base64.urlsafe_b64encode(
        uuid.uuid4().bytes  # base64 instead of hex saves 10 bytes
    ).decode().strip('=')
    mount_var_cache_yum = '' if preserve_yum_cache else f'''
                         mkdir -p {work_dir}/var/cache/yum ; \
                         mount --bind /var/cache/yum {work_dir}/var/cache/yum ;
                         '''
    opts = nspawn_in_subvol_parse_opts([
        '--layer', 'UNUSED',
        '--user', 'root',
        # You can see below --no-private-network in conjunction with
        # --cap-net-admin.  CAP_NET_ADMIN is not intended to administer the
        # host's network stack.  See how yum_from_snapshot() brings loopback
        # interface up under protection of "unshare --net".
        '--cap-net-admin',
        '--no-private-network',
        '--bindmount-rw', install_root.decode(), work_dir,
        *nspawn_args,
        '--', 'sh', '-uec',
        f'''
        {mount_var_cache_yum}
        /rpm-repo-snapshot/default/bin/yum \
            {' '.join(
                '--protected-path=' + shlex.quote(p) for p in protected_paths
            )} \
            --install-root {work_dir} \
            -- {' '.join(shlex.quote(arg) for arg in yum_args)}
        ''',
    ])
    # Don't pollute stdout with logging
    nspawn_in_subvol(build_appliance, opts, stdout=sys.stderr)
