#!/usr/bin/env python3
import sys

from compiler.provides import ProvidesDirectory, ProvidesFile
from compiler.requires import require_directory, require_file
from tests.temp_subvolumes import TempSubvolumes

from ..install_file import InstallFileItem
from ..symlink import SymlinkToDirItem, SymlinkToFileItem

from .common import BaseItemTestCase, DUMMY_LAYER_OPTS, render_subvol


class SymlinkItemsTestCase(BaseItemTestCase):

    def test_symlink(self):
        self._check_item(
            SymlinkToDirItem(from_target='t', source='x', dest='y'),
            {ProvidesDirectory(path='y')},
            {require_directory('/'), require_directory('/x')},
        )

        self._check_item(
            SymlinkToFileItem(
                from_target='t', source='source_file', dest='dest_symlink'
            ),
            {ProvidesFile(path='dest_symlink')},
            {require_directory('/'), require_file('/source_file')},
        )

    def test_symlink_command(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create('tar-sv')
            subvol.run_as_root(['mkdir', subvol.path('dir')])

            # We need a source file to validate a SymlinkToFileItem
            InstallFileItem(
                from_target='t', source='/dev/null', dest='/file',
                is_executable_=False,
            ).build(subvol, DUMMY_LAYER_OPTS)
            SymlinkToDirItem(
                from_target='t', source='/dir', dest='/dir_symlink'
            ).build(subvol, DUMMY_LAYER_OPTS)
            SymlinkToFileItem(
                from_target='t', source='file', dest='/file_symlink'
            ).build(subvol, DUMMY_LAYER_OPTS)

            self.assertEqual(['(Dir)', {
                'dir': ['(Dir)', {}],
                'dir_symlink': ['(Symlink /dir)'],
                'file': ['(File m444)'],
                'file_symlink': ['(Symlink /file)'],
            }], render_subvol(subvol))
