#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""\
Garbage collection deletes subvolumes that are no longer referenced by
"subvolume JSON" build artifacts tracked by Buck.

This script first creates a refcount in a private directory, and shortly
afterwards makes the subvolume's outer wrapper directory.

To distinguish subvolumes that are referenced by the Buck cache ("live")
from ones that are not ("dead"), this tool initializes the Buck-supplied
"$OUT" (`--new-subvolume-json`) with a hardlink into a directory we own.

Given these hardlinks, we can garbage-collect any subvolumes, which do
**not** have a hardlink at all, or whose link count drops to 1.  This works
because in building the output, Buck actually `mv`s the new output on top of
the old one, which unlinks the previous version -- but we also always try to
`unlink "$OUT"`, just in case Buck's behavior changes.

KEY ASSUMPTIONS:

 - `buck-out/` (containing `--new-subvolume-json`) is on the same filesystem
    as `--refcounts-dir` (which lives in the the source repo).

 - Two garbage collector instances NEVER run concurrently with the same
   `--new-subvolume-wrapper-dir` NOR with the same `--new-subvolume-json`.
   In practice, the former is assured because `subvolume_version.py` returns
   a unique number for each new build.  The latter is (hopefully) guaranteed
   by Buck -- presumably, it does not concurrently start two builds with the
   same output.  Protecting against this with something like `flock` is too
   onerous (lockfiles can never be deleted), so we just assume our caller is
   sane.

 - Subvolumes live in wrapper directories, with this directory layout:
   <subvol_name>:<some unique id>/<subvol name>
