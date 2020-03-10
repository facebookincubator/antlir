#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import os
import tempfile
import unittest
import unittest.mock

from .. import subvolume_on_disk

_MY_HOST = 'my_host'


class SubvolumeOnDiskTestCase(unittest.TestCase):

    def _test_uuid(self, subvolume_path):
        if self._mock_uuid_stack:
            return self._mock_uuid_stack.pop()
        return f'test_uuid_of:{subvolume_path}'

    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        # Configure mocks shared by most of the tests.
        self._mock_uuid_stack = []

        self.patch_btrfs_get_volume_props = unittest.mock.patch.object(
            subvolume_on_disk, '_btrfs_get_volume_props'
        )
        self.mock_btrfs_get_volume_props = \
            self.patch_btrfs_get_volume_props.start()
        self.mock_btrfs_get_volume_props.side_effect = lambda subvolume_path: {
            # Since we key the uuid off the given argument, we don't have to
            # explicitly validate the given path for each mock call.
            'UUID': self._test_uuid(subvolume_path),
            'Parent UUID': 'zupa',
        }
        self.addCleanup(self.patch_btrfs_get_volume_props.stop)

        self.patch_getfqdn = unittest.mock.patch('socket.getfqdn')
        self.mock_getfqdn = self.patch_getfqdn.start()
        self.mock_getfqdn.side_effect = lambda: _MY_HOST
        self.addCleanup(self.patch_getfqdn.stop)

    def _check(self, actual_subvol, expected_path, expected_subvol):
        self.assertEqual(expected_path, actual_subvol.subvolume_path())
        self.assertEqual(expected_subvol, actual_subvol)

        # Automatically tests "normal case" serialization & deserialization
        fake_file = io.StringIO()

        # `to_json` will validate UUIDs by running `from`.
        stack_size = len(self._mock_uuid_stack)
        self._mock_uuid_stack.append(actual_subvol.btrfs_uuid)

        actual_subvol.to_json_file(fake_file)
        self.assertEqual(stack_size, len(self._mock_uuid_stack))
        fake_file.seek(0)

        # The `from` validation will consume another UUID.
        self._mock_uuid_stack.append(actual_subvol.btrfs_uuid)
        self.assertEqual(
            actual_subvol,
            subvolume_on_disk.SubvolumeOnDisk.from_json_file(
                fake_file, actual_subvol.subvolumes_base_dir
            ),
        )
        self.assertEqual(stack_size, len(self._mock_uuid_stack))

    def test_from_json_file_errors(self):
        with self.assertRaisesRegex(RuntimeError, 'Parsing subvolume JSON'):
            subvolume_on_disk.SubvolumeOnDisk.from_json_file(
                io.StringIO('invalid json'), '/subvols'
            )
        with self.assertRaisesRegex(RuntimeError, 'Parsed subvolume JSON'):
            subvolume_on_disk.SubvolumeOnDisk.from_json_file(
                io.StringIO('5'), '/subvols'
            )

    def test_from_serializable_dict_and_validation(self):
        with tempfile.TemporaryDirectory() as td:
            # Note: Unlike test_from_subvolume_path, this test uses a
            # trailing / (to increase coverage).
            subvols = td + '/'
            rel_path = 'test_subvol:v/test_subvol'
            good_path = os.path.join(subvols, rel_path)
            os.makedirs(good_path)  # `from_serializable_dict` checks this
            good_uuid = self._test_uuid(good_path)
            good = {
                subvolume_on_disk._BTRFS_UUID: good_uuid,
                subvolume_on_disk._HOSTNAME: _MY_HOST,
                subvolume_on_disk._SUBVOLUME_REL_PATH: rel_path,
            }

            bad_path = good.copy()
            bad_path[subvolume_on_disk._SUBVOLUME_REL_PATH] += '/x'
            with self.assertRaisesRegex(RuntimeError, 'must have the form'):
                subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
                    bad_path, subvols
                )

            wrong_inner = good.copy()
            wrong_inner[subvolume_on_disk._SUBVOLUME_REL_PATH] += 'x'
            with self.assertRaisesRegex(
                RuntimeError, r"\['test_subvol'\] instead of \['test_subvolx'"
            ):
                subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
                    wrong_inner, subvols
                )

            bad_host = good.copy()
            bad_host[subvolume_on_disk._HOSTNAME] = f'NOT_{_MY_HOST}'
            with self.assertRaisesRegex(
                RuntimeError, 'did not come from current host'
            ):
                subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
                    bad_host, subvols
                )

            bad_uuid = good.copy()
            bad_uuid[subvolume_on_disk._BTRFS_UUID] = 'BAD_UUID'
            with self.assertRaisesRegex(
                RuntimeError, 'UUID in subvolume JSON .* does not match'
            ):
                subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
                    bad_uuid, subvols
                )

            # Parsing the `good` dict does not throw, and gets the right result
            good_subvol = subvolume_on_disk.SubvolumeOnDisk \
                .from_serializable_dict(good, subvols)
            self._check(
                good_subvol,
                good_path,
                subvolume_on_disk.SubvolumeOnDisk(**{
                    subvolume_on_disk._BTRFS_UUID: good_uuid,
                    subvolume_on_disk._BTRFS_PARENT_UUID: 'zupa',
                    subvolume_on_disk._HOSTNAME: _MY_HOST,
                    subvolume_on_disk._SUBVOLUME_REL_PATH: rel_path,
                    subvolume_on_disk._SUBVOLUMES_BASE_DIR: subvols,
                }),
            )

    def test_from_subvolume_path(self):
        with tempfile.TemporaryDirectory() as td:
            # Note: Unlike test_from_serializable_dict_and_validation, this
            # test does NOT use a trailing / (to increase coverage).
            subvols = td.rstrip('/')
            rel_path = 'test_rule:vvv/test:subvol'
            subvol_path = os.path.join(subvols, rel_path)
            os.makedirs(subvol_path)  # `from_serializable_dict` checks this

            subvol = subvolume_on_disk.SubvolumeOnDisk.from_subvolume_path(
                subvol_path=subvol_path, subvolumes_dir=subvols,
            )
            with unittest.mock.patch('os.listdir') as listdir:
                listdir.return_value = ['test:subvol']
                self._check(
                    subvol,
                    subvol_path,
                    subvolume_on_disk.SubvolumeOnDisk(**{
                        subvolume_on_disk._BTRFS_UUID:
                            self._test_uuid(subvol_path),
                        subvolume_on_disk._BTRFS_PARENT_UUID: 'zupa',
                        subvolume_on_disk._HOSTNAME: _MY_HOST,
                        subvolume_on_disk._SUBVOLUME_REL_PATH: rel_path,
                        subvolume_on_disk._SUBVOLUMES_BASE_DIR: subvols,
                    }),
                )
                self.assertEqual(
                    listdir.call_args_list,
                    [((os.path.dirname(subvol_path),),)] * 2,
                )

            with self.assertRaisesRegex(
                RuntimeError, 'must be located inside the subvolumes directory'
            ):
                subvolume_on_disk.SubvolumeOnDisk.from_subvolume_path(
                    subvol_path=subvol_path, subvolumes_dir=subvols + '/bad',
                )


