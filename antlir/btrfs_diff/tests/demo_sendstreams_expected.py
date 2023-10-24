#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
`demo_sendstreams.py` performs filesystem operations to generate some
send-stream data, this records the send-stream we would expect to see from
these operations.

Any time you `buck run antlir/btrfs_diff:make-demo-sendstreams` with
`--write-gold-to-dir`, you need to update the constants below.  The actual
send-stream may need to change if the `btrfs send` implementation changes.
"""
import itertools
import logging
import os
from typing import BinaryIO, Dict, Iterable, List, Sequence, Tuple

from antlir.btrfs_diff.parse_dump import parse_btrfs_dump
from antlir.btrfs_diff.send_stream import (
    get_frequency_of_selinux_xattrs,
    ItemFilters,
    SendStreamItem,
    SendStreamItems,
)
from antlir.btrfs_diff.tests import render_subvols
from antlir.btrfs_diff.tests.subvolume_utils import InodeRepr


# Update these constants to make the tests pass again after running
# `demo_sendstreams` with `--update-gold`.
UUID_CREATE = b"58136d43-bc05-444a-a046-0ee20724f0ed"
TRANSID_CREATE = 1798
UUID_MUTATE = b"12667dcf-3f03-834b-b552-966fb4aae237"
TRANSID_MUTATE = 1801
# Take a `oNUM-NUM-NUM` file from the send-stream, and use the middle number.
TEMP_PATH_MIDDLES = {"create_ops": 1796, "mutate_ops": 1800}
# I have never seen this initial value change. First number in `oN-N-N`.
TEMP_PATH_COUNTER_START = 257

# We have a 56KB file, and `btrfs send` emits 48KB writes.
FILE_SZ1: int = 48 * 1024
FILE_SZ2: int = 8 * 1024
FILE_SZ: int = FILE_SZ1 + FILE_SZ2


_dir_with_acls_system_posix_acl = (
    b"\x02\x00\x00\x00\x01\x00\x07\x00\xff\xff\xff\xff\x04\x00\x05\x00\xff"
    b"\xff\xff\xff\x08\x00\x05\x00\x04\x00\x00\x00\x08\x00\x05\x00\n\x00"
    b"\x00\x00\x10\x00\x05\x00\xff\xff\xff\xff \x00\x05\x00\xff\xff\xff\xff"
)


def get_filtered_and_expected_items(
    items: Sequence[SendStreamItem],
    build_start_time: float,
    build_end_time: float,
    # A toggle for the couple of small differences between the ground truth
    # in the binary send-stream, and the output of `btrfs receive --dump`,
    # which `parse_dump` cannot correct.
    *,
    dump_mode: bool,
) -> Tuple[List[SendStreamItem], List[SendStreamItem]]:

    # Our test program does not touch the SELinux context, so if it's
    # set, it will be set to the default, and we can just filter out the
    # most frequent value.  We don't want to drop all SELinux attributes
    # blindly because having varying contexts suggests something broken
    # about the test or our environment.
    selinux_freqs = get_frequency_of_selinux_xattrs(items)
    assert len(selinux_freqs) > 0  # Our `gold` has SELinux attrs
    max_name, _count = max(selinux_freqs.items(), key=lambda p: p[1])
    logging.info(f"This test ignores SELinux xattrs set to {max_name}")
    filtered_items = items
    filtered_items = ItemFilters.selinux_xattr(
        filtered_items, discard_fn=lambda _path, ctx: ctx == max_name
    )
    filtered_items = ItemFilters.normalize_utimes(
        filtered_items, start_time=build_start_time, end_time=build_end_time
    )
    filtered_items = list(filtered_items)

    # In theory we never create more than ~10 temp paths but there's no
    # harm in letting this just grow forever.
    temp_path_counter = itertools.count(TEMP_PATH_COUNTER_START)

    di = SendStreamItems

    def p(p):
        # forgive missing `b`s, it's a test
        return os.path.normpath(p.encode() if isinstance(p, str) else p)

    def chown(path, gid=0, uid=0):
        return di.chown(path=p(path), gid=gid, uid=uid)

    def chmod(path, mode=0o644):
        return di.chmod(path=p(path), mode=mode)

    def utimes(path):
        return di.utimes(
            path=p(path),
            atime=build_start_time,
            mtime=build_start_time,
            ctime=build_start_time,
        )

    def base_metadata(path, mode=0o644, gid=0, uid=0):
        return [chown(path, gid, uid), chmod(path, mode), utimes(path)]

    # Future: if we end up doing a lot of mid-list insertions, we can
    # autogenerate the temporary names to match what btrfs does.
    def and_rename(item, real_name, utimes_parent=True):
        yield item
        renamed_item = di.rename(
            path=item.path,
            dest=p(os.path.join(os.path.dirname(bytes(item.path)), real_name)),
        )
        yield renamed_item
        if utimes_parent:  # Rarely, `btrfs send` breaks the pattern.
            yield utimes(os.path.dirname(bytes(renamed_item.dest)))

    def temp_path(prefix):
        return p(f"o{next(temp_path_counter)}-" f"{TEMP_PATH_MIDDLES[prefix]}-0")

    def write(path, *, offset: int, data: bytes):
        if dump_mode:
            return di.update_extent(path=p(path), offset=offset, len=len(data))
        return di.write(path=p(path), offset=offset, data=data)

    return (
        filtered_items,
        [
            di.subvol(path=p("create_ops"), uuid=UUID_CREATE, transid=TRANSID_CREATE),
            *base_metadata(".", mode=0o755),
            *and_rename(di.mkdir(path=temp_path("create_ops")), b"hello"),
            di.set_xattr(path=p("hello"), name=b"user.test_attr", data=b"chickens"),
            *base_metadata("hello", mode=0o755),
            *and_rename(di.mkdir(path=temp_path("create_ops")), b"dir_to_remove"),
            *base_metadata("dir_to_remove", mode=0o755),
            *and_rename(
                di.mkfile(path=temp_path("create_ops")),
                b"goodbye",
                utimes_parent=False,
            ),
            di.link(path=p("hello/world"), dest=p("goodbye")),
            utimes("."),
            utimes("hello"),
            *base_metadata("goodbye"),
            *and_rename(
                di.mknod(
                    path=temp_path("create_ops"),
                    mode=0o60600,
                    dev=os.makedev(42, 31),
                ),
                b"buffered",
            ),
            *base_metadata("buffered", mode=0o600),
            *and_rename(
                di.mknod(
                    path=temp_path("create_ops"),
                    mode=0o20644,
                    dev=os.makedev(42, 31),
                ),
                b"unbuffered",
            ),
            *base_metadata("unbuffered"),
            *and_rename(di.mkfifo(path=temp_path("create_ops")), b"fifo"),
            *base_metadata("fifo"),
            *and_rename(di.mksock(path=temp_path("create_ops")), b"unix_sock"),
            *base_metadata("unix_sock", mode=0o755),
            *and_rename(
                di.symlink(path=temp_path("create_ops"), dest=b"hello/world"),
                b"bye_symlink",
            ),
            chown("bye_symlink"),
            utimes("bye_symlink"),
            *and_rename(di.mkfile(path=temp_path("create_ops")), b"56KB_nuls"),
            write("56KB_nuls", offset=0, data=b"\0" * FILE_SZ1),
            write("56KB_nuls", offset=FILE_SZ1, data=b"\0" * FILE_SZ2),
            *base_metadata("56KB_nuls"),
            *and_rename(di.mkfile(path=temp_path("create_ops")), b"56KB_nuls_clone"),
            di.clone(
                path=p("56KB_nuls_clone"),
                offset=0,
                len=FILE_SZ,
                from_uuid=b"" if dump_mode else UUID_CREATE,
                # pyre-fixme[6]: Expected `bytes` for 5th param but got
                # `Union[bytes, int]`.
                from_transid=b"" if dump_mode else TRANSID_CREATE,
                from_path=p("56KB_nuls"),
                clone_offset=0,
            ),
            *base_metadata("56KB_nuls_clone"),
            *and_rename(di.mkfile(path=temp_path("create_ops")), b"zeros_hole_zeros"),
            write("zeros_hole_zeros", offset=0, data=b"\0" * 16384),
            write("zeros_hole_zeros", offset=32768, data=b"\0" * 16384),
            *base_metadata("zeros_hole_zeros"),
            *and_rename(di.mkfile(path=temp_path("create_ops")), b"hello_big_hole"),
            write("hello_big_hole", offset=0, data=b"hello\n" + b"\0" * 4090),
            di.truncate(path=p("hello_big_hole"), size=2**30),
            *base_metadata("hello_big_hole", mode=0o644),
            *and_rename(di.mkfile(path=temp_path("create_ops")), b"selinux_xattrs"),
            *base_metadata("selinux_xattrs"),
            *and_rename(di.mkdir(path=temp_path("create_ops")), b"dir_perms_0500"),
            *base_metadata("dir_perms_0500", mode=0o500),
            *and_rename(di.mkdir(path=temp_path("create_ops")), b"user1"),
            *base_metadata("user1", mode=0o700, gid=1, uid=1),
            *and_rename(di.mkfile(path=temp_path("create_ops")), b"user1/data"),
            *base_metadata("user1/data", mode=0o400, gid=1, uid=1),
            *and_rename(di.mkdir(path=temp_path("create_ops")), b"dir_with_acls"),
            di.set_xattr(
                path=p("dir_with_acls"),
                name=b"system.posix_acl_default",
                data=_dir_with_acls_system_posix_acl,
            ),
            di.set_xattr(
                path=p("dir_with_acls"),
                name=b"system.posix_acl_access",
                data=_dir_with_acls_system_posix_acl,
            ),
            *base_metadata("dir_with_acls", mode=0o755),
            di.snapshot(
                path=p("mutate_ops"),
                uuid=UUID_MUTATE,
                transid=TRANSID_MUTATE,
                parent_uuid=UUID_CREATE,
                parent_transid=TRANSID_CREATE,
            ),
            utimes("."),
            di.rename(path=p("hello"), dest=p("hello_renamed")),
            utimes("."),
            utimes("."),  # `btrfs send` is not so parsimonious
            di.remove_xattr(path=p("hello_renamed"), name=b"user.test_attr"),
            utimes("hello_renamed"),
            di.rmdir(path=p("dir_to_remove")),
            utimes("."),
            di.link(path=p("farewell"), dest=p("goodbye")),
            di.unlink(path=p("goodbye")),
            di.unlink(path=p("hello_renamed/world")),
            utimes("."),
            utimes("."),
            utimes("hello_renamed"),
            utimes("farewell"),
            di.truncate(path=p("hello_big_hole"), size=2),
            utimes("hello_big_hole"),
            *and_rename(di.mkfile(path=temp_path("mutate_ops")), b"hello_renamed/een"),
            di.set_xattr(
                path=p("hello_renamed/een"),
                name=b"btrfs.compression",
                data=b"zlib",
            ),
            # Not using `write` since we pass `--no-data` for `mutate_ops`.
            di.update_extent(path=p("hello_renamed/een"), offset=0, len=5),
            *base_metadata("hello_renamed/een"),
        ],
    )


def render_demo_subvols(*, create_ops=None, mutate_ops=None, lossy_packaging=None):
    """
    Test-friendly renderings of the subvolume contents that should be
    produced by the commands in `demo_sendstreams.py`.

    Set the `{create,mutate}_ops` kwargs to None to exclude that subvolume
    from the rendering.  Otherwise, they specify a name for that subvol.

    Read carefully: the return type depends on the args!
    """
    assert lossy_packaging in [None, "tar", "cpio"], lossy_packaging

    # For ease of maintenance, keep the subsequent filesystem views in
    # the order that `demo_sendstreams.py` performs the operations.

    goodbye_world = InodeRepr("(File)")  # This empty file gets hardlinked

    def render_reflink(s: str) -> str:
        return "" if lossy_packaging else f"({s})"

    def render_create_ops(kb_nuls, kb_nuls_clone, zeros_holes_zeros, big_hole):
        kb_nuls = render_reflink(kb_nuls)
        kb_nuls_clone = render_reflink(kb_nuls_clone)
        file_mode = "d" if lossy_packaging != "cpio" else "h"
        return render_subvols.expected_rendering(
            [
                "(Dir)",
                {
                    "hello": [
                        ("(Dir x'user.test_attr'='chickens')")
                        if lossy_packaging != "cpio"
                        else "(Dir)",
                        {"world": [goodbye_world]},
                    ],
                    "dir_to_remove": ["(Dir)", {}],
                    "buffered": [f"(Block m600 {os.makedev(42, 31):x})"],
                    "unbuffered": [f"(Char {os.makedev(42, 31):x})"],
                    "fifo": ["(FIFO)"],
                    **(
                        {"unix_sock": ["(Sock m755)"]} if not lossy_packaging else {}
                    ),  # default mode for sockets
                    "user1": [
                        "(Dir m700 o1:1)",
                        {"data": ["(File m400 o1:1)"]},
                    ],
                    "dir_with_acls": [
                        (
                            "(Dir x"
                            "'system.posix_acl_default'='{acl}',"
                            "'system.posix_acl_access'='{acl}'"
                            ")"
                        ).format(
                            acl=_dir_with_acls_system_posix_acl.decode(
                                "ASCII", "surrogateescape"
                            )
                            .encode("unicode-escape")
                            .decode("ISO-8859-1")
                        )
                        if lossy_packaging != "cpio"
                        else "(Dir)",
                        {},
                    ],
                    "goodbye": [goodbye_world],
                    "bye_symlink": ["(Symlink hello/world)"],
                    "dir_perms_0500": ["(Dir m500)", {}],
                    "56KB_nuls": [f"(File {file_mode}{FILE_SZ}{kb_nuls})"],
                    "56KB_nuls_clone": [f"(File {file_mode}{FILE_SZ}{kb_nuls_clone})"],
                    "zeros_hole_zeros": [f"(File {zeros_holes_zeros})"],
                    # We have 6 bytes of data, but holes are block-aligned
                    "hello_big_hole": [f"(File d4096{big_hole}h1073737728)"],
                    "selinux_xattrs": ["(File)"],
                },
            ]
        )

    def render_mutate_ops(kb_nuls, kb_nuls_clone, zeros_holes_zeros, big_hole):
        kb_nuls = render_reflink(kb_nuls)
        kb_nuls_clone = render_reflink(kb_nuls_clone)
        return render_subvols.expected_rendering(
            [
                "(Dir)",
                {
                    "hello_renamed": [
                        "(Dir)",
                        {"een": ["(File x'btrfs.compression'='zstd' d5)"]},
                    ],
                    "buffered": [f"(Block m600 {os.makedev(42, 31):x})"],
                    "unbuffered": [f"(Char {os.makedev(42, 31):x})"],
                    "fifo": ["(FIFO)"],
                    **(
                        {"unix_sock": ["(Sock m755)"]} if not lossy_packaging else {}
                    ),  # default mode for sockets
                    "user1": [
                        "(Dir m700 o1:1)",
                        {"data": ["(File m400 o1:1)"]},
                    ],
                    "farewell": [goodbye_world],
                    "bye_symlink": ["(Symlink hello/world)"],
                    "dir_perms_0500": ["(Dir m500)", {}],
                    "56KB_nuls": [f"(File d{FILE_SZ}{kb_nuls})"],
                    "56KB_nuls_clone": [f"(File d{FILE_SZ}{kb_nuls_clone})"],
                    "zeros_hole_zeros": [f"(File {zeros_holes_zeros})"],
                    # This got truncated to 2 bytes.
                    "hello_big_hole": [f"(File d2{big_hole})"],
                    "selinux_xattrs": ["(File)"],
                },
            ]
        )

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
        f"{create_ops}@56KB_nuls:0+{FILE_SZ1}@0/"
        f"{create_ops}@56KB_nuls:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}"
    )
    create_clone = (
        f"{create_ops}@56KB_nuls_clone:0+{FILE_SZ1}@0/"
        f"{create_ops}@56KB_nuls_clone:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}"
    )
    mutate = (
        f"{mutate_ops}@56KB_nuls:0+{FILE_SZ1}@0/"
        f"{mutate_ops}@56KB_nuls:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}"
    )
    mutate_clone = (
        f"{mutate_ops}@56KB_nuls_clone:0+{FILE_SZ1}@0/"
        f"{mutate_ops}@56KB_nuls_clone:{FILE_SZ1}+{FILE_SZ2}@{FILE_SZ1}"
    )
    if create_ops and mutate_ops:
        # Rendering both subvolumes together shows all the clones.
        return {
            create_ops: render_create_ops(
                kb_nuls=f"{create_clone}/{mutate}/{mutate_clone}",
                kb_nuls_clone=f"{create}/{mutate}/{mutate_clone}",
                zeros_holes_zeros=(
                    f"d16384({mutate_ops}@zeros_hole_zeros:0+16384@0)"
                    f"h16384({mutate_ops}@zeros_hole_zeros:16384+16384@0)"
                    f"d16384({mutate_ops}@zeros_hole_zeros:32768+16384@0)"
                ),
                big_hole=f"({mutate_ops}@hello_big_hole:0+2@0)",
            ),
            mutate_ops: render_mutate_ops(
                kb_nuls=f"{create}/{create_clone}/{mutate_clone}",
                kb_nuls_clone=f"{create}/{create_clone}/{mutate}",
                zeros_holes_zeros=(
                    f"d16384({create_ops}@zeros_hole_zeros:0+16384@0)"
                    f"h16384({create_ops}@zeros_hole_zeros:16384+16384@0)"
                    f"d16384({create_ops}@zeros_hole_zeros:32768+16384@0)"
                ),
                big_hole=f"({create_ops}@hello_big_hole:0+2@0)",
            ),
        }
    elif create_ops:
        return render_create_ops(
            kb_nuls=create_clone,
            kb_nuls_clone=create,
            zeros_holes_zeros="d16384h16384d16384"
            if lossy_packaging != "cpio"
            else "h49152",
            big_hole="",
        )
    elif mutate_ops:
        # This single-subvolume render of `mutate_ops` doesn't show the fact
        # that all data was cloned from `create_ops`.
        return render_mutate_ops(
            kb_nuls=mutate_clone,
            kb_nuls_clone=mutate,
            zeros_holes_zeros="d16384h16384d16384"
            if lossy_packaging != "cpio"
            else "h49152",
            big_hole="",
        )
    raise AssertionError("Set at least one of {create,mutate}_ops")


def render_demo_as_corrupted_by_cpio(*, create_ops=None, mutate_ops=None):
    demo_render = render_demo_subvols(create_ops=create_ops)
    # Cpio does not preserve the original's cloned extents of
    # zeros
    demo_render[1]["56KB_nuls"] = ["(File h57344)"]
    demo_render[1]["56KB_nuls_clone"] = ["(File h57344)"]
    demo_render[1]["zeros_hole_zeros"] = ["(File h49152)"]
    # Cpio does not preserve ACLs
    demo_render[1]["dir_with_acls"][0] = "(Dir)"
    # Cpio does not preserve xattrs
    demo_render[1]["hello"][0] = "(Dir)"
    # Cpio does not preserve unix domain sockets, as these are usable only for
    # the lifetime of the associated process and should therefore be safe to
    # ignore.
    demo_render[1].pop("unix_sock")
    return demo_render


def render_demo_as_corrupted_by_gnu_tar(*, create_ops=None, mutate_ops=None):
    demo_render = render_demo_subvols(create_ops=create_ops)
    # Tar does not preserve the original's cloned extents of
    # zeros
    demo_render[1]["56KB_nuls"] = ["(File d57344)"]
    demo_render[1]["56KB_nuls_clone"] = ["(File d57344)"]
    # Tar does not preserve unix domain sockets, as these are usable only for
    # the lifetime of the associated process and should therefore be safe to
    # ignore.
    demo_render[1].pop("unix_sock")
    return demo_render


def parse_demo_sendstreams_btrfs_dump(
    binary_infile: BinaryIO,
) -> Iterable[SendStreamItem]:
    """
    The 'create_ops' demo sendstream contains xattrs that are unparsable from
    `btrfs receive --dump` output. The reasons for are is documented in:
        antlir/btrfs_diff/parse_dump.py: set_xattr.parse_details()

    So to allow tests to verify btrfs dumped content from this sendstream, we
    provide this wrapper around parse_btrfs_dump() which skips the parsing of
    those fields and substitutes in the expected values.
    """

    create_ops_fix_fields: Dict[bytes, Dict[str, bytes]] = {
        b"name=system.posix_acl_default data=\x02 len=52": {
            "name": b"system.posix_acl_default",
            "data": _dir_with_acls_system_posix_acl,
        },
        b"name=system.posix_acl_access data=\x02 len=52": {
            "name": b"system.posix_acl_access",
            "data": _dir_with_acls_system_posix_acl,
        },
    }

    return parse_btrfs_dump(binary_infile, create_ops_fix_fields)
