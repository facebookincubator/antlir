#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
`demo_sendstreams.py` performs filesystem operations to generate some
send-stream data, this records the send-stream we would expect to see from
these operations.

Any time you run `demo_sendstreams` with `--update-gold`, you need to update
the constants below.  The actual send-stream may need to change if the
`btrfs send` implementation changes.
'''
import logging
import os

from typing import List, Sequence, Tuple

from . import render_subvols
from .subvolume_utils import InodeRepr

from ..send_stream import (
    get_frequency_of_selinux_xattrs, ItemFilters, SendStreamItem,
    SendStreamItems,
)


# Update these constants to make the tests pass again after running
# `demo_sendstreams` with `--update-gold`.
UUID_CREATE = b'481757f7-6c61-2942-9bea-ee222b120c81'
TRANSID_CREATE = 93993
UUID_MUTATE = b'b5db3896-faf8-b44e-a952-c02f903ab445'
TRANSID_MUTATE = 93996
# Take a `oNUM-NUM-NUM` file from the send-stream, and use the middle number.
TEMP_PATH_MIDDLES = {'create_ops': 93991, 'mutate_ops': 93995}
# I have never seen this initial value change. First number in `oN-N-N`.
TEMP_PATH_COUNTER = 256

# We have a 56KB file, and `btrfs send` emits 48KB writes.
FILE_SZ1 = 48 * 1024
FILE_SZ2 = 8 * 1024
FILE_SZ = FILE_SZ1 + FILE_SZ2


def get_filtered_and_expected_items(
    items: Sequence[SendStreamItem],
    build_start_time: float, build_end_time: float,
    # A toggle for the couple of small differences between the ground truth
    # in the binary send-stream, and the output of `btrfs receive --dump`,
    # which `parse_dump` cannot correct.
    *, dump_mode: bool
) -> Tuple[List[SendStreamItem], List[SendStreamItem]]:

    # Our test program does not touch the SELinux context, so if it's
    # set, it will be set to the default, and we can just filter out the
    # most frequent value.  We don't want to drop all SELinux attributes
    # blindly because having varying contexts suggests something broken
    # about the test or our environment.
    selinux_freqs = get_frequency_of_selinux_xattrs(items)
    assert len(selinux_freqs) > 0  # Our `gold` has SELinux attrs
    max_name, _count = max(selinux_freqs.items(), key=lambda p: p[1])
    logging.info(f'This test ignores SELinux xattrs set to {max_name}')
    filtered_items = items
    filtered_items = ItemFilters.selinux_xattr(
        filtered_items,
        discard_fn=lambda _path, ctx: ctx == max_name,
    )
    filtered_items = ItemFilters.normalize_utimes(
        filtered_items, start_time=build_start_time, end_time=build_end_time,
    )
    filtered_items = list(filtered_items)

    di = SendStreamItems

    def p(p):
        # forgive missing `b`s, it's a test
        return os.path.normpath(p.encode() if isinstance(p, str) else p)

    def chown(path):
        return di.chown(path=p(path), gid=0, uid=0)

    def chmod(path, mode=0o644):
        return di.chmod(path=p(path), mode=mode)

    def utimes(path):
        return di.utimes(
            path=p(path),
            atime=build_start_time,
            mtime=build_start_time,
            ctime=build_start_time,
        )

    def base_metadata(path, mode=0o644):
        return [chown(path), chmod(path, mode), utimes(path)]

    # Future: if we end up doing a lot of mid-list insertions, we can
    # autogenerate the temporary names to match what btrfs does.
    def and_rename(item, real_name, utimes_parent=True):
        yield item
        renamed_item = di.rename(
            path=item.path,
            dest=p(
                os.path.join(os.path.dirname(bytes(item.path)), real_name)
            ),
        )
        yield renamed_item
        if utimes_parent:  # Rarely, `btrfs send` breaks the pattern.
            yield utimes(os.path.dirname(bytes(renamed_item.dest)))

    def temp_path(prefix):
        global TEMP_PATH_COUNTER
        TEMP_PATH_COUNTER += 1
        return p(f'o{TEMP_PATH_COUNTER}-{TEMP_PATH_MIDDLES[prefix]}-0')

    def write(path, *, offset: int, data: bytes):
        if dump_mode:
            return di.update_extent(path=p(path), offset=offset, len=len(data))
        return di.write(path=p(path), offset=offset, data=data)

    return filtered_items, [
        di.subvol(
            path=p('create_ops'), uuid=UUID_CREATE, transid=TRANSID_CREATE,
        ),
        *base_metadata('.', mode=0o755),

        *and_rename(di.mkdir(path=temp_path('create_ops')), b'hello'),
        di.set_xattr(
            path=p('hello'), name=b'user.test_attr', data=b'chickens',
        ),
        *base_metadata('hello', mode=0o755),

        *and_rename(di.mkdir(path=temp_path('create_ops')), b'dir_to_remove'),
        *base_metadata('dir_to_remove', mode=0o755),

        *and_rename(
            di.mkfile(path=temp_path('create_ops')), b'goodbye',
            utimes_parent=False,
        ),
        di.link(path=p('hello/world'), dest=p('goodbye')),
        utimes('.'),
        utimes('hello'),
        *base_metadata('goodbye'),

        *and_rename(di.mknod(
            path=temp_path('create_ops'), mode=0o60600, dev=0x7a539b7,
        ), b'buffered'),
        *base_metadata('buffered', mode=0o600),

        *and_rename(di.mknod(
            path=temp_path('create_ops'), mode=0o20644, dev=0x7a539b7,
        ), b'unbuffered'),
        *base_metadata('unbuffered'),

        *and_rename(di.mkfifo(path=temp_path('create_ops')), b'fifo'),
        *base_metadata('fifo'),

        *and_rename(
            di.mksock(path=temp_path('create_ops')), b'unix_sock',
        ),
        *base_metadata('unix_sock', mode=0o755),

        *and_rename(di.symlink(
            path=temp_path('create_ops'), dest=b'hello/world',
        ), b'bye_symlink'),
        chown('bye_symlink'),
        utimes('bye_symlink'),

        *and_rename(
            di.mkfile(path=temp_path('create_ops')), b'56KB_nuls',
        ),
        write('56KB_nuls', offset=0, data=b'\0' * FILE_SZ1),
        write('56KB_nuls', offset=FILE_SZ1, data=b'\0' * FILE_SZ2),
        *base_metadata('56KB_nuls'),

        *and_rename(
            di.mkfile(path=temp_path('create_ops')), b'56KB_nuls_clone',
        ),
        di.clone(
            path=p('56KB_nuls_clone'), offset=0, len=FILE_SZ,
            from_uuid=b'' if dump_mode else UUID_CREATE,
            from_transid=b'' if dump_mode else TRANSID_CREATE,
            from_path=p('56KB_nuls'), clone_offset=0,
        ),
        *base_metadata('56KB_nuls_clone'),

        *and_rename(
            di.mkfile(path=temp_path('create_ops')), b'zeros_hole_zeros',
        ),
        write('zeros_hole_zeros', offset=0, data=b'\0' * 16384),
        write('zeros_hole_zeros', offset=32768, data=b'\0' * 16384),
        *base_metadata('zeros_hole_zeros'),

        *and_rename(
            di.mkfile(path=temp_path('create_ops')), b'hello_big_hole',
        ),
        write('hello_big_hole', offset=0, data=b'hello\n' + b'\0' * 4090),
        di.truncate(path=p('hello_big_hole'), size=2**30),
        *base_metadata('hello_big_hole', mode=0o644),

        di.snapshot(
            path=p('mutate_ops'),
            uuid=UUID_MUTATE,
            transid=TRANSID_MUTATE,
            parent_uuid=UUID_CREATE,
            parent_transid=TRANSID_CREATE,
        ),
        utimes('.'),
        di.rename(path=p('hello'), dest=p('hello_renamed')),
        utimes('.'),
        utimes('.'),  # `btrfs send` is not so parsimonious

        di.remove_xattr(path=p('hello_renamed'), name=b'user.test_attr'),
        utimes('hello_renamed'),

        di.rmdir(path=p('dir_to_remove')),
        utimes('.'),

        di.link(path=p('farewell'), dest=p('goodbye')),
        di.unlink(path=p('goodbye')),
        di.unlink(path=p('hello_renamed/world')),
        utimes('.'),
        utimes('.'),
        utimes('hello_renamed'),
        utimes('farewell'),

        di.truncate(path=p('hello_big_hole'), size=2),
        utimes('hello_big_hole'),

        *and_rename(
            di.mkfile(path=temp_path('mutate_ops')), b'hello_renamed/een',
        ),
        # Not using `write` since we pass `--no-data` for `mutate_ops`.
        di.update_extent(path=p('hello_renamed/een'), offset=0, len=5),
        *base_metadata('hello_renamed/een'),
    ]


def render_demo_subvols(*, create_ops=None, mutate_ops=None):
    '''
    Test-friendly renderings of the subvolume contents that should be
    produced by the commands in `demo_sendstreams.py`.

    Set the `{create,mutate}_ops` kwargs to None to exclude that subvolume
    from the rendering.  Otherwise, they specify a name for that subvol.

    Read carefully: the return type depends on the args!
    '''

    # For ease of maintenance, keep the subsequent filesystem views in
    # the order that `demo_sendstreams.py` performs the operations.

    goodbye_world = InodeRepr('(File)')  # This empty file gets hardlinked

    def render_create_ops(kb_nuls, kb_nuls_clone, zeros_holes_zeros, big_hole):
        return render_subvols.expected_rendering(['(Dir)', {
            'hello': ["(Dir x'user.test_attr'='chickens')", {
                'world': [goodbye_world],
            }],
            'dir_to_remove': ['(Dir)', {}],
            'buffered': [f'(Block m600 {os.makedev(1337, 31415):x})'],
            'unbuffered': [f'(Char {os.makedev(1337, 31415):x})'],
            'fifo': ['(FIFO)'],
            'unix_sock': ['(Sock m755)'],  # default mode for sockets
            'goodbye': [goodbye_world],
            'bye_symlink': ['(Symlink hello/world)'],
            '56KB_nuls': [f'(File d{FILE_SZ}({kb_nuls}))'],
            '56KB_nuls_clone': [f'(File d{FILE_SZ}({kb_nuls_clone}))'],
            'zeros_hole_zeros': [f'(File {zeros_holes_zeros})'],
            # We have 6 bytes of data, but holes are block-aligned
            'hello_big_hole': [f'(File d4096{big_hole}h1073737728)'],
        }])

    def render_mutate_ops(kb_nuls, kb_nuls_clone, zeros_holes_zeros, big_hole):
        return render_subvols.expected_rendering(['(Dir)', {
            'hello_renamed': ['(Dir)', {"een": ['(File d5)']}],
            'buffered': [f'(Block m600 {os.makedev(1337, 31415):x})'],
            'unbuffered': [f'(Char {os.makedev(1337, 31415):x})'],
            'fifo': ['(FIFO)'],
            'unix_sock': ['(Sock m755)'],  # default mode for sockets
            'farewell': [goodbye_world],
            'bye_symlink': ['(Symlink hello/world)'],
            '56KB_nuls': [f'(File d{FILE_SZ}({kb_nuls}))'],
            '56KB_nuls_clone': [f'(File d{FILE_SZ}({kb_nuls_clone}))'],
            'zeros_hole_zeros': [f'(File {zeros_holes_zeros})'],
            # This got truncated to 2 bytes.
            'hello_big_hole': [f'(File d2{big_hole})'],
        }])

    # These ChunkClones get repeated a lot below.
    #
    # Future: note that in theory, the adjacent ChunkClones could be merged
    # into a single 56KB clone.  Doing this generically, for all possible
    # sequences of writes and clones requires some decently complex code.
    # Doing this specifically for the case of "make a bunch of adjacent
    # writes, then make clones" is probably easier.  But either approach
    # requires work, and the cost of uglier subvolume rendering is currently
    # too low to bother.  When it does bite us, we can fix it.
    create = (
        f'{create_ops}@56KB_nuls:0+{FILE_SZ1}@0/'
        f'{create_ops}@56KB_nuls:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}'
    )
    create_clone = (
        f'{create_ops}@56KB_nuls_clone:0+{FILE_SZ1}@0/'
        f'{create_ops}@56KB_nuls_clone:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}'
    )
    mutate = (
        f'{mutate_ops}@56KB_nuls:0+{FILE_SZ1}@0/'
        f'{mutate_ops}@56KB_nuls:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}'
    )
    mutate_clone = (
        f'{mutate_ops}@56KB_nuls_clone:0+{FILE_SZ1}@0/'
        f'{mutate_ops}@56KB_nuls_clone:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}'
    )
    if create_ops and mutate_ops:
        # Rendering both subvolumes together shows all the clones.
        return {
            create_ops: render_create_ops(
                kb_nuls=f'{create_clone}/{mutate}/{mutate_clone}',
                kb_nuls_clone=f'{create}/{mutate}/{mutate_clone}',
                zeros_holes_zeros=(
                    f'd16384({mutate_ops}@zeros_hole_zeros:0+16384@0)'
                    f'h16384({mutate_ops}@zeros_hole_zeros:16384+16384@0)'
                    f'd16384({mutate_ops}@zeros_hole_zeros:32768+16384@0)'
                ),
                big_hole=f'({mutate_ops}@hello_big_hole:0+2@0)',
            ),
            mutate_ops: render_mutate_ops(
                kb_nuls=f'{create}/{create_clone}/{mutate_clone}',
                kb_nuls_clone=f'{create}/{create_clone}/{mutate}',
                zeros_holes_zeros=(
                    f'd16384({create_ops}@zeros_hole_zeros:0+16384@0)'
                    f'h16384({create_ops}@zeros_hole_zeros:16384+16384@0)'
                    f'd16384({create_ops}@zeros_hole_zeros:32768+16384@0)'
                ),
                big_hole=f'({create_ops}@hello_big_hole:0+2@0)',
            ),
        }
    elif create_ops:
        return render_create_ops(
            kb_nuls=create_clone,
            kb_nuls_clone=create,
            zeros_holes_zeros='d16384h16384d16384',
            big_hole='',
        )
    elif mutate_ops:
        # This single-subvolume render of `mutate_ops` doesn't show the fact
        # that all data was cloned from `create_ops`.
        return render_mutate_ops(
            kb_nuls=mutate_clone,
            kb_nuls_clone=mutate,
            zeros_holes_zeros='d16384h16384d16384',
            big_hole='',
        )
    raise AssertionError('Set at least one of {create,mutate}_ops')
