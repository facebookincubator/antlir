#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Helpers for setting `stat (2)` options on files, directories, etc, which
we are creating inside the image.
'''
import os
import pwd

from typing import Union

from fs_image.fs_utils import Path
from fs_image.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from fs_image.nspawn_in_subvol.non_booted import run_non_booted_nspawn
from fs_image.subvol_utils import Subvol

from .common import generate_work_dir

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
        build_appliance=None,
):
    assert full_target_path.startswith(subvol.path()), \
        '{self}: A symlink to {full_target_path} would point outside the image'
    rel_path = os.path.relpath(full_target_path, subvol.path())
    # `chmod` lacks a --no-dereference flag to protect us from following
    # `full_target_path` if it's a symlink.  As far as I know, this should
    # never occur, so just let the exception fly.
    if build_appliance:
        work_dir = generate_work_dir()
        opts = new_nspawn_opts(
            cmd=['test', '!', '-L', Path(work_dir) / rel_path],
            layer=build_appliance,
            bindmount_rw=[(subvol.path(), work_dir)],
            user=pwd.getpwnam('root'),
        )
        run_non_booted_nspawn(opts, PopenArgs())
    else:
        subvol.run_as_root(['test', '!', '-L', full_target_path])
    if do_not_set_mode:
        assert getattr(item, 'mode', None) is None, item
    else:
        # -R is not a problem since it cannot be the case that we are
        # creating a directory that already has something inside it.  On the
        # plus side, it helps with nested directory creation.
        if build_appliance:
            work_dir = generate_work_dir()
            opts = new_nspawn_opts(
                cmd=[
                    'chmod', '-R', mode_to_str(item.mode),
                    Path(work_dir) / rel_path,
                ],
                layer=build_appliance,
                bindmount_rw=[(subvol.path(), work_dir)],
                user=pwd.getpwnam('root'),
            )
            run_non_booted_nspawn(opts, PopenArgs())
        else:
            subvol.run_as_root([
                'chmod', '-R', mode_to_str(item.mode), full_target_path,
            ])
    if build_appliance:
        work_dir = generate_work_dir()
        opts = new_nspawn_opts(
            cmd=[
                'chown', '--no-dereference', '-R', item.user_group,
                Path(work_dir) / rel_path,
            ],
            layer=build_appliance,
            bindmount_rw=[(subvol.path(), work_dir)],
            bindmount_ro=[
                ('/etc/passwd', '/etc/passwd'),
                ('/etc/group', '/etc/group'),
            ],
            user=pwd.getpwnam('root'),
        )
        run_non_booted_nspawn(opts, PopenArgs())
    else:
        subvol.run_as_root([
            'chown', '--no-dereference', '-R', item.user_group,
            full_target_path,
        ])
