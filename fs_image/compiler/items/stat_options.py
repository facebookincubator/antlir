#!/usr/bin/env python3
'''
Helpers for setting `stat (2)` options on files, directories, etc, which
we are creating inside the image.
'''
from typing import Union

from subvol_utils import Subvol

# `mode` can be an integer fully specifying the bits, or a symbolic
# string like `u+rx`.  In the latter case, the changes are applied on
# top of mode 0.
STAT_OPTION_FIELDS = [('mode', None), ('user_group', None)]

Mode = Union[str, int]  # human-readable, or octal


def customize_stat_options(kwargs, *, default_mode):
    'Mutates `kwargs`.'
    if kwargs.get('mode') is None:
        kwargs['mode'] = default_mode
    if kwargs.get('user_group') is None:
        kwargs['user_group'] = 'root:root'


def mode_to_str(mode: Mode) -> str:
    if isinstance(mode, int):
        return f'{mode:04o}'
    # The symbolic mode must be applied after 0ing all bits.
    return f'a-rwxXst,{mode}'


# Future: this should validate that the user & group actually exist in the
# image's passwd/group databases (blocked on having those be first-class
# objects in the image build process).
def build_stat_options(
    item, subvol: Subvol, full_target_path: str, *, do_not_set_mode=False,
):
    # `chmod` lacks a --no-dereference flag to protect us from following
    # `full_target_path` if it's a symlink.  As far as I know, this should
    # never occur, so just let the exception fly.
    subvol.run_as_root(['test', '!', '-L', full_target_path])
    if do_not_set_mode:
        assert item.mode is None, item
    else:
        # -R is not a problem since it cannot be the case that we are
        # creating a directory that already has something inside it.  On the
        # plus side, it helps with nested directory creation.
        subvol.run_as_root([
            'chmod', '-R', mode_to_str(item.mode), full_target_path,
        ])
    subvol.run_as_root([
        'chown', '--no-dereference', '-R', item.user_group,
        full_target_path,
    ])
