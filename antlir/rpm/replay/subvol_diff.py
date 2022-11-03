# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See the `subvol_diff()` docblock."
import fnmatch
import os
import re
import subprocess
from typing import Iterable, Iterator

from antlir.common import get_logger
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol

log = get_logger()

# See _match_or_child to understand how these are matched with paths
_PATH_PATTERNS_EXPECTED_TO_DIFFER = set()
for p in [
    "etc/shadow",  # FIXME: Only "days since pwd change may differ"
    "etc/ld.so.cache",
    "etc/dnf/modules.d",
    # Needed for git-lfs RPM; TODO: Add fuzzy matching with ConfigParser to
    # handle cases like this (files are equivalent but order is different)
    "etc/gitconfig",
    "usr/lib/fontconfig/cache",
    "usr/share/fonts/.uuid",
    "usr/share/fonts/*/.uuid",
    "usr/share/X11/fonts/.uuid",
    "usr/share/X11/fonts/*/.uuid",
    # Ordering of `info` sections is non-deterministic here:
    "usr/share/info/dir",
    "usr/share/info/dir.old",
    # Web2C `.log` files include timestamps
    "var/lib/texmf/web2c/*/*.log",
    # Web2C transpiles Pascal to C and builds fonts via scriptlets,
    # and these builds are *not* bitwise-deterministic.
    "var/lib/texmf/web2c/metafont/mf.base",
    "var/lib/texmf/web2c/*/*.fmt",
    "var/cache/ldconfig/aux-cache",
    "var/lib/rpm",
    "var/lib/yum",
    "var/lib/dnf",
    "var/log/yum.log",
    "var/log/hawkey.log",
    "var/log/dnf.librepo.log",
    "var/log/dnf.log",
    "var/log/dnf.rpm.log",
    ".meta/build/target",
    ".meta/key_value_store",
]:
    assert not p.startswith("/"), p
    _PATH_PATTERNS_EXPECTED_TO_DIFFER.add(p.encode())


def _parse_diff_output(
    left_base: bytes,
    right_base: bytes,
    out: bytes,
) -> Iterator[bytes]:
    """
    Parse the output of `LANG=C diff --brief --recursive` as a quick and dirty
    comparison of filesystem contents.  See `subvol_diff` for what should be the
    long-term approach to replace this.

    In this mode, `diff` only outputs 2 types of lines. We match for both:
      - "Files left/x and right/x differ" -- yield "x"
      - "Only in left_or_right/foo: bar" -- yield "foo/bar".  This could also
        defensibly return "foo", but the upside of returning "foo/bar" is that
        our fuzzy matching (_PATH_PATTERNS_EXPECTED_TO_DIFFER) can then ignore
        stuff that exists only in one image (i.e.  we don't care if it exists or
        not).
    """
    left_base = left_base.rstrip(b"/") + b"/"
    right_base = right_base.rstrip(b"/") + b"/"
    assert not left_base.startswith(right_base) and not right_base.startswith(
        left_base
    ), (
        left_base,
        right_base,
    )
    for l in out.splitlines():
        m = re.match(b"Files (.*) and (.*) differ$", l)
        if m:
            assert all(b" and " not in g for g in m.groups()), ("ambiguous", l)
            left, right = m.groups()
            assert left.startswith(left_base) and right.startswith(right_base), (
                left_base,
                right_base,
                l,
            )
            left = os.path.relpath(left, left_base)
            right = os.path.relpath(right, right_base)
            assert left == right, l
            log.info(f"File differs {left}")
            yield left
            continue

        m = re.match(b"Only in (.*): ([^/]*)$", l)
        if m:
            left_or_right, lacks_counterpart = m.groups()
            assert not re.match(b".*: [^/]*$", left_or_right), ("ambigous", l)
            left_or_right += b"/"
            if left_or_right.startswith(left_base):
                left_or_right = os.path.relpath(left_or_right, left_base)
            elif left_or_right.startswith(right_base):
                left_or_right = os.path.relpath(left_or_right, right_base)
            else:
                raise AssertionError(
                    f"Neither left nor right {left_base} {right_base} {l}"
                )
            log.info(f"Dir differs {left_or_right}: {lacks_counterpart}")
            yield left_or_right + b"/" + lacks_counterpart
            continue

        raise NotImplementedError(f"diff line {l}")


def _match_or_child(child: bytes, potential_parent: bytes) -> bool:
    child = child.rstrip(b"/")
    parent = potential_parent.rstrip(b"/")
    return fnmatch.fnmatch(child, parent) or child.startswith(parent + b"/")


def _discard_path_expected_to_differ(
    diff_paths: Iterable[bytes],
) -> Iterator[Path]:
    for p in diff_paths:
        if any(
            _match_or_child(p, expected_p)
            for expected_p in _PATH_PATTERNS_EXPECTED_TO_DIFFER
        ):
            continue
        yield Path(p)


def subvol_diff(left: Subvol, right: Subvol) -> Iterator[Path]:
    """
    IMPORTANT: This is NOT a generic subvolume-diffing primitive, it's
    currently intended just to compare `subvols.leaf` with the output
    of `replay_rpms_and_compiler_items()`.

    Returns the list of paths whose contents differs between `left` and `right`.
    This does NOT compare filesystem metadata.

    TODO: Build general comparison primitives.  Use them to make this
    comparison stronger & faster.  Specifically:
      - Use `btrfs_diff/` to avoid touching unchanged files, and for
        a full metadata comparison (--no-data & incremental). Note that
        this requires implementing incremental sendstream support.
      - Thereafter, we should not need a `diff` shell-out, just a run an
        Antlir-internal binary as root.
      - Do a "smart" comparison of key files & directories, e.g. tolerate
        only the following "allowable" `/etc/shadow` difference instead of
        ignoring all differences:
           $ diff TODO/old/etc/shadow TODO/new/etc/shadow
           41c41
           < nginx:!!:18768::::::
           ---
           > nginx:!!:18769::::::
    """
    ret = left.run_as_root(
        [
            "diff",
            "--brief",
            "--recursive",
            "--no-dereference",
            left.path(),
            right.path(),
        ],
        env={
            **os.environ,
            "LANG": "C",  # We match on stdout, so get predictable strings
        },
        stdout=subprocess.PIPE,
        check=False,
    )

    if ret.returncode == 0 and not ret.stdout:
        return  # Subvolumes identical.
    elif ret.returncode != 1 or not ret.stdout:
        raise RuntimeError(f"diff internal error: {ret}")

    yield from _discard_path_expected_to_differ(
        _parse_diff_output(left.path(), right.path(), ret.stdout)
    )
