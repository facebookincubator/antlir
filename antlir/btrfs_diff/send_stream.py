#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The items below are an in-memory representation of a btrfs send-stream.

We have code to parse these both from the binary send-stream, and from the
output of `btrfs receive --dump`.  The latter parse is imperfect and has a
number of limitations, but we find it useful for testing -- refer to the
`parse_dump.py` docblock.
"""
import re
from collections import Counter
from dataclasses import dataclass
from typing import Callable, ClassVar, Iterable, Tuple


_SELINUX_XATTR = b"security.selinux"


@dataclass(frozen=True)
class SendStreamItem:
    path: bytes
    # This is a constant attribute, so we set it up with init=False, since it
    # won't ever be set from the constructor.
    #
    # Using init=False is necessary, because when we have subclasses defining
    # their own attributes, the subclass attributes may not follow attributes
    # with a default. Using init=False avoids that issue altogether.
    sets_subvol_name: ClassVar[bool] = False

    def __str__(self) -> str:
        # The default __repr__ of a dataclass includes the __qualname__ of the
        # class, which in our case includes the outer class SendStreamItems.
        #
        # This behavior is different from namedtuple, which includes only
        # __name__ as part of the repr.
        #
        # It turns out that we depend on the output of __str__, which we use on
        # exception messages such as "Can only <action> on <object>."
        #
        # We have some tests that match those messages by regex. And it turns
        # out that it actually looks better if the "<action>" is a single word,
        # so let's override the __str__ method here to match the namedtuple
        # format.
        return re.sub(r"^((\w+|<\w+>)\.)*", "", repr(self))


class SendStreamItems:
    """
    This class only exists to group its inner classes.

    This items should exactly match the content of `read_and_process_cmd` in
    https://github.com/kdave/btrfs-progs/blob/master/send-stream.c

    The field naming follows `--dump`, cf. `btrfs_print_send_ops`
      https://github.com/kdave/btrfs-progs/blob/master/send-dump.c
    The one exception is that `from` in `clone` became `from_path` since
    `from` is reserved in Python.
    """

    #
    # operations making new subvolumes
    #

    @dataclass(frozen=True)
    class subvol(SendStreamItem):
        uuid: bytes
        transid: int
        sets_subvol_name: ClassVar[bool] = True

    @dataclass(frozen=True)
    class snapshot(SendStreamItem):
        uuid: bytes
        transid: int
        parent_uuid: bytes
        parent_transid: int
        sets_subvol_name: ClassVar[bool] = True

    #
    # operations making new inodes
    #

    @dataclass(frozen=True)
    class mkfile(SendStreamItem):
        pass

    @dataclass(frozen=True)
    class mkdir(SendStreamItem):
        pass

    @dataclass(frozen=True)
    class mknod(SendStreamItem):
        mode: int
        dev: int

    @dataclass(frozen=True)
    class mkfifo(SendStreamItem):
        pass

    @dataclass(frozen=True)
    class mksock(SendStreamItem):
        pass

    @dataclass(frozen=True)
    class symlink(SendStreamItem):
        dest: bytes

    #
    # operations on the path -> inode mapping
    #

    @dataclass(frozen=True)
    class rename(SendStreamItem):
        dest: bytes

    @dataclass(frozen=True)
    class link(SendStreamItem):
        # WATCH OUT: This `dest` does not mean what you think it means.  We
        # will create a hardlink from `dest` to `path`.  So the `dest` the
        # destination of the new link being created.  Awkward!  This
        # unfortunate naming was borrowed from `btrfs receive --dump`.
        dest: bytes

    @dataclass(frozen=True)
    class unlink(SendStreamItem):
        pass

    @dataclass(frozen=True)
    class rmdir(SendStreamItem):
        pass

    #
    # per-inode operations
    #

    @dataclass(frozen=True)
    class write(SendStreamItem):
        offset: int
        data: bytes

    @dataclass(frozen=True)
    class clone(SendStreamItem):
        offset: int
        len: int
        from_uuid: bytes  # `btrfs receive --dump` does NOT display this :/
        from_transid: bytes  # ... nor this.
        from_path: bytes
        clone_offset: int

    @dataclass(frozen=True)
    class set_xattr(SendStreamItem):
        name: bytes
        data: bytes

    @dataclass(frozen=True)
    class remove_xattr(SendStreamItem):
        name: bytes

    @dataclass(frozen=True)
    class truncate(SendStreamItem):
        size: int

    @dataclass(frozen=True)
    class chmod(SendStreamItem):
        mode: int

    @dataclass(frozen=True)
    class chown(SendStreamItem):
        gid: int
        uid: int

    @dataclass(frozen=True)
    class utimes(SendStreamItem):
        atime: Tuple[int, int]
        mtime: Tuple[int, int]
        ctime: Tuple[int, int]

    # Just like `write` but with no data, used by `btrfs send --no-data`.
    @dataclass(frozen=True)
    class update_extent(SendStreamItem):
        offset: int
        len: int


def get_frequency_of_selinux_xattrs(items):
    'Returns {"xattr_value": <count>}. Useful for ItemFilters.selinux_xattr.'
    counter = Counter()
    for item in items:
        if isinstance(item, SendStreamItems.set_xattr):
            if item.name == _SELINUX_XATTR:
                counter[item.data] += 1
    return counter


class ItemFilters:
    """
    A namespace of filters for taking a just-parsed Iterable[SendStreamItems],
    and making it useful for filesystem testing.
    """

    @staticmethod
    def selinux_xattr(
        items: Iterable[SendStreamItem],
        discard_fn: Callable[[bytes, bytes], bool],
    ) -> Iterable[SendStreamItem]:
        """
        SELinux always sets a security context on filesystem objects, but most
        images will not ship data with non-default contexts, so it is easiest to
        just filter out these `set_xattr`s
        """
        for item in items:
            if isinstance(item, SendStreamItems.set_xattr):
                if item.name == _SELINUX_XATTR and discard_fn(
                    item.path, item.data
                ):
                    continue
            yield item

    @staticmethod
    def normalize_utimes(
        items: Iterable[SendStreamItem], start_time: float, end_time: float
    ) -> Iterable[SendStreamItem]:
        """
        Build-time timestamps will vary, since the build takes some time.
        We can make them predictable by replacing any timestamp within the
        build time-range by `start_time`.
        """

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
