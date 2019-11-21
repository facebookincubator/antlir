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


def _pop_and_make_None(d, k, default=RAISE_KEY_ERROR):
    'Like dict.pop, but inserts None into `d` afterwards.'
    v = d.pop(k) if default is RAISE_KEY_ERROR else d.pop(k, default)
    d[k] = None
    return v


class InstallFileItem(metaclass=ImageItem):
    fields = [
        'source',
        'dest',
        'is_executable_',  # None after `customize_fields`
    ] + STAT_OPTION_FIELDS

    def customize_fields(kwargs):  # noqa: B902
        coerce_path_field_normal_relative(kwargs, 'dest')
        customize_stat_options(
            kwargs,
            default_mode=0o555 if _pop_and_make_None(kwargs, 'is_executable_')
                else 0o444,
        )

    def provides(self):
        yield ProvidesFile(path=self.dest)

    def requires(self):
        yield require_directory(os.path.dirname(self.dest))

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        dest = subvol.path(self.dest)
        subvol.run_as_root(['cp', self.source, dest])
        build_stat_options(self, subvol, dest)
