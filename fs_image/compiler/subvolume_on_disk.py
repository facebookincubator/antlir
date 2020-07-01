#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'See the SubvolumeOnDisk docblock.'
import json
import logging
import os
import socket
import subprocess

from collections import namedtuple

log = logging.Logger(__name__)

# These constants can represent both JSON keys for
# serialization/deserialization, and namedtuple keys.  Legend:
#  (1) Field in the namedtuple SubvolumeOnDisk
#  (2) Written into the on-disk dictionary format
#  (3) Read from the on-disk dictionary format
_BTRFS_UUID = 'btrfs_uuid'  # (1-3)
_BTRFS_PARENT_UUID = 'btrfs_parent_uuid'  # (1)
_HOSTNAME = 'hostname'  # (1-3)
_SUBVOLUMES_BASE_DIR = 'subvolumes_base_dir'  # (1)
_SUBVOLUME_REL_PATH = 'subvolume_rel_path'  # (1-3)
_DANGER = 'DANGER'  # (2)


def _btrfs_get_volume_props(subvolume_path):
    SNAPSHOTS = 'Snapshot(s)'
    props = {}
    # It's unfair to assume that the OS encoding is UTF-8, but our JSON
    # serialization kind of requires it, and Python3 makes it hyper-annoying
    # to work with bytestrings, so **shrug**.
    #
    # If this turns out to be a problem for a practical use case, we can add
    # `surrogateescape` all over the place, or even set
    # `PYTHONIOENCODING=utf-8:surrogateescape` in the environment.
    for l in subprocess.check_output([
        'sudo', 'btrfs', 'subvolume', 'show', subvolume_path,
    ]).decode('utf-8').split('\n')[1:]:  # Skip the header line
        if SNAPSHOTS in props:
            if l:  # Ignore the trailing empty line
                TABS = 4
                assert l[:TABS] == '\t' * TABS, 'Not a snapshot line' + repr(l)
                props[SNAPSHOTS].append(l[TABS:])
        else:
            k, v = l.strip().split(':', 1)
            k = k.rstrip(':')
            v = v.strip()
            if k == SNAPSHOTS:
                assert v == '', f'Should have nothing after ":" in: {l}'
                props[SNAPSHOTS] = []
            else:
                assert k not in props, f'{l} already had a value {props[k]}'
                if k.endswith(' UUID') and v == '-':
                    v = None
                props[k] = v
    return props


