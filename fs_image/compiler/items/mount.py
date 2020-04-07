#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
NB: Surprisingly, we don't need any special cleanup for the `mount` operations
    performed by `build` and `clone_mounts` -- it appears that subvolume
    deletion, as performed by `subvolume_garbage_collector.py`, implicitly
    lazy-unmounts any mounts therein.
'''
import json
import os

from dataclasses import dataclass
from typing import Mapping, NamedTuple

from subvol_utils import Subvol
from find_built_subvol import find_built_subvol

from fs_image.compiler import procfs_serde
from fs_image.compiler.requires_provides import (
    ProvidesDoNotAccess, require_directory
)

from .common import coerce_path_field_normal_relative, ImageItem, LayerOpts
from .mount_utils import META_MOUNTS_DIR, MOUNT_MARKER, ro_rbind_mount


class _BuildSource(NamedTuple):
    type: str
    # This is overloaded to mean different things depending on `type`.
    source: str

    def to_path(
        self, *, target_to_path: Mapping[str, str], subvolumes_dir: str,
    ) -> str:
        if self.type == 'layer':
            out_path = target_to_path.get(self.source)
            if out_path is None:
                raise AssertionError(
                    f'MountItem could not resolve {self.source}'
                )
            subvol = find_built_subvol(out_path, subvolumes_dir=subvolumes_dir)
            # If we allowed mounting a layer that has other mounts inside,
            # it would force us to support nested mounts.  We don't want to
            # do this (yet).
            if os.path.exists(subvol.path(META_MOUNTS_DIR)):
                raise AssertionError(
                    f'Refusing to mount {subvol.path()} since that would '
                    'require the tooling to support nested mounts.'
                )
            return subvol.path()
        elif self.type == 'host':
            return self.source
        else:  # pragma: no cover
            raise AssertionError(
                f'Bad mount source "{self.type}" for {self.source}'
            )


@dataclass(init=False, frozen=True)
class MountItem(ImageItem):
    mountpoint: str
    build_source: _BuildSource
    runtime_source: str
    is_directory: bool

    @classmethod
    def customize_fields(cls, kwargs):
        layer_opts = kwargs.pop('layer_opts', None)
        target = kwargs.pop('target')
        cfg = kwargs.pop('mount_config')
        assert (target is None) ^ (cfg is None), \
            f'Exactly one of `target` or `mount_config` must be set in {kwargs}'
        if cfg is not None:
            cfg = cfg.copy()  # We must not mutate our input!
        else:
            with open(os.path.join(target, 'mountconfig.json')) as f:
                cfg = json.load(f)

        default_mountpoint = cfg.pop('default_mountpoint', None)
        if kwargs.get('mountpoint') is None:  # Missing or None => use default
            kwargs['mountpoint'] = default_mountpoint
            if kwargs['mountpoint'] is None:
                raise AssertionError(f'MountItem {kwargs} lacks mountpoint')
        coerce_path_field_normal_relative(kwargs, 'mountpoint')

        kwargs['is_directory'] = cfg.pop('is_directory')

        kwargs['build_source'] = _BuildSource(**cfg.pop('build_source'))
        if kwargs['build_source'].type == 'host' and not (
            kwargs['from_target'] in layer_opts.allowed_host_mount_targets
            or kwargs['from_target'].startswith('//fs_image/compiler/test')
        ):
            raise AssertionError(
                'Host mounts cause containers to be non-hermetic and '
                'fragile, so they must be located under one of '
                f'{layer_opts.allowed_host_mount_targets} '
                'to enable close review by the owners of `fs_image`.'
            )

        # This is supposed to be the run-time equivalent of `build_source`,
        # but for us it's just an opaque JSON blob that the runtime wants.
        # Hack: We serialize this back to JSON since the compiler expects
        # items to be hashable, and the source WILL contain dicts.
        runtime_source = cfg.pop('runtime_source', None)
        # Future: once runtime_source grows a schema, use it here?
        if (runtime_source and runtime_source.get('type') == 'host'):
            raise AssertionError(
                f'Only `build_source` may specify host mounts: {kwargs}'
            )
        kwargs['runtime_source'] = json.dumps(runtime_source, sort_keys=True)

        assert cfg == {}, f'Unparsed fields in {kwargs} mount_config: {cfg}'

    def provides(self):
        # For now, nesting of mounts is not supported, and we certainly
        # cannot allow regular items to write inside a mount.
        yield ProvidesDoNotAccess(path=self.mountpoint)

    def requires(self):
        # We don't require the mountpoint itself since it will be shadowed,
        # so this item just makes it with default permissions.
        yield require_directory(os.path.dirname(self.mountpoint))

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        mount_dir = os.path.join(META_MOUNTS_DIR, self.mountpoint, MOUNT_MARKER)
        for name, data in (
            # NB: Not exporting self.mountpoint since it's implicit in the path.
            ('is_directory', self.is_directory),
            ('build_source', self.build_source._asdict()),
            ('runtime_source', json.loads(self.runtime_source)),
        ):
            procfs_serde.serialize(data, subvol, os.path.join(mount_dir, name))
        source_path = self.build_source.to_path(
            target_to_path=layer_opts.target_to_path,
            subvolumes_dir=layer_opts.subvolumes_dir,
        )
        # Support mounting directories and non-directories...  This check
        # follows symlinks for the mount source, which seems correct.
        is_dir = os.path.isdir(source_path)
        assert is_dir == self.is_directory, self
        if is_dir:
            subvol.run_as_root([
                'mkdir', '--mode=0755', subvol.path(self.mountpoint),
            ])
        else:  # Regular files, device nodes, FIFOs, you name it.
            # `touch` lacks a `--mode` argument, but the mode of this
            # mountpoint will be shadowed anyway, so let it be whatever.
            subvol.run_as_root(['touch', subvol.path(self.mountpoint)])
        ro_rbind_mount(source_path, subvol, self.mountpoint)
