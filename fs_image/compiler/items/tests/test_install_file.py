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

from fs_image.compiler.requires_provides import (
    ProvidesDirectory, ProvidesFile, require_directory
)
from find_built_subvol import find_built_subvol
from fs_image.fs_utils import temp_dir, Path
from tests.temp_subvolumes import TempSubvolumes

from ..common import image_source_item
from ..install_file import _InstallablePath, InstallFileItem

from .common import BaseItemTestCase, DUMMY_LAYER_OPTS, render_subvol


def _install_file_item(**kwargs):
    # The dummy object works here because `subvolumes_dir` of `None` runs
    # `artifacts_dir` internally, while our "prod" path uses the
    # already-computed value.
    return image_source_item(
        InstallFileItem, exit_stack=None, layer_opts=DUMMY_LAYER_OPTS,
    )(**kwargs)


class InstallFileItemTestCase(BaseItemTestCase):

    def test_phase_order(self):
        self.assertIs(
            None,
            InstallFileItem(
                from_target='t', source='/etc/passwd', dest='b',
            ).phase_order(),
        )

    def test_install_file(self):
        with tempfile.NamedTemporaryFile() as tf:
            os.chmod(tf.name, stat.S_IXUSR)
            exe_item = _install_file_item(
                from_target='t', source={'source': tf.name}, dest='d/c',
            )
        ep = _InstallablePath(Path(tf.name), ProvidesFile(path='d/c'), 'a+rx')
        self.assertEqual((ep,), exe_item.paths)
        self.assertEqual(tf.name.encode(), exe_item.source)
        self._check_item(exe_item, {ep.provides}, {require_directory('d')})

        # Checks `image.source(path=...)`
        with temp_dir() as td:
            os.mkdir(td / 'b')
            open(td / 'b/q', 'w').close()
            data_item = _install_file_item(
                from_target='t',
                source={'source': td, 'path': '/b/q'},
                dest='d',
            )
        dp = _InstallablePath(td / 'b/q', ProvidesFile(path='d'), 'a+r')
        self.assertEqual((dp,), data_item.paths)
        self.assertEqual(td / 'b/q', data_item.source)
        self._check_item(data_item, {dp.provides}, {require_directory('/')})

        # NB: We don't need to get coverage for this check on ALL the items
        # because the presence of the ProvidesDoNotAccess items it the real
        # safeguard -- e.g. that's what prevents TarballItem from writing
        # to /meta/ or other protected paths.
        with self.assertRaisesRegex(AssertionError, 'cannot start with meta/'):
            _install_file_item(
                from_target='t', source={'source': 'a/b/c'}, dest='/meta/foo',
            )

    def test_install_file_from_layer(self):
        layer = find_built_subvol(
            Path(__file__).dirname() / 'test-with-one-local-rpm'
        )
        path_in_layer = b'usr/share/rpm_test/cheese2.txt'
        item = _install_file_item(
            from_target='t',
            source={'layer': layer, 'path': '/' + path_in_layer.decode()},
            dest='cheese2',
        )
        source_path = layer.path(path_in_layer)
        p = _InstallablePath(source_path, ProvidesFile(path='cheese2'), 'a+r')
        self.assertEqual((p,), item.paths)
        self.assertEqual(source_path, item.source)
        self._check_item(item, {p.provides}, {require_directory('/')})

    def test_install_file_command(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes, \
                tempfile.NamedTemporaryFile() as empty_tf:
            subvol = temp_subvolumes.create('tar-sv')
            subvol.run_as_root(['mkdir', subvol.path('d')])

            _install_file_item(
                from_target='t', source={'source': empty_tf.name},
                dest='/d/empty',
            ).build(subvol, DUMMY_LAYER_OPTS)
            self.assertEqual(
                ['(Dir)', {'d': ['(Dir)', {'empty': ['(File m444)']}]}],
                render_subvol(subvol),
            )

            # Fail to write to a nonexistent dir
            with self.assertRaises(subprocess.CalledProcessError):
                _install_file_item(
                    from_target='t', source={'source': empty_tf.name},
                    dest='/no_dir/empty',
                ).build(subvol, DUMMY_LAYER_OPTS)

            # Running a second copy to the same destination. This just
            # overwrites the previous file, because we have a build-time
            # check for this, and a run-time check would add overhead.
            _install_file_item(
                from_target='t', source={'source': empty_tf.name},
                dest='/d/empty',
                # A non-default mode & owner shows that the file was
                # overwritten, and also exercises HasStatOptions.
                mode='u+rw', user_group='12:34',
            ).build(subvol, DUMMY_LAYER_OPTS)
            self.assertEqual(
                ['(Dir)', {'d': ['(Dir)', {'empty': ['(File m600 o12:34)']}]}],
                render_subvol(subvol),
            )

    def test_install_file_unsupported_types(self):
        with self.assertRaisesRegex(
            RuntimeError, ' must be a regular file or directory, '
        ):
            _install_file_item(
                from_target='t', source={'source': '/dev/null'}, dest='d/c',
            )
        with self.assertRaisesRegex(RuntimeError, ' neither a file nor a dir'):
            _install_file_item(
                from_target='t', source={'source': '/dev'}, dest='d/c',
            )

    def test_install_file_command_recursive(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create('tar-sv')
            subvol.run_as_root(['mkdir', subvol.path('d')])

            with temp_dir() as td:
                with open(td / 'data.txt', 'w') as df:
                    print('Hello', file=df)
                os.mkdir(td / 'subdir')
                with open(td / 'subdir/exe.sh', 'w') as ef:
                    print('#!/bin/sh\necho "Hello"', file=ef)
                os.chmod(td / 'subdir/exe.sh', 0o100)

                dir_item = _install_file_item(
                    from_target='t', source={'source': td}, dest='/d/a',
                )

                ps = [
                    _InstallablePath(
                        td,
                        ProvidesDirectory(path='d/a'),
                        'u+rwx,og+rx',
                    ),
                    _InstallablePath(
                        td / 'data.txt',
                        ProvidesFile(path='d/a/data.txt'),
                        'a+r',
                    ),
                    _InstallablePath(
                        td / 'subdir',
                        ProvidesDirectory(path='d/a/subdir'),
                        'u+rwx,og+rx',
                    ),
                    _InstallablePath(
                        td / 'subdir/exe.sh',
                        ProvidesFile(path='d/a/subdir/exe.sh'),
                        'a+rx',
                    ),
                ]
                self.assertEqual(sorted(ps), sorted(dir_item.paths))
                self.assertEqual(td, dir_item.source)
                self._check_item(
                    dir_item, {p.provides for p in ps}, {require_directory('d')}
                )

                # This implicitly checks that `a` precedes its contents.
                dir_item.build(subvol, DUMMY_LAYER_OPTS)

            self.assertEqual(
                ['(Dir)', {'d': ['(Dir)', {'a': ['(Dir)', {
                    'data.txt': ['(File m444 d6)'],
                    'subdir': ['(Dir)', {
                        'exe.sh': ['(File m555 d23)'],
                    }],
                }]}]}],
                render_subvol(subvol),
            )