class SubvolumeOnDisk(namedtuple('SubvolumeOnDisk', [
    _BTRFS_UUID,
    _BTRFS_PARENT_UUID,
    _HOSTNAME,
    _SUBVOLUMES_BASE_DIR,
    _SUBVOLUME_REL_PATH,
])):
    '''
    This class stores a disk path to a btrfs subvolume (built image layer),
    together with some minimal metadata about the layer.  It knows how to
    serialize & deserialize this metadata to a JSON format that can be
    safely used as as Buck output representing the subvolume.
    '''

    def subvolume_path(self):
        return os.path.join(self.subvolumes_base_dir, self.subvolume_rel_path)

    @classmethod
    def from_subvolume_path(
        cls,
        subvol_path: str,
        subvolumes_dir: str,
    ):
        subvol_rel_path = os.path.relpath(subvol_path, subvolumes_dir)
        pieces = subvol_rel_path.split('/')
        if pieces[:1] == [''] or '..' in pieces:
            raise RuntimeError(
                f'{subvol_path} must be located inside the subvolumes '
                f'directory {subvolumes_dir}'
            )
        # This function deliberately does no validation on the fields it
        # populates -- that is done only in `from_serializable_dict`.  We
        # will not commit a buggy structure to disk since
        # `to_serializable_dict` checks the idepmpotency of our
        # serialization-deserialization.
        volume_props = _btrfs_get_volume_props(subvol_path)
        self = cls(**{
            _BTRFS_UUID: volume_props['UUID'],
            _BTRFS_PARENT_UUID: volume_props['Parent UUID'],
            _HOSTNAME: socket.gethostname(),
            _SUBVOLUMES_BASE_DIR: subvolumes_dir,
            _SUBVOLUME_REL_PATH: subvol_rel_path,
        })
        return self

    @classmethod
    def from_serializable_dict(cls, d, subvolumes_dir):
        # This incidentally checks that the subvolume exists and is btrfs.
        subvol_path = os.path.join(
            # This is copypasta of subvolume_path() but I need it before
            # creating the object. The assert below keeps them in sync.
            subvolumes_dir, d[_SUBVOLUME_REL_PATH],
        )
        volume_props = _btrfs_get_volume_props(subvol_path)
        self = cls(**{
            _BTRFS_UUID: d[_BTRFS_UUID],
            _BTRFS_PARENT_UUID: volume_props['Parent UUID'],
            _HOSTNAME: d[_HOSTNAME],
            _SUBVOLUMES_BASE_DIR: subvolumes_dir,
            _SUBVOLUME_REL_PATH: d[_SUBVOLUME_REL_PATH],
        })
        assert subvol_path == self.subvolume_path(), (d, subvolumes_dir)

        # Check that the relative path is garbage-collectable.
        inner_dir = os.path.basename(d[_SUBVOLUME_REL_PATH])
        outer_dir = os.path.basename(os.path.dirname(d[_SUBVOLUME_REL_PATH]))
        if ':' not in outer_dir or (
            d[_SUBVOLUME_REL_PATH] != os.path.join(outer_dir, inner_dir)
        ):
            raise RuntimeError(
                'Subvolume must have the form <rule name>:<version>/<subvol>,'
                f' not {d[_SUBVOLUME_REL_PATH]}'
            )
        outer_dir_content = os.listdir(os.path.join(subvolumes_dir, outer_dir))
        # For GC, the wrapper must contain the subvolume, and nothing else.
        if outer_dir_content != [inner_dir]:
            raise RuntimeError(
                f'Subvolume wrapper {outer_dir} contained {outer_dir_content} '
                f'instead of {[inner_dir]}'
            )
        # Check that the subvolume matches the description.
        cur_host = socket.gethostname()
        if cur_host != self.hostname:
            raise RuntimeError(
                f'Subvolume {self} did not come from current host {cur_host}'
            )
        if volume_props['UUID'] != self.btrfs_uuid:
            raise RuntimeError(
                f'UUID in subvolume JSON {self} does not match that of the '
                f'actual subvolume {volume_props}'
            )
        return self

    def to_serializable_dict(self):
        # `subvolumes_dir` is an absolute path to a known location inside
        # the repo.  We must not serialize it inside a Buck outputs, since
        # that will break if the repo is moved.  Instead, we always
        # recompute the path relative to the current subvolumes directory.
        d = {
            _BTRFS_UUID: self.btrfs_uuid,
            # Not serializing _BTRFS_PARENT_UUID since it's always deduced.
            _HOSTNAME: self.hostname,
            _SUBVOLUME_REL_PATH: self.subvolume_rel_path,
            _DANGER: 'Do NOT edit manually: this can break future builds, or '
                'break refcounting, causing us to leak or prematurely destroy '
                'subvolumes.',
        }
        # Self-test -- there should be no way for this assertion to fail
        new_self = self.from_serializable_dict(d, self.subvolumes_base_dir)
        assert self == new_self, \
          f'Got {new_self} from {d}, when serializing {self}'
        return d

    @classmethod
    def from_json_file(cls, infile, subvolumes_dir):
        parsed_json = '<NO JSON PARSED>'
        try:
            parsed_json = json.load(infile)
            return cls.from_serializable_dict(
                parsed_json, subvolumes_dir
            )
        except json.JSONDecodeError as ex:
            raise RuntimeError(
                f'Parsing subvolume JSON from {infile}: {ex.doc}'
            ) from ex
        except Exception as ex:
            raise RuntimeError(
                f'Parsed subvolume JSON from {infile}: {parsed_json}'
            ) from ex

    def to_json_file(self, outfile):
        outfile.write(json.dumps(self.to_serializable_dict()))
