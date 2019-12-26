#!/usr/bin/env python3
from compiler.provides import ProvidesDirectory
from compiler.requires import require_directory

from ..common import ImageItem, image_source_item
from ..install_file import InstallFileItem
from ..make_dirs import MakeDirsItem
from ..make_subvol import FilesystemRootItem, ParentLayerItem

from .common import BaseItemTestCase, DUMMY_LAYER_OPTS


class FakeImageSourceItem(metaclass=ImageItem):
    fields = ['source', 'kitteh']


class ItemsCommonTestCase(BaseItemTestCase):

    def test_image_source_item(self):
        # Cover the `source=None` branch in `image_source_item`.
        it = image_source_item(
            FakeImageSourceItem,
            exit_stack=None,
            layer_opts=DUMMY_LAYER_OPTS,
        )(from_target='m', source=None, kitteh='meow')
        self.assertEqual(
            FakeImageSourceItem(from_target='m', source=None, kitteh='meow'),
            it,
        )
        self.assertIsNone(it.source)
        self.assertEqual('meow', it.kitteh)

    def test_enforce_no_parent_dir(self):
        with self.assertRaisesRegex(AssertionError, r'cannot start with \.\.'):
            InstallFileItem(
                from_target='t', source='/etc/passwd', dest='a/../../b',
            )

    def test_stat_options(self):
        self._check_item(
            MakeDirsItem(
                from_target='t',
                into_dir='x',
                path_to_make='y/z',
                mode=0o733,
                user_group='cat:dog',
            ),
            {ProvidesDirectory(path='x/y'), ProvidesDirectory(path='x/y/z')},
            {require_directory('x')},
        )
