#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Library + demonstration tool for parsing the metadata contained in a `btrfs
send` stream, as printed by `btrfs receive --dump`.

Do not rely on parsing `--dump` output for production purposes. Instead use
the binary parser in `parse_send_stream.py`.  The latter is better because:

 - It sidesteps the bugs & limitation of `--dump` described below.

 - It avoids a dependency on `btrfs-progs`, thus allowing this library
   to be used in more contexts.

We bother parsing `--dump` output for two reasons:

 - It ensures that we never diverge from `btrfs-progs`'s parsing. By
   checking that our parse of the send-stream is effectively identical to
   our parse of `--dump` output, we verify that both the Python and C
   implementations of send-stream parsing work the same way.

 - It continuously tests `--dump` functionality. It is important for that
   feature to work correctly for the send-stream functionality to be
   discoverable and understandable by new users without scrutinizing the
   code.  Any differences in parsed items must be attributable to the known
   set of `--dump` bugs, and nothing else.
 -

Usage:

  btrfs send --no-data SUBVOL/ | btrfs receive --dump | parse_dump.py

The `--no-data` flag is optional, but should speed up the send & receive
considerably.  The parsed output will be similar because this code treats
`write` and `update_extent` items identically.  The only difference comes
from the fact that `send` emits sequential `write` instructions for
sequential chunks of an extent, but `update_extent` is emitted just once per
extent.

Limitations of `btrfs receive --dump` (filed as T25376790):

 - With the exception of the path of the object being manipulated by the
   current dump item, none of the string attributes (paths, xattr names &
   values) are quoted in the current version of `btrfs`. This means that
   if any of those values have a newline, we will fail to parse.

 - xattr values are only printed up to the first \\0 character, see `set_xattr`

 - timestamps are printed only to 1-second resolution, while the filesystem
   records timestamps with nanosecond precision.

 - `clone` commands specify the source subvolume's UUID & transid, but
   `btrfs receive --dump` does not output those, making it impossible to
   unravel the source of a clone when more than one source is in use.
"""
import datetime
import os
import re
from collections import OrderedDict
from typing import Any, BinaryIO, Dict, Iterable, Optional, Pattern, Tuple

from .send_stream import SendStreamItem, SendStreamItems


_ESCAPED_TO_UNESCAPED = OrderedDict(
    [
        (rb"\a", b"\a"),
        (rb"\b", b"\b"),
        (rb"\e", b"\x1b"),
        (rb"\f", b"\f"),
        (rb"\n", b"\n"),
        (rb"\r", b"\r"),
        (rb"\t", b"\t"),
        (rb"\v", b"\v"),
        (rb"\ ", b" "),
        (b"\\\\", b"\\"),
        *[(f"\\{i:03o}".encode("ascii"), bytes([i])) for i in range(256)],
        # For now: leave alone any backslashes not involved in a known path
        # escape sequence.  However, we could add fallbacks here.
    ]
)
# You can visualize the resulting table with this snippet:
#   print('\n'.join(
#       '\t'.join(
#           (
#               f'{e.decode("ascii")} -> {repr(u).lstrip("b")}'
#                   if (e, u) != (None, None) else ''
#           ) for e, u in group
#       ) for group in itertools.zip_longest(
#           *([iter(_ESCAPED_TO_UNESCAPED.items())] * 5),
#           fillvalue=(None, None),
#       )
#   ))
_ESCAPED_REGEX = re.compile(
    b"|".join(re.escape(e) for e in _ESCAPED_TO_UNESCAPED)
)


def unquote_btrfs_progs_path(s):
    """
    `btrfs receive --dump` always quotes the first field of an item -- the
    subvolume path being touched.  Its quoting is similar to C, but
    idiosyncratic (see `print_path_escaped` in `send-dump.c`), so we need a
    custom un-quoting function.  Future: fix `btrfs-progs` so that other
    fields (paths & data) are quoted too.
    """
    return _ESCAPED_REGEX.sub(lambda m: _ESCAPED_TO_UNESCAPED[m.group(0)], s)


class RegexItemParser:
    "Almost all item types can be parsed with a single regex."

    regex: Pattern = re.compile(b"")

    @classmethod
    def parse_details(
        cls, subvol_name: bytes, details: bytes
    ) -> Optional[Dict[str, Any]]:
        m = cls.regex.fullmatch(details)
        return (
            {
                # Handle `conv_FIELD_NAME` class methods for converting fields.
                # These take a single positional argument, and handle most
                # cases.
                #
                # We currently only use `context_conv_FIELD_NAME` when a detail
                # field needs to know the subvolume name, see e.g. `clone`.
                k: getattr(
                    cls, f"context_conv_{k}", lambda value, subvol_name: value
                )(
                    getattr(cls, f"conv_{k}", lambda x: x)(v),
                    subvol_name=subvol_name,
                )
                for k, v in m.groupdict().items()
            }
            if m
            else None
        )


def _normalize_subvolume_path(s: bytes, *, subvol_name: bytes) -> bytes:
    # `normpath` is needed since `btrfs receive --dump` is inconsistent
    # about trailing slashes on directory paths.
    stripped = os.path.relpath(s, subvol_name)
    if len(stripped) >= len(s) or stripped.startswith(b".."):
        raise RuntimeError(f"{s} did not start with {subvol_name}")
    return stripped


def _from_octal(s: bytes) -> int:
    return int(s, base=8)


class SendStreamItemParsers:
    """
    This class exists to group its inner classes, see NAME_TO_PARSER_TYPE.

    The correspondence to `SendStreamItems` members is automatic via the
    inner class name.
    """

    class subvol(RegexItemParser):
        regex = re.compile(
            rb"uuid=(?P<uuid>[-0-9a-f]+) " rb"transid=(?P<transid>[0-9]+)"
        )
        conv_transid = staticmethod(int)

    class snapshot(RegexItemParser):
        regex = re.compile(
            rb"uuid=(?P<uuid>[-0-9a-f]+) "
            rb"transid=(?P<transid>[0-9]+) "
            rb"parent_uuid=(?P<parent_uuid>[-0-9a-f]+) "
            rb"parent_transid=(?P<parent_transid>[0-9]+)"
        )
        conv_transid = staticmethod(int)
        conv_parent_transid = staticmethod(int)

    class mkfile(RegexItemParser):
        pass

    class mkdir(RegexItemParser):
        pass

    class mknod(RegexItemParser):
        regex = re.compile(rb"mode=(?P<mode>[0-7]+) dev=0x(?P<dev>[0-9a-f]+)")
        conv_mode = staticmethod(_from_octal)

        @staticmethod
        def conv_dev(dev: bytes) -> int:
            return int(dev, base=16)

    class mkfifo(RegexItemParser):
        pass

    class mksock(RegexItemParser):
        pass

    class symlink(RegexItemParser):
        # NB unlike the paths in other items, the symlink target is just an
        # arbitrary string with no filesystem signficance, so we do not
        # process it at all.  Unfortunately, `dest` is not quoted in
        # `send-dump.c`.
        regex = re.compile(rb"dest=(?P<dest>.*)")

    class rename(RegexItemParser):
        # This path is not quoted in `send-dump.c`
        regex = re.compile(rb"dest=(?P<dest>.*)")
        context_conv_dest = _normalize_subvolume_path

    class link(RegexItemParser):
        # This path is not quoted in `send-dump.c`
        regex = re.compile(rb"dest=(?P<dest>.*)")

        # `btrfs receive` is inconsistent -- unlike other paths, its `dest`
        # does not start with the subvolume path.
        conv_dest = os.path.normpath

    class unlink(RegexItemParser):
        pass

    class rmdir(RegexItemParser):
        pass

    # NB: `write` is not here because below we map it to `update_extent`.

    class clone(RegexItemParser):
        # The path `from` is not quoted in `send-dump.c`, but a greedy
        # regex can still parse this fixed format correctly.
        regex = re.compile(
            rb"offset=(?P<offset>[0-9]+) "
            rb"len=(?P<len>[0-9]+) "
            rb"from=(?P<from_path>.+) "  # the field is `from_path`, not `from`
            rb"clone_offset=(?P<clone_offset>[0-9]+)"
            rb"(?P<from_uuid>)(?P<from_transid>)"
        )
        conv_offset = staticmethod(int)
        conv_len = staticmethod(int)
        context_conv_from_path = _normalize_subvolume_path
        conv_clone_offset = staticmethod(int)

    class set_xattr:
        # `btrfs --dump` outputs a `len` field, which is just `len(data)`,
        # but see the caveat below.
        #
        # This cannot be parsed unambiguously with a single regex because
        # both `name` and `data` can contain arbitrary bytes, and neither is
        # quoted.
        first_regex = re.compile(rb"(.*) len=([0-9]+)")
        second_regex = re.compile(rb"name=(.*) data=")

        @classmethod
        def parse_details(
            cls, subvol_name: bytes, details: bytes
        ) -> Optional[Dict[str, Any]]:
            m = cls.first_regex.fullmatch(details)
            if not m:
                return None
            rest = m.group(1)

            # An awful hack to deal with the fact that we cannot
            # unambiguously parse this name / data line as implemented.
            # The reason is that, `btrfs receive --dump` prints xattrs with
            # this `printf`:
            #   "name=%s data=%.*s len=%d", name, len, (char *)data, len
            # The end result is that `data` gets printed up to the first \0.
            #
            # Our workaround is to first assume that all of `data` was
            # printed.  If that doesn't work, we try again, assuming that it
            # just has a trailing \0 byte.  If that doesn't work either, we
            # give up.
            #
            # The alternative would be for the parse to store `len` &
            # `data`, with `len(data) < len` in some cases.  This seems
            # broken and useless, and makes downstream code harder.  If we
            # need to support xattrs with \0 chars in the middle, we should
            # either fix `btrfs receive --dump` to do quoting, or just parse
            # the binary send-stream.
            length = m.group(2)
            for has_trailing_null in [False, True]:
                end_of_data = len(rest) - int(length) + has_trailing_null
                m = cls.second_regex.fullmatch(rest[:end_of_data])
                data = rest[end_of_data:]
                if has_trailing_null:
                    data += b"\0"
                assert len(data) == int(length)  # We don't need to store `len`
                if m:
                    return {"name": m.group(1), "data": data}
            return None

    class remove_xattr(RegexItemParser):
        # This name is not quoted in `send-dump.c`
        regex = re.compile(rb"name=(?P<name>.*)")

    class truncate(RegexItemParser):
        regex = re.compile(rb"size=(?P<size>[0-9]+)")
        conv_size = staticmethod(int)

    class chmod(RegexItemParser):
        regex = re.compile(rb"mode=(?P<mode>[0-7]+)")
        conv_mode = staticmethod(_from_octal)

    class chown(RegexItemParser):
        regex = re.compile(rb"gid=(?P<gid>[0-9]+) uid=(?P<uid>[0-9]+)")
        conv_gid = staticmethod(int)
        conv_uid = staticmethod(int)

    class utimes(RegexItemParser):
        regex = re.compile(
            rb"atime=(?P<atime>[^ ]+) "
            rb"mtime=(?P<mtime>[^ ]+) "
            rb"ctime=(?P<ctime>[^ ]+)"
        )

        @classmethod
        def conv_atime(cls, t: bytes) -> Tuple[int, int]:
            return (
                int(
                    datetime.datetime.strptime(
                        t.decode(), "%Y-%m-%dT%H:%M:%S%z"
                    ).timestamp()
                ),
                0,
            )  # --dump discards nanoseconds

        conv_mtime = conv_atime
        conv_ctime = conv_atime

    # This is used instead of `write` when `btrfs send --no-data` is used.
    class update_extent(RegexItemParser):
        regex = re.compile(rb"offset=(?P<offset>[0-9]+) len=(?P<len>[0-9]+)")
        conv_offset = staticmethod(int)
        conv_len = staticmethod(int)


# The inner classes of SendStreamItems, omitting internals like __doc__.
# The keys must be bytes because `btrfs` does not give us unicode.
NAME_TO_PARSER_TYPE = {
    k.encode(): v
    for k, v in SendStreamItemParsers.__dict__.items()
    if k[0] != "_"
}
NAME_TO_ITEM_TYPE = {
    k.encode(): v
    for k, v in SendStreamItems.__dict__.items()
    if k[0] != "_" and k != "write"
}
assert set(NAME_TO_PARSER_TYPE.keys()) == set(NAME_TO_ITEM_TYPE.keys())


def parse_btrfs_dump(
    binary_infile: BinaryIO,
    fix_fields: Optional[Dict[bytes, Dict[str, bytes]]] = None,
) -> Iterable[SendStreamItem]:
    reg = re.compile(rb"([^ ]+) +((\\ |[^ ])+) *(.*)\n")
    subvol_name = None
    for l in binary_infile:
        m = reg.fullmatch(l)
        if not m:
            raise RuntimeError(f"line has unexpected format: {repr(l)}")
        item_name, path, _, details = m.groups()

        # This parser maps `write` to `update_extent` regardless of whether
        # the send-stream used `--no-data` or not.  The reason is that
        # `btrfs receive --dump` never displays the `data` field (because it
        # can be huge, and not very illuminating to the user).
        if item_name == b"write":
            item_name = b"update_extent"

        item_class = NAME_TO_ITEM_TYPE.get(item_name)
        if not item_class:
            raise RuntimeError(f"unknown item type {item_name} in {repr(l)}")
        item_parser = NAME_TO_PARSER_TYPE[item_name]

        # We MUST unquote here, or paths in field 1 will not be comparable
        # with as-of-now unquoted paths in the other fields.  For example,
        # `ItemFilters.rename` compares such paths.
        unnormalized_path = unquote_btrfs_progs_path(path)

        if subvol_name is None:
            if not item_class.sets_subvol_name:
                raise RuntimeError(
                    f"First stream item did not set subvolume name: {l}"
                )
            path = os.path.normpath(unnormalized_path)
            subvol_name = path
            if b"/" in path:
                raise RuntimeError(f"subvol path {path} contains /")
        elif item_class.sets_subvol_name:
            raise RuntimeError(
                f"Subvolume {subvol_name} created more than once."
            )
        else:
            path = _normalize_subvolume_path(
                unnormalized_path, subvol_name=subvol_name
            )

        if fix_fields and details in fix_fields.keys():
            fields = fix_fields[details]
        else:
            fields = item_parser.parse_details(subvol_name, details)

        if fields is None:
            raise RuntimeError(f"unexpected format in line details: {repr(l)}")

        assert "path" not in fields, f"{item_name}.regex defined <path>"
        fields["path"] = path

        yield item_class(**fields)


if __name__ == "__main__":  # pragma: no cover
    import sys

    for item in parse_btrfs_dump(sys.stdin.buffer):
        print(item)
