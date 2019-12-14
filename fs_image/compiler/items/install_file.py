#!/usr/bin/env python3
import os

from subvol_utils import Subvol

from compiler.provides import ProvidesFile
from compiler.requires import require_directory

from .common import coerce_path_field_normal_relative, ImageItem, LayerOpts
from .stat_options import (
    build_stat_options, customize_stat_options, STAT_OPTION_FIELDS,
)

RAISE_KEY_ERROR = object()


class InstallFileItem(metaclass=ImageItem):
    fields = [
        'source',
        'dest',
    ] + STAT_OPTION_FIELDS

    def customize_fields(kwargs):  # noqa: B902
        coerce_path_field_normal_relative(kwargs, 'dest')
        customize_stat_options(
            kwargs,
            # This tests whether the build repo user can execute the file. 
            # This is a very natural test for build artifacts, and files in
            # the repo.  Note that this can be affected if the ambient umask
            # is pathological, which is why `compiler.py` checks the umask.
            default_mode=0o555
                if os.access(kwargs['source'], os.X_OK) else 0o444,
        )

    def provides(self):
        yield ProvidesFile(path=self.dest)

    def requires(self):
        yield require_directory(os.path.dirname(self.dest))

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        dest = subvol.path(self.dest)
        subvol.run_as_root(['cp', self.source, dest])
        build_stat_options(self, subvol, dest)