class BtrfsVolumePropsTestCase(unittest.TestCase):
    'Separate from SubvolumeOnDiskTestCase because to avoid its mocks.'

    @unittest.mock.patch('subprocess.check_output')
    def test_btrfs_get_volume_props(self, check_output):
        parent = '/subvols/dir/parent'
        check_output.return_value = b'''\
dir/parent
\tName: \t\t\tparent
\tUUID: \t\t\tf96b940f-10d3-fc4e-8b2d-9362af0ee8df
\tParent UUID: \t\t-
\tReceived UUID: \t\t-
\tCreation time: \t\t2017-12-29 21:55:54 -0800
\tSubvolume ID:  \t\t277
\tGeneration: \t\t123
\tGen at creation: \t103
\tParent ID: \t\t5
\tTop level ID: \t\t5
\tFlags: \t\t\treadonly
\tSnapshot(s):
\t\t\t\tdir/foo
\t\t\t\tdir/bar
'''
        self.assertEquals(
            subvolume_on_disk._btrfs_get_volume_props(parent),
            {
                'Name': 'parent',
                'UUID': 'f96b940f-10d3-fc4e-8b2d-9362af0ee8df',
                'Parent UUID': None,
                'Received UUID': None,
                'Creation time': '2017-12-29 21:55:54 -0800',
                'Subvolume ID': '277',
                'Generation': '123',
                'Gen at creation': '103',
                'Parent ID': '5',
                'Top level ID': '5',
                'Flags': 'readonly',
                'Snapshot(s)': ['dir/foo', 'dir/bar'],
            }
        )
        check_output.assert_called_once_with(
            ['sudo', 'btrfs', 'subvolume', 'show', parent]
        )

        # Unlike the parent, this has no snapshots, so the format differs.
        child = '/subvols/dir/child'
        check_output.reset_mock()
        check_output.return_value = b'''\
dir/child
\tName: \t\t\tchild
\tUUID: \t\t\ta1a3eb3e-eb89-7743-8335-9cd5219248e7
\tParent UUID: \t\tf96b940f-10d3-fc4e-8b2d-9362af0ee8df
\tReceived UUID: \t\t-
\tCreation time: \t\t2017-12-29 21:56:32 -0800
\tSubvolume ID: \t\t278
\tGeneration: \t\t121
\tGen at creation: \t\t107
\tParent ID: \t\t5
\tTop level ID: \t\t5
\tFlags: \t\t\t-
\tSnapshot(s):
'''
        self.assertEquals(
            subvolume_on_disk._btrfs_get_volume_props(child),
            {
                'Name': 'child',
                'UUID': 'a1a3eb3e-eb89-7743-8335-9cd5219248e7',
                'Parent UUID': 'f96b940f-10d3-fc4e-8b2d-9362af0ee8df',
                'Received UUID': None,
                'Creation time': '2017-12-29 21:56:32 -0800',
                'Subvolume ID': '278',
                'Generation': '121',
                'Gen at creation': '107',
                'Parent ID': '5',
                'Top level ID': '5',
                'Flags': '-',
                'Snapshot(s)': [],
            }
        )
        check_output.assert_called_once_with(
            ['sudo', 'btrfs', 'subvolume', 'show', child]
        )


if __name__ == '__main__':
    unittest.main()
