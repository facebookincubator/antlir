#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
The items below are an in-memory representation of a btrfs send-stream.

We have code to parse these both from the binary send-stream, and from the
output of `btrfs receive --dump`.  The latter parse is imperfect and has a
number of limitations, but we find it useful for testing -- refer to the
`parse_dump.py` docblock.
'''
from collections import Counter
from typing import Callable, Iterable

from compiler.enriched_namedtuple import metaclass_new_enriched_namedtuple

_SELINUX_XATTR = b'security.selinux'


class SendStreamItem(type):
    'Metaclass for the btrfs sendstream commands.'
    def __new__(metacls, classname, bases, dct):
        return metaclass_new_enriched_namedtuple(
            __class__,
            # This is relative to the subvolume directory, except for the
            # subvolume-making commands `subvol` and `snapshot`, where it
            # **is** the name of the subvolume directory.
            ['path'],
            metacls, classname, bases, {'sets_subvol_name': False, **dct},
        )


class SendStreamItems:
    '''
    This class only exists to group its inner classes.

    This items should exactly match the content of `read_and_process_cmd` in
    https://github.com/kdave/btrfs-progs/blob/master/send-stream.c

    The field naming follows `--dump`, cf. `btrfs_print_send_ops`
      https://github.com/kdave/btrfs-progs/blob/master/send-dump.c
    The one exception is that `from` in `clone` became `from_path` since
    `from` is reserved in Python.
    '''

    #
    # operations making new subvolumes
    #

    class subvol(metaclass=SendStreamItem):
        fields = ['uuid', 'transid']
        sets_subvol_name = True

    class snapshot(metaclass=SendStreamItem):
        fields = ['uuid', 'transid', 'parent_uuid', 'parent_transid']
        sets_subvol_name = True

    #
    # operations making new inodes
    #

    class mkfile(metaclass=SendStreamItem):
        pass

    class mkdir(metaclass=SendStreamItem):
        pass

    class mknod(metaclass=SendStreamItem):
        fields = ['mode', 'dev']

    class mkfifo(metaclass=SendStreamItem):
        pass

    class mksock(metaclass=SendStreamItem):
        pass

    class symlink(metaclass=SendStreamItem):
        fields = ['dest']

    #
    # operations on the path -> inode mapping
    #

    class rename(metaclass=SendStreamItem):
        fields = ['dest']

    class link(metaclass=SendStreamItem):
        # WATCH OUT: This `dest` does not mean what you think it means.  We
        # will create a hardlink from `dest` to `path`.  So the `dest` the
        # destination of the new link being created.  Awkward!  This
        # unfortunate naming was borrowed from `btrfs receive --dump`.
        fields = ['dest']

    class unlink(metaclass=SendStreamItem):
        pass

    class rmdir(metaclass=SendStreamItem):
        pass

    #
    # per-inode operations
    #

    class write(metaclass=SendStreamItem):
        fields = ['offset', 'data']

    class clone(metaclass=SendStreamItem):
        fields = [
            'offset',
            'len',
            'from_uuid',  # `btrfs receive --dump` does NOT display this :/
            'from_transid',  # ... nor this.
            'from_path',
            'clone_offset',
        ]

    class set_xattr(metaclass=SendStreamItem):
        fields = ['name', 'data']

    class remove_xattr(metaclass=SendStreamItem):
        fields = ['name']

    class truncate(metaclass=SendStreamItem):
        fields = ['size']

    class chmod(metaclass=SendStreamItem):
        fields = ['mode']

    class chown(metaclass=SendStreamItem):
        fields = ['gid', 'uid']

    class utimes(metaclass=SendStreamItem):
        fields = ['atime', 'mtime', 'ctime']

    # Just like `write` but with no data, used by `btrfs send --no-data`.
    class update_extent(metaclass=SendStreamItem):
        fields = ['offset', 'len']


def get_frequency_of_selinux_xattrs(items):
    'Returns {"xattr_value": <count>}. Useful for ItemFilters.selinux_xattr.'
    counter = Counter()
    for item in items:
        if isinstance(item, SendStreamItems.set_xattr):
            if item.name == _SELINUX_XATTR:
                counter[item.data] += 1
    return counter


class ItemFilters:
    '''
    A namespace of filters for taking a just-parsed Iterable[SendStreamItems],
    and making it useful for filesystem testing.
    '''

    @staticmethod
    def selinux_xattr(
        items: Iterable[SendStreamItem],
        discard_fn: Callable[[bytes, bytes], bool],
    ) -> Iterable[SendStreamItem]:
        '''
        SELinux always sets a security context on filesystem objects, but most
        images will not ship data with non-default contexts, so it is easiest to
        just filter out these `set_xattr`s
        '''
        for item in items:
            if isinstance(item, SendStreamItems.set_xattr):
                if (
                    item.name == _SELINUX_XATTR and
                    discard_fn(item.path, item.data)
                ):
                    continue
            yield item

    @staticmethod
    def normalize_utimes(
        items: Iterable[SendStreamItem],
        start_time: float,
        end_time: float,
    ) -> Iterable[SendStreamItem]:
        '''
        Build-time timestamps will vary, since the build takes some time.
        We can make them predictable by replacing any timestamp within the
        build time-range by `start_time`.
        '''
        def normalize_time(t):
            return start_time if start_time <= t <= end_time else t

        for item in items:
            if isinstance(item, SendStreamItems.utimes):
                yield type(item)(
                    path=item.path,
                    atime=normalize_time(item.atime),
                    mtime=normalize_time(item.mtime),
                    ctime=normalize_time(item.ctime),
                )
            else:
                yield item
