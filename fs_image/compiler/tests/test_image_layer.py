#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
import json
import os
import unittest

from contextlib import contextmanager
from grp import getgrnam
from pwd import getpwnam

from btrfs_diff.tests.render_subvols import render_sendstream, pop_path
from btrfs_diff.tests.demo_sendstreams_expected import render_demo_subvols
from find_built_subvol import find_built_subvol
from rpm.yum_dnf_conf import YumDnf


TARGET_ENV_VAR_PREFIX = 'test_image_layer_path_to_'
TARGET_TO_PATH = {
    target[len(TARGET_ENV_VAR_PREFIX):]: path
        for target, path in os.environ.items()
            if target.startswith(TARGET_ENV_VAR_PREFIX)
}


class ImageLayerTestCase(unittest.TestCase):

    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    @contextmanager
    def target_subvol(self, target, mount_config=None):
        with self.subTest(target):
            # The mount configuration is very uniform, so we can check it here.
            expected_config = {
                'is_directory': True,
                'build_source': {
                    'type': 'layer',
                    'source': '//fs_image/compiler/test_images:' + target,
                },
            }
            if mount_config:
                expected_config.update(mount_config)
            with open(TARGET_TO_PATH[target] + '/mountconfig.json') as infile:
                self.assertEqual(expected_config, json.load(infile))
            yield find_built_subvol(TARGET_TO_PATH[target])

    def _check_hello(self, subvol_path):
        with open(os.path.join(subvol_path, b'hello_world')) as hello:
            self.assertEqual('', hello.read())

    def _check_parent(self, subvol_path):
        self._check_hello(subvol_path)
        # :parent_layer
        for path in [
            b'usr/share/rpm_test/hello_world.tar',
            b'foo/bar/even_more_hello_world.tar',
        ]:
            self.assertTrue(
                os.path.isfile(os.path.join(subvol_path, path)),
                path,
            )
        # :feature_dirs not tested by :parent_layer
        self.assertTrue(
            os.path.isdir(os.path.join(subvol_path, b'foo/bar/baz')),
        )
        # :hello_world_base was mounted here
        self.assertTrue(os.path.exists(
            os.path.join(subvol_path, b'mounted_hello/hello_world')
        ))

        # :feature_symlinks
        for source, dest in [
            (b'bar', b'foo/fighter'),
            (b'bar', b'foo/face'),
            (b'..', b'foo/bar/baz/bar'),
            (b'hello_world.tar', b'foo/symlink_to_hello_world.tar'),
        ]:
            self.assertTrue(os.path.exists(os.path.join(
                subvol_path, os.path.dirname(dest), source,
            )), (dest, source))
            self.assertTrue(
                os.path.islink(os.path.join(subvol_path, dest)),
                dest
            )
            self.assertEqual(
                source, os.readlink(os.path.join(subvol_path, dest))
            )

    def _check_child(self, subvol_path):
        self._check_parent(subvol_path)
        for path in [
            # :feature_tar_and_rpms
            b'foo/borf/hello_world',
            b'foo/hello_world',
            b'usr/share/rpm_test/mice.txt',
            b'usr/share/rpm_test/cheese2.txt',
            # :child/layer
            b'foo/extracted_hello/hello_world',
            b'foo/more_extracted_hello/hello_world',
        ]:
            self.assertTrue(os.path.isfile(os.path.join(subvol_path, path)))
        for path in [
            # :feature_tar_and_rpms ensures these are absent
            b'usr/share/rpm_test/carrot.txt',
            b'usr/share/rpm_test/milk.txt',
        ]:
            self.assertFalse(os.path.exists(os.path.join(subvol_path, path)))

    def test_image_layer_targets(self):
        # Future: replace these checks by a more comprehensive test of the
        # image's data & metadata using our `btrfs_diff` library.
        with self.target_subvol(
            'hello_world_base',
            mount_config={'runtime_source': {'type': 'chicken'}},
        ) as subvol:
            self._check_hello(subvol.path())
        with self.target_subvol(
            'parent_layer',
            mount_config={'runtime_source': {'type': 'turkey'}},
        ) as subvol:
            self._check_parent(subvol.path())
            # Cannot check this in `_check_parent`, since that gets called
            # by `_check_child`, but the RPM gets removed in the child.
            self.assertTrue(os.path.isfile(
                subvol.path('usr/share/rpm_test/carrot.txt')
            ))
        with self.target_subvol('child/layer') as subvol:
            self._check_child(subvol.path())
        with self.target_subvol('base_cheese_layer') as subvol:
            self._check_hello(subvol.path())
            self.assertTrue(os.path.isfile(
                subvol.path('/usr/share/rpm_test/cheese2.txt')
            ))
        with self.target_subvol('older_cheese_layer') as subvol:
            self._check_hello(subvol.path())
            self.assertTrue(os.path.isfile(
                subvol.path('/usr/share/rpm_test/cheese1.txt')
            ))
            # Make sure the original file is removed when the RPM is downgraded
            self.assertFalse(os.path.isfile(
                subvol.path('/usr/share/rpm_test/cheese2.txt')
            ))
        with self.target_subvol('newer_cheese_layer') as subvol:
            self._check_hello(subvol.path())
            self.assertTrue(os.path.isfile(
                subvol.path('/usr/share/rpm_test/cheese3.txt')
            ))
            # Make sure the original file is removed when the RPM is upgraded
            self.assertFalse(os.path.isfile(
                subvol.path('/usr/share/rpm_test/cheese2.txt')
            ))
        with self.target_subvol('install_toy_rpm') as subvol:
            self._check_hello(subvol.path())
            self.assertTrue(os.path.isfile(
                subvol.path('/usr/bin/toy_src_file')
            ))

    def test_layer_from_demo_sendstreams(self):
        # `btrfs_diff.demo_sendstream` produces a subvolume send-stream with
        # fairly thorough coverage of filesystem features.  This test grabs
        # that send-stream, receives it into an `image_layer`, and validates
        # that the send-stream of the **received** volume has the same
        # rendering as the original send-stream was supposed to have.
        #
        # In other words, besides testing `image_sendstream_layer`, this is
        # also a test of idempotence for btrfs send+receive.
        #
        # Notes:
        #  - `compiler/tests/TARGETS` explains why `mutate_ops` is not here.
        #  - Currently, `mutate_ops` also uses `--no-data`, which would
        #    break this test of idempotence.
        for original_name, subvol_name, mount_config in [
            ('create_ops', 'create_ops', None),
            ('create_ops', 'create_ops-from-dir', None),
            ('create_ops', 'create_ops-from-layer', None),
            ('create_ops', 'create_ops-alias', {
                'build_source': {
                    'type': 'layer',
                    'source': '//fs_image/compiler/test_images:create_ops',
                }
            }),
        ]:
            with self.target_subvol(
                subvol_name, mount_config=mount_config,
            ) as sv:
                self.assertEqual(
                    render_demo_subvols(**{original_name: original_name}),
                    render_sendstream(sv.mark_readonly_and_get_sendstream()),
                )

    def _check_rpm_common(self, rendered_subvol, yum_dnf: YumDnf):
        r = copy.deepcopy(rendered_subvol)

        # Ignore a bunch of yum / dnf / rpm spam

        if yum_dnf == YumDnf.yum:
            ino, = pop_path(r, f'var/log/yum.log')
            self.assertRegex(ino, r'^\(File m600 d[0-9]+\)$')
            for ignore_dir in ['var/cache/yum', 'var/lib/yum']:
                ino, _ = pop_path(r, ignore_dir)
                self.assertEqual('(Dir)', ino)
        elif yum_dnf == YumDnf.dnf:
            self.assertEqual(['(Dir)', {
                'dnf': ['(Dir)', {'modules.d': ['(Dir)', {}]}],
            }], pop_path(r, 'etc'))
            for logname in [
                'dnf.log', 'dnf.librepo.log', 'dnf.rpm.log', 'hawkey.log',
            ]:
                ino, = pop_path(r, f'var/log/{logname}')
                self.assertRegex(ino, r'^\(File d[0-9]+\)$', logname)
            for ignore_dir in ['var/cache/dnf', 'var/lib/dnf']:
                ino, _ = pop_path(r, ignore_dir)
                self.assertEqual('(Dir)', ino)
            self.assertEqual(['(Dir)', {}], pop_path(r, 'var/tmp'))
        else:
            raise AssertionError(yum_dnf)

        ino, _ = pop_path(r, 'var/lib/rpm')
        self.assertEqual('(Dir)', ino)

        self.assertEqual(['(Dir)', {
            'dev': ['(Dir)', {}],
            'meta': ['(Dir)', {'private': ['(Dir)', {'opts': ['(Dir)', {
                'artifacts_may_require_repo': ['(File d2)'],
            }]}]}],
            'usr': ['(Dir)', {
                'share': ['(Dir)', {
                    # Whatever is here should be `pop_path`ed before
                    # calling `_check_rpm_common`.
                }],
            }],
            'var': ['(Dir)', {
                'cache': ['(Dir)', {}],
                'lib': ['(Dir)', {}],
                'log': ['(Dir)', {}],
            }],
        }], r)

    def test_build_appliance(self):
        # The appliance this uses defaults to `dnf`.  This is not a dual
        # test, unlike `test-rpm-action-item`, because force-overriding the
        # package manager is not currently exposed to the `.bzl` layer.  So
        # we only have the `dnf` build artifact to test here.
        #
        # If the extra coverage were thought important, we could either pass
        # this flag to the compiler CLI via `image.opts`, or just add a copy
        # of `repo-snapshot-for-tests` defaulting to `yum`.
        with self.target_subvol('validates-build-appliance') as sv:
            r = render_sendstream(sv.mark_readonly_and_get_sendstream())

            ino, = pop_path(r, 'bin/sh')  # Busybox from `rpm-test-milk`
            # NB: We changed permissions on this at some point, but after
            # the migration diffs land, the [75] can become a 5.
            self.assertRegex(ino, r'^\(File m[75]55 d[0-9]+\)$')

            self.assertEqual(['(Dir)', {
                'milk.txt': ['(File d12)'],
                # From the `rpm-test-milk` post-install script
                'post.txt': ['(File d6)'],
            }], pop_path(r, 'usr/share/rpm_test'))

            ino, _ = pop_path(r, 'usr/lib/.build-id')
            self.assertEqual('(Dir)', ino)
            self.assertEqual(['(Dir)', {}], pop_path(r, 'bin'))
            self.assertEqual(['(Dir)', {}], pop_path(r, 'usr/lib'))

            self._check_rpm_common(r, YumDnf.dnf)

    def test_non_default_rpm_snapshot(self):
        with self.target_subvol('layer-with-non-default-snapshot-rpm') as sv:
            r = render_sendstream(sv.mark_readonly_and_get_sendstream())

            self.assertEqual(['(Dir)', {
                'cake.txt': ['(File d17)'],
            }], pop_path(r, 'usr/share/rpm_test'))

            self._check_rpm_common(r, YumDnf.yum)

    def test_installed_files(self):
        with self.target_subvol('installed-files') as sv:
            r = render_sendstream(sv.mark_readonly_and_get_sendstream())

            # We don't know the exact sizes because these 2 may be wrapped
            ino, = pop_path(r, 'foo/bar/installed/print-ok')
            self.assertRegex(ino, r'^\(File m555 d[0-9]+\)$')
            ino, = pop_path(r, 'foo/bar/installed/print-ok-too')
            self.assertRegex(ino, r'^\(File m555 d[0-9]+\)$')

            uid = getpwnam('nobody').pw_uid
            gid = getgrnam('nobody').gr_gid
            self.assertEqual(['(Dir)', {
                'foo': ['(Dir)', {'bar': ['(Dir)', {
                    'baz': ['(Dir)', {}],
                    'hello_world.tar': ['(File m444 d10240)'],
                    'hello_world_again.tar': [
                        f'(File m444 o{uid}:{gid} d10240)'
                    ],
                    'installed': ['(Dir)', {
                        'yittal-kitteh': ['(File m444 d5)'],
                        'script-dir': ['(Dir)', {
                            'subdir': ['(Dir)', {
                                'exe.sh': ['(File m555 d21)'],
                            }],
                            'data.txt': ['(File m444 d6)'],
                        }],
                        'solo-exe.sh': ['(File m555 d21)'],
                    }],
                }]}],
                'meta': ['(Dir)', {'private': ['(Dir)', {'opts': ['(Dir)', {
                    'artifacts_may_require_repo': ['(File d2)'],
                }]}]}],
            }], r)
