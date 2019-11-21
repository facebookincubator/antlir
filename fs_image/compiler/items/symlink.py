#!/usr/bin/env python3
import os

from subvol_utils import Subvol

from compiler.provides import ProvidesDirectory, ProvidesFile
from compiler.requires import require_directory, require_file

from .common import (
    coerce_path_field_normal_relative, ImageItem, LayerOpts,
    make_path_normal_relative,
)


def _make_rsync_style_dest_path(dest: str, source: str) -> str:
    '''
    rsync convention for a destination: "ends/in/slash/" means "write
    into this directory", "does/not/end/with/slash" means "write with
    the specified filename".
    '''

    # Normalize after applying the rsync convention, since this would
    # remove any trailing / in 'dest'.
    return make_path_normal_relative(
        os.path.join(dest,
            os.path.basename(source)) if dest.endswith('/') else dest
    )


class SymlinkBase:
    __slots__ = ()
    fields = ['source', 'dest']

    def _customize_fields_impl(kwargs):  # noqa: B902
        coerce_path_field_normal_relative(kwargs, 'source')

        kwargs['dest'] = _make_rsync_style_dest_path(
            kwargs['dest'], kwargs['source']
        )

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        dest = subvol.path(self.dest)
        # Source is always absolute inside the image subvolume
        source = os.path.join('/', self.source)
        subvol.run_as_root(
            ['ln', '--symbolic', '--no-dereference', source, dest]
        )


class SymlinkToDirItem(SymlinkBase, metaclass=ImageItem):
    customize_fields = SymlinkBase._customize_fields_impl

    def provides(self):
        yield ProvidesDirectory(path=self.dest)

    def requires(self):
        yield require_directory(self.source)
        yield require_directory(os.path.dirname(self.dest))


# We should allow symlinks to certain files that will be in the image
# at runtime but may not be at build time.
def _whitelisted_symlink_source(source: str) -> bool:
    return source in [
        'dev/null',
    ]


class SymlinkToFileItem(SymlinkBase, metaclass=ImageItem):
    customize_fields = SymlinkBase._customize_fields_impl

    def provides(self):
        yield ProvidesFile(path=self.dest)

    def requires(self):
        if not _whitelisted_symlink_source(self.source):
            yield require_file(self.source)
        yield require_directory(os.path.dirname(self.dest))
