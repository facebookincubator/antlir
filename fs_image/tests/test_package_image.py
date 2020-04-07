#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import stat
import subprocess
import sys
import tempfile
import unittest

from contextlib import contextmanager
from typing import Iterator

from fs_image.btrfs_diff.tests.render_subvols import render_sendstream

from ..find_built_subvol import subvolumes_dir
from ..package_image import package_image, Format
from ..unshare import Namespace, nsenter_as_root, Unshare
from .temp_subvolumes import TempSubvolumes


def _render_sendstream_path(path):
    if path.endswith('.zst'):
        data = subprocess.check_output(
            ['zstd', '--decompress', '--stdout', path]
        )
    else:
        with open(path, 'rb') as infile:
            data = infile.read()
    return render_sendstream(data)


class PackageImageTestCase(unittest.TestCase):

    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        self.subvolumes_dir = subvolumes_dir(sys.argv[0])
        # Works in @mode/opt since the files of interest are baked into the XAR
        self.my_dir = os.path.dirname(__file__)

    @contextmanager
    def _package_image(
        self,
        layer_path: str,
        format: str,
        writable_subvolume: bool = False,
        seed_device: bool = False,
    ) -> Iterator[str]:
        with tempfile.TemporaryDirectory() as td:
            out_path = os.path.join(td, format)
            package_image([
                '--subvolumes-dir', self.subvolumes_dir,
                '--layer-path', layer_path,
                '--format', format,
                '--output-path', out_path,
                *(['--writable-subvolume'] if writable_subvolume else []),
                *(['--seed-device'] if seed_device else []),
            ])
            yield out_path

    def _sibling_path(self, rel_path: str):
        return os.path.join(self.my_dir, rel_path)

    def _assert_sendstream_files_equal(self, path1: str, path2: str):
        self.assertEqual(
            _render_sendstream_path(path1), _render_sendstream_path(path2),
        )

    # This tests `image_package.bzl` by consuming its output.
    def test_packaged_sendstream_matches_original(self):
        self._assert_sendstream_files_equal(
            self._sibling_path('create_ops-original.sendstream'),
            self._sibling_path('create_ops.sendstream'),
        )

    def test_package_image_as_sendstream(self):
        for format in ['sendstream', 'sendstream.zst']:
            with self._package_image(
                self._sibling_path('create_ops.layer'), format,
            ) as out_path:
                self._assert_sendstream_files_equal(
                    self._sibling_path('create_ops-original.sendstream'),
                    out_path,
                )

    def test_package_image_as_btrfs_loopback(self):
        with self._package_image(
            self._sibling_path('create_ops.layer'), 'btrfs',
        ) as out_path, \
                Unshare([Namespace.MOUNT, Namespace.PID]) as unshare, \
                tempfile.TemporaryDirectory() as mount_dir, \
                tempfile.NamedTemporaryFile() as temp_sendstream:
            # Future: use a LoopbackMount object here once that's checked in.
            subprocess.check_call(nsenter_as_root(
                unshare, 'mount', '-t', 'btrfs', '-o', 'loop,discard,nobarrier',
                out_path, mount_dir,
            ))
            try:
                # Future: Once I have FD, this should become:
                # Subvol(
                #     os.path.join(mount_dir.fd_path(), 'create_ops'),
                #     already_exists=True,
                # ).mark_readonly_and_write_sendstream_to_file(temp_sendstream)
                subprocess.check_call(nsenter_as_root(
                    unshare, 'btrfs', 'send', '-f', temp_sendstream.name,
                    os.path.join(mount_dir, 'create_ops'),
                ))
                self._assert_sendstream_files_equal(
                    self._sibling_path('create_ops-original.sendstream'),
                    temp_sendstream.name,
                )
            finally:
                nsenter_as_root(unshare, 'umount', mount_dir)

    def test_package_image_as_btrfs_loopback_writable(self):
        with self._package_image(
            self._sibling_path('create_ops.layer'),
            'btrfs',
            writable_subvolume=True,
        ) as out_path, \
                Unshare([Namespace.MOUNT, Namespace.PID]) as unshare, \
                tempfile.TemporaryDirectory() as mount_dir:
            os.chmod(
                out_path,
                stat.S_IMODE(os.stat(out_path).st_mode)
                | (stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH),
            )
            subprocess.check_call(nsenter_as_root(
                unshare, 'mount', '-t', 'btrfs', '-o', 'loop,discard,nobarrier',
                out_path, mount_dir,
            ))
            try:
                subprocess.check_call(nsenter_as_root(
                    unshare, 'touch', os.path.join(mount_dir,
                                                   'create_ops',
                                                   'foo'),
                ))
            finally:
                nsenter_as_root(unshare, 'umount', mount_dir)

    def test_package_image_as_btrfs_seed_device(self):
        with self._package_image(
            self._sibling_path('create_ops.layer'),
            'btrfs',
            writable_subvolume=True,
            seed_device=True,
        ) as out_path:
            proc = subprocess.run(
                ["btrfs", "inspect-internal", "dump-super", out_path],
                check=True,
                stdout=subprocess.PIPE
            )
            self.assertIn(b"SEEDING", proc.stdout)
        with self._package_image(
            self._sibling_path('create_ops.layer'),
            'btrfs',
            writable_subvolume=True,
            seed_device=False,
        ) as out_path:
            proc = subprocess.run(
                ["btrfs", "inspect-internal", "dump-super", out_path],
                check=True,
                stdout=subprocess.PIPE
            )
            self.assertNotIn(b"SEEDING", proc.stdout)

    def test_format_name_collision(self):
        with self.assertRaisesRegex(AssertionError, 'share format_name'):

            class BadFormat(Format, format_name='sendstream'):
                pass

    def test_package_image_as_squashfs(self):
        with self._package_image(
            self._sibling_path('create_ops.layer'), 'squashfs',
        ) as out_path, TempSubvolumes(sys.argv[0]) as temp_subvolumes, \
                tempfile.NamedTemporaryFile() as temp_sendstream:
            subvol = temp_subvolumes.create('subvol')
            with Unshare([Namespace.MOUNT, Namespace.PID]) as unshare, \
                    tempfile.TemporaryDirectory() as mount_dir:
                subprocess.check_call(nsenter_as_root(
                    unshare, 'mount', '-t', 'squashfs', '-o', 'loop',
                    out_path, mount_dir,
                ))
                # `unsquashfs` would have been cleaner than `mount` +
                # `rsync`, and faster too, but unfortunately it corrupts
                # device nodes as of v4.3.
                subprocess.check_call(nsenter_as_root(
                    unshare, 'rsync', '--archive', '--hard-links',
                    '--sparse', '--xattrs', mount_dir + '/', subvol.path(),
                ))
            with subvol.mark_readonly_and_write_sendstream_to_file(
                temp_sendstream
            ):
                pass
            original_render = _render_sendstream_path(
                self._sibling_path('create_ops-original.sendstream'),
            )
            # SquashFS does not preserve the original's cloned extents of
            # zeros, nor the zero-hole-zero patter.  In all cases, it
            # (efficiently) transmutes the whole file into 1 sparse hole.
            self.assertEqual(original_render[1].pop('56KB_nuls'), [
                '(File d57344(create_ops@56KB_nuls_clone:0+49152@0/' +
                'create_ops@56KB_nuls_clone:49152+8192@49152))'
            ])
            original_render[1]['56KB_nuls'] = ['(File h57344)']
            self.assertEqual(original_render[1].pop('56KB_nuls_clone'), [
                '(File d57344(create_ops@56KB_nuls:0+49152@0/' +
                'create_ops@56KB_nuls:49152+8192@49152))'
            ])
            original_render[1]['56KB_nuls_clone'] = ['(File h57344)']
            self.assertEqual(original_render[1].pop('zeros_hole_zeros'), [
                '(File d16384h16384d16384)'
            ])
            original_render[1]['zeros_hole_zeros'] = ['(File h49152)']
            self.assertEqual(
                original_render, _render_sendstream_path(temp_sendstream.name),
            )
