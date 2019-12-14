#!/usr/bin/env python3
import sys
import unittest.mock

from tests.temp_subvolumes import TempSubvolumes

from ..common import PhaseOrder, protected_path_set
from ..install_file import InstallFileItem
from ..make_dirs import MakeDirsItem
from ..remove_path import RemovePathAction, RemovePathItem
from ..symlink import SymlinkToDirItem

from .common import BaseItemTestCase, DUMMY_LAYER_OPTS, render_subvol


class RemovePathItemTestCase(BaseItemTestCase):

    def test_remove_item(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create('remove_action')
            self.assertEqual(['(Dir)', {}], render_subvol(subvol))

            MakeDirsItem(
                from_target='t', path_to_make='/a/b/c', into_dir='/',
            ).build(subvol, DUMMY_LAYER_OPTS)
            for d in ['d', 'e']:
                InstallFileItem(
                    from_target='t', source='/dev/null', dest=f'/a/b/c/{d}',
                ).build(subvol, DUMMY_LAYER_OPTS)
            MakeDirsItem(
                from_target='t', path_to_make='/f/g', into_dir='/',
            ).build(subvol, DUMMY_LAYER_OPTS)
            # Checks that `rm` won't follow symlinks
            SymlinkToDirItem(
                from_target='t', source='/f', dest='/a/b/f_sym',
            ).build(subvol, DUMMY_LAYER_OPTS)
            for d in ['h', 'i']:
                InstallFileItem(
                    from_target='t', source='/dev/null', dest=f'/f/{d}',
                ).build(subvol, DUMMY_LAYER_OPTS)
            SymlinkToDirItem(
                from_target='t', source='/f/i', dest='/f/i_sym',
            ).build(subvol, DUMMY_LAYER_OPTS)
            intact_subvol = ['(Dir)', {
                'a': ['(Dir)', {
                    'b': ['(Dir)', {
                        'c': ['(Dir)', {
                            'd': ['(File m444)'],
                            'e': ['(File m444)'],
                        }],
                        'f_sym': ['(Symlink /f)'],
                    }],
                }],
                'f': ['(Dir)', {
                    'g': ['(Dir)', {}],
                    'h': ['(File m444)'],
                    'i': ['(File m444)'],
                    'i_sym': ['(Symlink /f/i)'],
                }],
            }]
            self.assertEqual(intact_subvol, render_subvol(subvol))

            # We refuse to touch protected paths, even with "if_exists".  If
            # the paths started with 'meta', they would trip the check in
            # `_make_path_normal_relative`, so we mock-protect 'xyz'.
            for prot_path in ['xyz', 'xyz/potato/carrot']:
                with unittest.mock.patch(
                    'fs_image.compiler.items.remove_path.protected_path_set',
                    side_effect=lambda sv: protected_path_set(sv) | {'xyz'},
                ), self.assertRaisesRegex(
                    AssertionError, f'Cannot remove protected .*{prot_path}',
                ):
                    RemovePathItem.get_phase_builder([RemovePathItem(
                        from_target='t',
                        action=RemovePathAction.if_exists,
                        path=prot_path,
                    )], DUMMY_LAYER_OPTS)(subvol)

            # Check handling of non-existent paths without removing anything
            remove = RemovePathItem(
                from_target='t',
                action=RemovePathAction.if_exists,
                path='/does/not/exist',
            )
            self.assertEqual(PhaseOrder.REMOVE_PATHS, remove.phase_order())
            RemovePathItem.get_phase_builder([remove], DUMMY_LAYER_OPTS)(subvol)
            with self.assertRaisesRegex(AssertionError, 'does not exist'):
                RemovePathItem.get_phase_builder([
                    RemovePathItem(
                        from_target='t',
                        action=RemovePathAction.assert_exists,
                        path='/does/not/exist',
                    ),
                ], DUMMY_LAYER_OPTS)(subvol)
            self.assertEqual(intact_subvol, render_subvol(subvol))

            # Now remove most of the subvolume.
            RemovePathItem.get_phase_builder([
                # These 3 removes are not covered by a recursive remove.
                # And we leave behind /f/i, which lets us know that neither
                # `f_sym` nor `i_sym` were followed during their deletion.
                RemovePathItem(
                    from_target='t',
                    action=RemovePathAction.assert_exists,
                    path='/f/i_sym',
                ),
                RemovePathItem(
                    from_target='t',
                    action=RemovePathAction.assert_exists,
                    path='/f/h',
                ),
                RemovePathItem(
                    from_target='t',
                    action=RemovePathAction.assert_exists,
                    path='/f/g',
                ),

                # The next 3 items are intentionally sequenced so that if
                # they were applied in the given order, they would fail.
                RemovePathItem(
                    from_target='t',
                    action=RemovePathAction.if_exists,
                    path='/a/b/c/e',
                ),
                RemovePathItem(
                    from_target='t',
                    action=RemovePathAction.assert_exists,
                    # The surrounding items don't delete /a/b/c/d, e.g. so
                    # this recursive remove is still tested.
                    path='/a/b/',
                ),
                RemovePathItem(
                    from_target='t',
                    action=RemovePathAction.assert_exists,
                    path='/a/b/c/e',
                ),
            ], DUMMY_LAYER_OPTS)(subvol)
            self.assertEqual(['(Dir)', {
                'a': ['(Dir)', {}],
                'f': ['(Dir)', {'i': ['(File m444)']}],
            }], render_subvol(subvol))