"""
import argparse
import contextlib
import fcntl
import glob
import logging
import os
import re
import stat
import sys
from typing import Iterator, List, Tuple

from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol


log = logging.Logger(os.path.basename(__file__))  # __name__ is __main__


@contextlib.contextmanager
def nonblocking_flock(path: Path) -> Iterator[bool]:
    "Yields True if we got the lock, False otherwise."
    # We don't set CLOEXEC on this FD because we want e.g. `sudo btrfs` to
    # inherit it and hold the lock while it does its thing.  It seems OK to
    # trust that `btrfs` will not be doing shenanigans like daemonizing a
    # service that runs behind our back.
    fd = os.open(path, os.O_RDONLY)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield True
        except BlockingIOError:
            yield False
    finally:
        os.close(fd)  # Don't hold the lock any longer than we have to!


def list_subvolume_wrappers(subvolumes_dir: Path) -> List[Path]:
    # Ignore directories that don't match the <subvol name>:<id> pattern.
    subvolumes = [
        Path(p).relpath(subvolumes_dir) for p in glob.glob(f"{subvolumes_dir}/*:*/")
    ]
    # If glob works correctly, this list should always be empty.
    bad_subvolumes = [s for s in subvolumes if b"/" in s]
    assert not bad_subvolumes, f"{bad_subvolumes} globbing {subvolumes_dir}"
    return subvolumes


def list_refcounts(refcounts_dir: Path) -> Iterator[Tuple[Path, int]]:
    # The the first part of the name may contain 0 or more colons.
    reg = re.compile("^(?P<name>.+):(?P<version>[^:]+).json$")
    for p in glob.glob(f"{refcounts_dir}/*:*.json"):
        m = reg.match(os.path.basename(p))
        # Only fails if glob does not work.
        assert m is not None, f"Bad refcount item {p} in {refcounts_dir}"
        st = os.stat(p)
        if not stat.S_ISREG(st.st_mode):
            raise RuntimeError(f"Refcount {p} is not a regular file")
        # It is tempting to check that the subvolume name & version match
        # `SubvolumeOnDisk.from_json_file`, but we cannot do that because
        # our GC pass might be running concurrently with another build, and
        # the refcount file might be empty or half-written.
        yield (Path(f"{m.group('name')}:{m.group('version')}"), st.st_nlink)


def garbage_collect_subvolumes(refcounts_dir: Path, subvolumes_dir: Path) -> None:
    # IMPORTANT: We must list subvolumes BEFORE refcounts. The risk is that
    # this runs concurrently with another build, which will create a new
    # refcount & subvolume (in that order).  If we read refcounts first, we
    # might end up winning the race against the other build, and NOT reading
    # the new refcount.  If we then lose the second part of the race, we
    # would find the subvolume that the other process just created, and
    # delete it.
    subvol_wrappers = set(list_subvolume_wrappers(subvolumes_dir))
    subvol_wrapper_to_nlink = dict(list_refcounts(refcounts_dir))

    # Delete subvolumes (& their wrappers) with insufficient refcounts.
    for subvol_wrapper in subvol_wrappers:
        nlink = subvol_wrapper_to_nlink.get(subvol_wrapper, 0)
        if nlink == 2:
            continue  # expected case, this is a hardlink with 2 refs
        elif nlink > 2:
            # An actual real way for us to end up with nlink > 2 is if
            # something else hardlinks the "subvol JSON" inside buck-out.
            # This can be done via antlir/bzl/image_layer_alias.bzl.
            # It is an unusual case, but not fatal.
            log.info(f"{nlink} > 2 links to subvolume {subvol_wrapper}")
            continue
        refcount_path = Path(refcounts_dir) / f"{subvol_wrapper}.json"
        log.info(f"Deleting {subvol_wrapper} since its refcount has {nlink} links")
        # Start by unlinking the refcount to dramatically decrease the
        # chance of leaving an orphaned refcount file on disk.  The most
        # obvious way to get an orphaned refcount is for this program to
        # abort between the line that creates the refcount link, and the
        # next line that creates the subvolume wrapper.
        #
        # I do not see a great way to completely eliminate orphan refcount
        # files.  One could try to have a separate pass that flocks the
        # refcount file before removing it, and to also flock the refcount
        # file before creating the wrapper directory.  But, since file
        # creation & flock cannot be atomic, this leaves us open to a race
        # where a concurrent GC pass removes the refcount link immediately
        # after it gets created, so that part of the code would have to be
        # willing to repeat the race until it wins.  In all, that extra
        # complexity is far too ugly compared to the slim risk or leaving
        # some unused refcount files on disk.
        if nlink:
            refcount_path.unlink()

        # Subvols are wrapped in a user-owned temporary directory, following
        # the convention `{rule name}:{version}/{subvol}`.
        wrapper_path = Path(subvolumes_dir) / subvol_wrapper

        wrapper_content = set(wrapper_path.listdir())
        # We may have run `systemd-nspawn` against the subvolume, e.g.
        # as part of `image.genrule_layer`, which creates this lockfile.
        maybe_lockfile = [f for f in wrapper_content if f.startswith(b".#")]
        if maybe_lockfile:
            assert len(maybe_lockfile) == 1, maybe_lockfile
            (maybe_lockfile,) = maybe_lockfile
            wrapper_content.remove(maybe_lockfile)

        if len(wrapper_content) > 1:
            raise RuntimeError(
                f"{wrapper_path} must contain just 1 subvol: {wrapper_content}"
            )
        elif len(wrapper_content) == 1:
            (subvol,) = wrapper_content
            expected_lock_path = wrapper_path / f".#{subvol}.lck"
            assert (
                # The output of `.listdir()` should match our `.exists()`
                bool(maybe_lockfile) == expected_lock_path.exists()
                and (
                    not maybe_lockfile
                    or maybe_lockfile == expected_lock_path.basename()
                )
            ), (maybe_lockfile, expected_lock_path)
            if maybe_lockfile:
                expected_lock_path.unlink()
            Subvol(wrapper_path / subvol, already_exists=True).delete()
        else:  # No subvolume in wrapper
            # We don't expect to see a stray lockfile here because we delete
            # the lockfile before the subvol.
            assert not maybe_lockfile, f"Stray lockfile found: {maybe_lockfile}"

        os.rmdir(wrapper_path)


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--refcounts-dir",
        required=True,
        type=Path.from_argparse,
        help="We will create a hardlink to `--new-subvolume-json` in this "
        "directory. For that reason, this needs to be on same device, "
        "and thus cannot be under `--subvolumes-dir`",
    )
    parser.add_argument(
        "--subvolumes-dir",
        required=True,
        type=Path.from_argparse,
        help="A directory on a btrfs volume, where all the subvolume wrapper "
        "directories reside.",
    )
    parser.add_argument(
        "--new-subvolume-wrapper-dir",
        type=Path.from_argparse,
        help="Subvolumes live inside wrapper directories, following the "
        "convention <name>:<version>/<name>. This parameter should "
        "consist just of the <name>:<version> part.",
    )
    parser.add_argument(
        "--new-subvolume-json",
        type=Path.from_argparse,
        help="We will delete any file at this path, then create an empty one, "
        "and hard-link into `--refcounts-dir` for refcounting purposes. "
        "The image compiler will then write data into this file.",
    )
    return Path.parse_args(parser, argv)


def has_new_subvolume(args) -> bool:
    new_subvolume_args = (
        args.new_subvolume_wrapper_dir,
        args.new_subvolume_json,
    )
    if None not in new_subvolume_args:
        if (
            b":" not in args.new_subvolume_wrapper_dir
            or b"/" in args.new_subvolume_wrapper_dir
        ):
            raise RuntimeError(
                "--new-subvolume-wrapper-dir must contain : but not /, got "
                f"{args.new_subvolume_wrapper_dir}"
            )
        wrapper_path = args.subvolumes_dir / args.new_subvolume_wrapper_dir
        if wrapper_path.exists():
            raise RuntimeError(f"--new-subvolume-wrapper-dir exists {wrapper_path}")
        return True
    if new_subvolume_args != (None,) * 2:
        raise RuntimeError(
            "Either pass both --new-subvolume-* arguments, or pass none."
        )
    return False


def subvolume_garbage_collector(argv) -> None:
    """
    IMPORTANT:

     - Multiple copies of this function can run concurrently, subject to the
       KEY ASSUMPTIONS in the file's docblock.

     - The garbage-collection pass must be robust against failures in the
       middle of the code (imagine somebody hitting Ctrl-C, or worse).

       Here is why this code resists interruptions. It makes these writes:
         (a) unlinks the subvolume json & refcount for the new subvolume
             being created,
         (b) deletes subvolumes with insufficient refcounts,
         (c) populates an empty subvolume json + linked refcount.

       Failing during or after (a) is fine -- it'll have the side effect of
       making the subvolume eligible to be GC'd by another build, but as far
       as I know, Buck will consider the subvolume's old json output dead
       anyhow.  (The fix is easy if it turns out to be a problem.)

       Failing during (b) is also fine. Presumably, `btrfs subvolume delete`
       is atomic, so at worst we will not delete ALL the garbage.

       Failure before (c), or in the middle of (c) will abort the build, so
       the lack of a refcount link won't cause issues later.
    """
    args = parse_args(argv)

    # Delete unused subvolumes.
    #
    # The below `flock` mutex prevents more than one of these GC passes from
    # running concurrently.  The docs of `garbage_collect_subvolumes`
    # explain why a GC pass can safely concurrently run with a build.
    #
    # We don't want to block here to avoid serializing the
    # garbage-collection passes of concurrently running builds.  This
    # may increase disk usage, but overall, the build speed should be
    # better.  Caveat: I don't have meaningful benchmarks to
    # substantiate this, so this is just informed demagoguery ;)
    #
    # Future: if disk usage is a problem, we can loop this code until no
    # deletions are made.  For bonus points, daemonize the loop so that
    # the build that triggered the GC actually gets to make progress.
    with nonblocking_flock(args.subvolumes_dir) as got_lock:
        if got_lock:
            garbage_collect_subvolumes(args.refcounts_dir, args.subvolumes_dir)
        else:
            # That other build probably won't clean up the prior version of
            # the subvolume we are creating, but we don't rely on that to
            # make space anyhow, so let's continue.
            log.warning("A concurrent build is garbage-collecting subvolumes.")

    # .json outputs and refcounts are written as an unprivileged user. We
    # only need root for subvolume manipulation (above).
    try:
        os.mkdir(args.refcounts_dir, mode=0o700)
    except FileExistsError:  # Don't fail on races to `mkdir`.
        pass

    # Prepare the output file for the compiler to write into. We'll need the
    # json output to exist to hardlink it.  But, first, ensure it does not
    # exist so that its link count starts at 1.  Finally, make the hardlink
    # that will serve as a refcount for `garbage_collect_subvolumes`.
    #
    # The `unlink` & `open` below are concurrency-safe per one of KEY
    # ASSUMPTIONS above.  Specifically, Buck's should not ever run 2 build
    # processes with the same output file.
    #
    # The hardlink won't interact with concurrent GC passes, either.
    #  1) Since the subvolume ID is unique, no other process will race to
    #     create the hardlink.
    #  2) Another GC will never delete our subvolume, because we create
    #     subvolumes **after** creating refcounts, while
    #     `garbage_collect_subvolumes` enumerates subvolumes **before**
    #     reading refcounts.
    if has_new_subvolume(args):
        new_subvolume_refcount = Path(
            args.refcounts_dir / f"{args.new_subvolume_wrapper_dir}.json"
        )
        # This should never happen since the name & version are supposed to
        # be unique for this one subvolume (KEY ASSUMPTIONS).
        if new_subvolume_refcount.exists():
            raise RuntimeError(f"Refcount already exists: {new_subvolume_refcount}")

        # Our refcounting relies on the hard-link counts of the output
        # files.  Therefore, we must not write into an existing output file,
        # and must instead unlink and re-create it.  NB: At present, Buck
        # actually gives us an empty directory, so this is done solely for
        # robustness.
        for p in (new_subvolume_refcount, args.new_subvolume_json):
            try:
                p.unlink()
            except FileNotFoundError:
                pass
        os.close(
            os.open(
                args.new_subvolume_json,
                flags=os.O_CREAT | os.O_CLOEXEC | os.O_NOCTTY,
                mode=0o600,
            )
        )

        # Throws if the Buck output is not on the same device as the
        # refcount dir.  That case has to fail, that's how hardlinks work.
        # However, it's easy enough to make the refcounts dir a symlink to a
        # directory on the appropriate device, so this is a non-issue.
        os.link(args.new_subvolume_json, new_subvolume_refcount)
        # This should come AFTER refcount link creation, since we enumerate
        # subvolumes BEFORE enumerating refcounts.
        os.mkdir(
            args.subvolumes_dir / args.new_subvolume_wrapper_dir,
            mode=0o700,
        )


def main() -> None:
    subvolume_garbage_collector(sys.argv[1:])  # pragma: no cover


if __name__ == "__main__":
    main()  # pragma: no cover
