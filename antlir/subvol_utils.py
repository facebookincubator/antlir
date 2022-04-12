#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools
import logging
import os
import platform
import subprocess
import sys
import time
from contextlib import contextmanager, ExitStack
from typing import AnyStr, BinaryIO, Iterable, Iterator, Optional, TypeVar

from .artifacts_dir import find_artifacts_dir
from .btrfs_diff.freeze import DoNotFreeze
from .bzl.loopback_opts import loopback_opts_t
from .common import (
    check_popen_returncode,
    get_logger,
    open_fd,
    pipe,
    run_stdout_to_err,
)
from .compiler.subvolume_on_disk import SubvolumeOnDisk
from .fs_utils import Path, temp_dir
from .loopback import BtrfsLoopbackVolume, MIN_CREATE_BYTES
from .unshare import Namespace, Unshare, nsenter_as_root, nsenter_as_user


log = get_logger()
KiB = 2 ** 10
MiB = 2 ** 20


# Exposed as a helper so that test_compiler.py can mock it.
def _path_is_btrfs_subvol(path: Path) -> bool:
    "Ensure that there is a btrfs subvolume at this path. As per @kdave at"
    "https://stackoverflow.com/a/32865333"
    # You'd think I could just `os.statvfs`, but no, not until Py3.7
    # https://bugs.python.org/issue32143
    fs_type = subprocess.run(
        ["stat", "-f", "--format=%T", path], stdout=subprocess.PIPE, text=True
    ).stdout.strip()
    ino = os.stat(path).st_ino
    return fs_type == "btrfs" and ino == 256


T = TypeVar


# HACK ALERT: `Subvol.delete()` removes subvolumes nested inside it. Some
# of these may also be tracked as `Subvol` objects. In this scenario,
# we have to update `._exists` for the nested `Subvol`s. This global
# registry contains all the **created and not deleted** `Subvol`s known
# to the current program.
#
# This design is emphatically not thread-safe etc.  It also leaks any
# `Subvol` objects that are destroyed without deleting the underlying
# subvolume.
_UUID_TO_SUBVOLS = {}


def _mark_deleted(uuid: str) -> None:
    "Mark all the clones of this `Subvol` as deleted. Ignores unknown UUIDs."
    subvols = _UUID_TO_SUBVOLS.get(uuid)
    if not subvols:
        # This happens if we are deleting a subvolume created outside of the
        # Antlir compiler, which is nested in a `Subvol`.
        return
    for sv in subvols:
        # Not checking that `._path` agrees because that check would
        # take work to make non-fragile.
        assert uuid == sv._uuid, (uuid, sv._uuid, sv._path)
        sv._USE_mark_created_deleted_INSTEAD_exists = False
        sv._uuid = None
    del _UUID_TO_SUBVOLS[uuid]


def _query_uuid(subvol: "Subvol", path: Path):
    res = subvol.run_as_root(
        ["btrfs", "subvolume", "show", path], stdout=subprocess.PIPE
    )
    res.check_returncode()
    subvol_metadata = res.stdout.split(b"\n", 3)
    # /
    # Name:                   <FS_TREE>
    # UUID:                   15a88f92-4185-47c9-8048-f065a159f119
    # Parent UUID:            -
    # Received UUID:          -
    # Creation time:          2020-09-30 09:36:02 -0700
    # Subvolume ID:           5
    # Generation:             2045967
    # Gen at creation:        0
    # Parent ID:              0
    # Top level ID:           0
    # Flags:                  -
    # Snapshot(s):

    return subvol_metadata[2].split(b":")[1].decode().strip()


# Subvol is marked as `DoNotFreeze` as it's hash is just of
# byte string that contains the path to the subvol. It's member
# variables are just a cache of the external state of the subvol
# and do not affect its hash.
class Subvol(DoNotFreeze):
    """
    ## What is this for?

    This class is to be a privilege / abstraction boundary that allows
    regular, unprivileged Python code to construct images.  Many btrfs
    ioctls require CAP_SYS_ADMIN (or some kind of analog -- e.g. a
    `libguestfs` VM or a privileged server for performing image operations).
    Furthermore, writes to the image-under-construction may require similar
    sorts of privileges to manipulate the image-under-construction as uid 0.

    One approach would be to eschew privilege boundaries, and to run the
    entire build process as `root`.  However, that would forever confine our
    build tool to be run in VMs and other tightly isolated contexts.  Since
    unprivileged image construction is technically possible, we will instead
    take the approach that -- as much as possible -- the build code runs
    unprivileged, as the repo-owning user, and only manipulates the
    filesystem-under-construction via this one class.

    For now, this means shelling out via `sudo`, but in the future,
    `libguestfs` or a privileged filesystem construction proxy could be
    swapped in with minimal changes to the overall structure.

    ## Usage

    - Think of `Subvol` as a ticket to operate on a btrfs subvolume that
      exists, or is about to be created, at a known path on disk. This
      convention lets us cleanly describe paths on a subvolume that does not
      yet physically exist.

    - Call the functions from the btrfs section to manage the subvolumes.

    - Call `subvol.run_as_root()` to use shell commands to manipulate the
      image under construction.

    - Call `subvol.path('image/relative/path')` to refer to paths inside the
      subvolume e.g. in arguments to the `subvol.run_*` functions.
    """

    def __init__(
        self,
        path: AnyStr,
        *,
        already_exists: bool = False,
        _test_only_allow_existing: bool = False,
    ) -> None:
        """
        `Subvol` can represent not-yet-created (or created-and-deleted)
        subvolumes.  Unless already_exists=True, you must call create() or
        snapshot() to actually make the subvolume.

        WATCH OUT: Because of `_UUID_TO_SUBVOLS`, all `Subvol` objects in the
        "exists" state (created, snapshotted, initialized with
        `already_exists=True`, etc) will **leak** if the owning code loses
        the last reference without deleting the underlying subvol.

        This is OK for now since we don't store any expensive / mutexed
        resources here.  However, if this ever changes, we may need to play
        difficult games with `weakref` to avoid leaking those resources.
        """
        self._path = Path(path).abspath()
        self._USE_mark_created_deleted_INSTEAD_exists = False
        self._uuid = None
        if already_exists:
            if not _path_is_btrfs_subvol(self._path):
                raise AssertionError(f"No btrfs subvol at {self._path}")
            self._mark_created()
        elif not _test_only_allow_existing:
            assert not os.path.exists(self._path), self._path

    # This is read-only because any writes bypassing the `_mark*` functions
    # would violate our internal invariants.
    @property
    def _exists(self):
        return self._USE_mark_created_deleted_INSTEAD_exists

    def _mark_created(self) -> None:
        assert not self._exists and not self._uuid, (self._path, self._uuid)
        self._USE_mark_created_deleted_INSTEAD_exists = True
        # The UUID is valid only while `._exists == True`
        self._uuid = _query_uuid(self, self.path())
        # This not a set because our `hash()` is based on just `._path` and
        # we really care about object identity here.
        _UUID_TO_SUBVOLS.setdefault(self._uuid, []).append(self)

    def _mark_deleted(self) -> None:
        assert self._exists and self._uuid, self._path
        assert any(
            # `_mark_deleted()` will ignore unknown UUIDs, but ours must be
            # known since we are not deleted.
            (self is sv)
            for sv in _UUID_TO_SUBVOLS.get(self._uuid, [])
        ), (self._uuid, self._path)
        _mark_deleted(self._uuid)

    def __eq__(self, other: "Subvol") -> bool:
        assert self._exists == other._exists, self.path()
        equal = self._path == other._path
        assert not equal or self._uuid == other._uuid, (
            self._path,
            self._uuid,
            other._uuid,
        )
        return equal

    # `__hash__` contains only `_path`. The member variables
    # of `Subvol` are just a cache of the external state of the subvol.
    def __hash__(self) -> int:
        return hash(self._path)

    def path(
        self,
        path_in_subvol: AnyStr = b".",
        *,
        no_dereference_leaf: bool = False,
        resolve_links: bool = False,
    ) -> Path:
        """
        The only safe way to access paths inside the subvolume.  Do NOT
        `os.path.join(subvol.path('a/path'), 'more/path')`, since that skips
        crucial safety checks.  Instead: `subvol.path(os.path.join(...))`.

        See the `Path.ensure_child` doc for more details.
        """
        # It's important that this is normalized.  The `btrfs` CLI is not
        # very flexible, so it will try to name a subvol '.' if we do not
        # normalize `/subvol/.`.
        return self._path.normalized_subpath(
            path_in_subvol,
            no_dereference_leaf=no_dereference_leaf,
            resolve_links=resolve_links,
        )

    def canonicalize_path(self, path: AnyStr) -> Path:
        """
        IMPORTANT: At present, this will silently fail to resolve symlinks
        in the image that are not traversible by the repo user.  This means
        it's really only appropriate for paths that are known to be
        world-readable, e.g.  repo snapshot stuff in `__antlir__`.

        The analog of `os.path.realpath()` taking an in-subvolume path
        (subvol-absolute or relative to subvol root) and returning a
        subvolume-absolute path.

        Due to a limitation of `path()` this will currently fail on any
        components that are absolute symlinks, but there's no strong
        incentive to do the more complex correct implementation (yet).
        """
        assert self._exists, f"{self.path()} does not exist"
        root = self.path().realpath()
        rel = self.path(path).realpath()
        if rel == root:
            return Path("/")
        assert rel.startswith(root + b"/"), (rel, root)
        return Path("/") / rel.relpath(root)

    # This differs from the regular `subprocess.Popen` interface in these ways:
    #   - stdout maps to stderr by default (to protect the caller's stdout),
    #   - `check` is supported, and default to `True`,
    #   - `cwd` is prohibited.
    #
    # `_subvol_exists` is a private kwarg letting us `run_as_root` to create
    # new subvolumes, and not just touch existing ones.
    @contextmanager
    def popen_as_root(
        self,
        args,
        *,
        _subvol_exists: bool = True,
        stdout=None,
        check: bool = True,
        **kwargs,
    ):
        if "cwd" in kwargs:
            raise AssertionError(
                "cwd= is not permitted as an argument to run_as_root, "
                "because that makes it too easy to accidentally traverse "
                "a symlink from inside the container and touch the host "
                "filesystem. Best practice: wrap your path with "
                "Subvol.path() as close as possible to its site of use."
            )
        if "pass_fds" in kwargs:
            # Future: if you add support for this, see the note on how to
            # improve `receive`, too.
            #
            # Why doesn't `pass_fds` just work?  `sudo` closes all FDs in a
            # (likely misguided) attempt to improve security.  `sudo -C` can
            # help here, but it's disabled by default.
            raise NotImplementedError(  # pragma: no cover
                "But there is a straightforward fix -- you would need to "
                "move the usage of our FD-passing wrapper from "
                "nspawn_in_subvol.py to this function."
            )
        if _subvol_exists != self._exists:
            raise AssertionError(
                f"{self.path()} exists is {self._exists}, not {_subvol_exists}"
            )
        # Ban our subcommands from writing to stdout, since many of our
        # tools (e.g. make-demo-sendstream, compiler) write structured
        # data to stdout to be usable in pipelines.
        if stdout is None:
            stdout = 2
        # The '--' is to avoid `args` from accidentally being parsed as
        # environment variables or `sudo` options.
        with subprocess.Popen(
            # Clobber any pre-existing `TMP` because in the context of Buck,
            # this is often set to something inside the repo's `buck-out`
            # (as documented in https://buck.build/rule/genrule.html).
            # Using the in-repo temporary directory causes a variety of
            # issues, including (i) `yum` leaking root-owned files into
            # `buck-out`, breaking `buck clean`, and (ii) `systemd-nspawn`
            # bugging out with "Failed to create inaccessible file node"
            # when we use `--bind-repo-ro`.
            ["sudo", "TMP=", "--", *args],
            stdout=stdout,
            **kwargs,
        ) as pr:
            yield pr
        if check:
            check_popen_returncode(pr)

    def run_as_root(
        self,
        args,
        timeout=None,
        input=None,
        _subvol_exists: bool = True,
        check: bool = True,
        **kwargs,
    ):
        """
        Run a command against an image.  IMPORTANT: You MUST wrap all image
        paths with `Subvol.path`, see that function's docblock.

        Mostly API-compatible with subprocess.run, except that:
            - `check` defaults to True instead of False,
            - `stdout` is redirected to stderr by default,
            - `cwd` is prohibited.
        """
        # IMPORTANT: Any logic that CAN go in popen_as_root, MUST go there.
        if input:
            assert "stdin" not in kwargs
            kwargs["stdin"] = subprocess.PIPE
        with self.popen_as_root(
            args, _subvol_exists=_subvol_exists, check=check, **kwargs
        ) as proc:
            stdout, stderr = proc.communicate(timeout=timeout, input=input)
        return subprocess.CompletedProcess(
            args=proc.args,
            returncode=proc.returncode,
            stdout=stdout,
            stderr=stderr,
        )

    # Future: run_in_image()

    # From here on out, every public method directly maps to the btrfs API.
    # For now, we shell out, but in the future, we may talk to a privileged
    # `btrfsutil` helper, or use `guestfs`.

    def create(self) -> "Subvol":
        self.run_as_root(
            ["btrfs", "subvolume", "create", self.path()], _subvol_exists=False
        )
        self._mark_created()
        return self

    @contextmanager
    def maybe_create_externally(self) -> Iterator[None]:
        assert not self._exists, self._path
        assert not os.path.exists(self._path), self._path
        try:
            yield
        finally:
            if os.path.exists(self._path):
                self._mark_created()

    def snapshot(self, source: "Subvol") -> "Subvol":
        # Since `snapshot` has awkward semantics around the `dest`,
        # `_subvol_exists` won't be enough and we ought to ensure that the
        # path physically does not exist.  This needs to run as root, since
        # `os.path.exists` may not have the right permissions.
        self.run_as_root(["test", "!", "-e", self.path()], _subvol_exists=False)
        self.run_as_root(
            ["btrfs", "subvolume", "snapshot", source.path(), self.path()],
            _subvol_exists=False,
        )
        self._mark_created()
        return self

    @contextmanager
    def delete_on_exit(self) -> Iterator["Subvol"]:
        "Delete the subvol if it exists when exiting the context."
        try:
            yield self
        finally:
            if self._exists:
                self.delete()

    def delete(self) -> None:
        """
        This will delete the subvol AND all nested/inner subvolumes that
        exist underneath this subvol.

        This fails if the `Subvol` does not exist.  This is because normal
        business logic explicit deletion can safely assume that the `Subvol`
        was already created.  This is a built-in correctness check.

        For "cleanup" logic, check out `delete_on_exit`.
        """
        assert self._exists, self._path

        # Set RW from the outermost to the innermost
        subvols = list(self._gen_inner_subvol_paths())
        self.set_readonly(False)
        for inner_path in sorted(subvols):
            assert _path_is_btrfs_subvol(inner_path), inner_path
            self.run_as_root(
                ["btrfs", "property", "set", "-ts", inner_path, "ro", "false"]
            )
        # Delete from the innermost to the outermost
        for inner_path in sorted(subvols, reverse=True):
            uuid = _query_uuid(self, inner_path)
            self.run_as_root(["btrfs", "subvolume", "delete", inner_path])
            # Will succeed even if this subvolume was created by a
            # subcommand, and is not tracked in `_UUID_TO_SUBVOLS`
            _mark_deleted(uuid)

        self.run_as_root(["btrfs", "subvolume", "delete", self.path()])
        self._mark_deleted()

    def _gen_inner_subvol_paths(self) -> Iterable[Path]:
        """
        Implementation detail for `delete`.

        The intent of the code below is to make as many assertions as
        possible to avoid accidentally deleting a subvolume that's not a
         descendant of `self.` So, we write many assertions.  Additionally,
        this gets some implicit safeguards from other `Subvol` methods.
          - `.path` checks the inner subvol paths to ensure they're not
            traversing symlinks to go outside of the subvol.
          - The fact that `Subvol` exists means that we checked that it's a
            subvolume at construction time -- this is important since `btrfs
            subvol list -o` returns bad results for non-subvolume paths.
            Moreover, our `btrfs subvol show` reconfirms it.
        """
        # `btrfs subvol {show,list}` both use the subvolume's path relative
        # to the volume root.
        my_rel_to_vol_root, _ = self.run_as_root(
            ["btrfs", "subvolume", "show", self.path()], stdout=subprocess.PIPE
        ).stdout.split(b"\n", 1)
        my_path = self.path()

        # NB: The below will fire if `Subvol` is the root subvol, since its
        # relative path is `/`.  However, that's not a case we need to
        # support in any foreseeable future, and it would also require
        # special-casing in the prefix search logic.
        assert not my_rel_to_vol_root.startswith(b"/"), my_rel_to_vol_root

        # Depending on how this subvolume has been mounted and is being used
        # the interaction between the `btrfs subvolume show` path (the first
        # line of `btrfs subvolume show` is what we care about) and this
        # subvolume path (`self.path()`) is different. The cases we have to
        # solve for are as it relates to inner subvolumes are:
        #  - This subvolume is used as the "root" subvol for a container
        #    and inner subvols are created within that container.
        #    This is what happens with `nspawn_in_subvol`, ie: as part of an
        #    `image_*_unittest`, `image.genrule`, or via a `=container`
        #    `buck run` target.  In this case the btrfs volume is mounted
        #    using a `subvol=` mount option, resulting in the mount "seeing"
        #    only the contents of the selected subvol.
        #  - This subvol is used on the *host* machine (where `buck` runs)
        #    and inner subvols are created.  This is the standard case for
        #    `*_unittest` targets since those are executed in the host context.
        #    In this case the btrfs volume is mounted such that the `FS_TREE`
        #    subvol (id=5) is used resulting in the mount "seeing" *all*
        #    of the subvols contained within the volume.

        # In this case the output of `btrfs subvolume show` looks something
        # like this (taken from running the `:test-subvol-utils` test):
        #
        #  tmp/delete_recursiveo7x56sn2/outer
        #      Name:                outer
        #      UUID:                aa2d8590-ba00-8a45-aee2-c1553f3dd292
        #      Parent UUID:         -
        #      Received UUID:       -
        #      Creation time:       2021-05-18 08:07:17 -0700
        #      Subvolume ID:        323
        #      Generation:     92
        #      Gen at creation:     89
        #      Parent ID:      5
        #      Top level ID:        5
        #      Flags:               -
        #      Snapshot(s):
        # and `my_path` looks something like this:
        #  /data/users/lsalis/fbsource/fbcode/buck-image-out/volume/tmp/delete_recursiveo7x56sn2/outer # noqa: E501
        vol_mounted_at_fstree = my_path.endswith(b"/" + my_rel_to_vol_root)

        # In this case the output of `btrfs subvolume show` looks something
        # like this (taken from running the `:test-subvol-utils-inner` test):
        #
        #
        #  tmp/TempSubvolumes_wk81xmx0/test-subvol-utils-inner__test_layer:Jb__IyU.HzvZ.p73f/delete_recursiveotwxda64/outer # noqa: E501
        #      Name:                outer
        #      UUID:                76866b7c-c4cc-1d4b-bafa-6aa6f898de16
        #      Parent UUID:         -
        #      Received UUID:       -
        #      Creation time:       2021-05-18 08:04:01 -0700
        #      Subvolume ID:        319
        #      Generation:     87
        #      Gen at creation:     84
        #      Parent ID:      318
        #      Top level ID:        318
        #      Flags:               -
        #      Snapshot(s):
        #
        # and `my_path` looks something like this:
        #  /delete_recursiveotwxda64/outer
        vol_mounted_at_subvol = my_rel_to_vol_root.endswith(my_path)

        assert vol_mounted_at_fstree ^ vol_mounted_at_subvol, (
            "Unexpected paths calculated from btrfs subvolume show: "
            f"{my_rel_to_vol_root}, {my_path}"
        )

        # In either case we need to calculate what the proper vol_dir is, this
        # is used below to list all the subvolumes that the volume contains
        # and filter out subvolumes that are "inside" this subvol.

        # If the volume has been mounted as an fstree (see the comments above)
        # then we want to list subvols below the "root" of the volume, which is
        # right above the path returned by `btrfs subvolume show`.
        # Example `btrfs subvolume list` (taken from `:test-subvol-utils`):
        #
        # ]# btrfs subvolume list /data/users/lsalis/fbsource/fbcode/buck-image-out/volume/ # noqa: E501
        # ID 260 gen 20 top level 5 path targets/test-layer:Jb__FIQ.HyZR.fkyU/volume # noqa: E501
        # ID 266 gen 83 top level 5 path targets/test-subvol-utils-inner__test_layer:Jb__IyU.HzvZ.p73f/volume # noqa: E501
        # ID 272 gen 64 top level 5 path targets/build-appliance.c7:Jb__hV4.H42o.pR_O/volume # noqa: E501
        # ID 300 gen 66 top level 5 path targets/build_appliance_testingprecursor-without-caches-to-build_appliance_testing:Jb__o1c.H8Bc.ASOl/volume # noqa: E501
        # ID 307 gen 70 top level 5 path targets/build_appliance_testing:Jb__rtA.H89Z.j0z3/volume # noqa: E501
        # ID 308 gen 72 top level 5 path targets/hello_world_base:Jb__u0g.H9yB.t9oN/volume # noqa: E501
        # ID 323 gen 92 top level 5 path tmp/delete_recursiveo7x56sn2/outer
        # ID 324 gen 91 top level 323 path tmp/delete_recursiveo7x56sn2/outer/inner1 # noqa: E501
        # ID 325 gen 91 top level 324 path tmp/delete_recursiveo7x56sn2/outer/inner1/inner2 # noqa: E501
        # ID 326 gen 92 top level 323 path tmp/delete_recursiveo7x56sn2/outer/inner3 # noqa: E501
        # ]#
        if vol_mounted_at_fstree:
            vol_dir = my_path[: -len(my_rel_to_vol_root)]
            my_prefix = my_rel_to_vol_root

        # If the volume has been mounted at a specific subvol (see the comments
        # above).  Then we want to list subvols below `/` since that is seen
        # to be the "root" of the volume.
        # Example `btrfs subvolume list` taken from `:test-subvol-utils-inner`:
        #
        # ]# btrfs subvolume list /
        # ID 260 gen 20 top level 5 path targets/test-layer:Jb__FIQ.HyZR.fkyU/volume # noqa: E501
        # ID 266 gen 83 top level 5 path targets/test-subvol-utils-inner__test_layer:Jb__IyU.HzvZ.p73f/volume # noqa: E501
        # ID 272 gen 64 top level 5 path targets/build-appliance.c7:Jb__hV4.H42o.pR_O/volume # noqa: E501
        # ID 300 gen 66 top level 5 path targets/build_appliance_testingprecursor-without-caches-to-build_appliance_testing:Jb__o1c.H8Bc.ASOl/volume # noqa: E501
        # ID 307 gen 70 top level 5 path targets/build_appliance_testing:Jb__rtA.H89Z.j0z3/volume # noqa: E501
        # ID 308 gen 72 top level 5 path targets/hello_world_base:Jb__u0g.H9yB.t9oN/volume # noqa: E501
        # ID 318 gen 84 top level 5 path tmp/TempSubvolumes_wk81xmx0/test-subvol-utils-inner__test_layer:Jb__IyU.HzvZ.p73f # noqa: E501
        # ID 319 gen 87 top level 318 path delete_recursiveotwxda64/outer
        # ID 320 gen 86 top level 319 path delete_recursiveotwxda64/outer/inner1 # noqa: E501
        # ID 321 gen 86 top level 320 path delete_recursiveotwxda64/outer/inner1/inner2 # noqa: E501
        # ID 322 gen 87 top level 319 path delete_recursiveotwxda64/outer/inner3 # noqa: E501
        # ]#
        # Note: code coverage for this branch is in the
        # :test-subvol-utils-inner test, but because of the way
        # coverage works I can't properly cover this in the larger
        # :test-subvol-utils test.
        elif vol_mounted_at_subvol:  # pragma: nocover
            vol_dir = b"/"
            my_prefix = my_path[1:]

        # We need a trailing slash to chop off this path prefix below.
        my_prefix = my_prefix + (b"" if my_prefix.endswith(b"/") else b"/")

        # NB: The `-o` option does not work correctly, don't even bother.
        for inner_line in self.run_as_root(
            ["btrfs", "subvolume", "list", vol_dir], stdout=subprocess.PIPE
        ).stdout.split(b"\n"):
            if not inner_line:  # Handle the trailing newline
                continue
            l = {}  # Used to check that the labels are as expected
            (
                l["ID"],
                _,
                l["gen"],
                _,
                l["top"],
                l["level"],
                _,
                l["path"],
                p,
            ) = inner_line.split(b" ", 8)
            for k, v in l.items():
                assert k.encode() == v, (k, v)

            if not p.startswith(my_prefix):  # Skip non-inner subvolumes
                continue

            inner_subvol = p[len(my_prefix) :]
            assert inner_subvol == os.path.normpath(inner_subvol), inner_subvol
            yield self.path(inner_subvol)

    def set_readonly(self, readonly: bool) -> None:
        self.run_as_root(
            [
                "btrfs",
                "property",
                "set",
                "-ts",
                self.path(),
                "ro",
                "true" if readonly else "false",
            ]
        )

    def sync(self) -> None:
        self.run_as_root(["btrfs", "filesystem", "sync", self.path()])

    @contextmanager
    def _mark_readonly_and_send(
        self,
        *,
        stdout,
        no_data: bool = False,
        # pyre-fixme[9]: parent has type `Subvol`; used as `None`.
        parent: "Subvol" = None,
    ) -> Iterator[subprocess.Popen]:
        self.set_readonly(True)

        # Btrfs bug #25329702: in some cases, a `send` without a sync will
        # violate read-after-write consistency and send a "past" view of the
        # filesystem.  Do this on the read-only filesystem to improve
        # consistency.
        self.sync()

        # Btrfs bug #25379871: our 4.6 kernels have an experimental xattr
        # caching patch, which is broken, and results in xattrs not showing
        # up in the `send` stream unless that metadata is `fsync`ed.  For
        # some dogscience reason, `getfattr` on a file actually triggers
        # such an `fsync`.  We do this on a read-only filesystem to improve
        # consistency. Coverage: manually tested this on a 4.11 machine:
        #   platform.uname().release.startswith('4.11.')
        if platform.uname().release.startswith("4.6."):  # pragma: no cover
            self.run_as_root(
                [
                    # Symlinks can point outside of the subvol, don't follow
                    # them
                    "getfattr",
                    "--no-dereference",
                    "--recursive",
                    self.path(),
                ]
            )

        with self.popen_as_root(
            [
                "btrfs",
                "send",
                *(["--no-data"] if no_data else []),
                *(["-p", parent.path()] if parent else []),
                self.path(),
            ],
            stdout=stdout,
        ) as proc:
            yield proc

    def mark_readonly_and_get_sendstream(self, **kwargs) -> bytes:
        with self._mark_readonly_and_send(
            stdout=subprocess.PIPE, **kwargs
        ) as proc:
            # pyre-fixme[16]: Optional type has no attribute `read`.
            return proc.stdout.read()

    @contextmanager
    def mark_readonly_and_write_sendstream_to_file(
        self, outfile: BinaryIO, **kwargs
    ) -> Iterator[None]:
        with self._mark_readonly_and_send(stdout=outfile, **kwargs):
            yield

    @contextmanager
    def write_tarball_to_file(
        self, outfile: BinaryIO, **kwargs
    ) -> Iterator[None]:
        # gnu tar has a nasty bug where even if you specify `--one-file-system`
        # it still tries to perform various operations on other mount points.
        # The problem with this is that some filesystem types don't support all
        # of the various fs layer calls needed, like `flistxattr` or `savedir`
        # in the case of the `gvfs` file system.
        # Because layer mounts or host mounts are currently setup in the root
        # namespace, when we try to archive this subvol, we might run into these
        # kinds of mounts.  So to work around this, we start a new mount
        # namespace, unmount everything that is under this subvol, and then
        # run the tar command.
        with self.popen_as_root(
            [
                "unshare",
                "--mount",
                "bash",
                "-c",
                # Unmount everything that contains the subvol name, that's the
                # dir *above* the `volume/` path.
                "(mount |"
                f" grep {self.path().dirname().basename()} |"
                " xargs umount "
                ")1>&2; "  # Make sure any errors in the umounts go to stderr
                "tar c "
                "--sparse "
                "--one-file-system "  # Keep this just in case
                "--acls "
                "--xattrs "
                "--to-stdout "
                "-C "
                f"{self.path()} "
                ".",
            ],
            stdout=outfile,
        ):
            yield

    def estimate_content_bytes(self) -> int:
        """
        Returns a (usually) tight lower-bound guess of the filesystem size
        necessary to contain this subvolume.  The caller is responsible for
        appropriately padding this size when creating the destination FS.

        ## Future: Query the subvolume qgroup to estimate its size

          - If quotas are enabled, this should be an `O(1)` operation
            instead of the more costly filesystem tree traversal.  NB:
            qgroup size estimates tend to run a bit (~1%) lower than `du`,
            so growth factors may need a tweak.  `estimate_content_bytes()`
            should `log.warning` and fall back to `du` if quotas are
            disabled in an older `buck-image-out`.  It's also an option to
            enable quotas and to trigger a `rescan -w`, but requires more
            code/testing.

          - Using qgroups for builds is a good stress test of the qgroup
            subsystem. It would help us gain confidence in that (a) they
            don't trigger overt issues (filesystem corruption, dramatic perf
            degradation, or crashes), and that (b) they report reasonably
            accurate numbers on I/O-stressed build hosts.

          - Should run an A/B test to see if the build perf wins of querying
            qgroups exceed the perf hit of having quotas enabled.

          - Eventually, we'd enable quotas by default for `buck-image-out`
            volumes.

          - Need to delete the qgroup whenever we delete a subvolume.  Two
            main cases: `Subvol.delete` and `subvolume_garbage_collector.py`.
            Can check if we are leaking cgroups by building & running &
            image tests, and looking to see if that leaves behind 0-sized
            cgroups unaffiliated with subvolumes.

          - The basic logic for qgroups looks like this:

            $ sudo btrfs subvol show buck-image-out/volume/example |
                grep 'Subvolume ID'
                    Subvolume ID:           1432

            $ sudo btrfs qgroup show --raw --sync buck-image-out/volume/ |
                grep ^0/1432
            0/1432     1381523456        16384
            # We want the second column, bytes in referenced extents.

            # For the `qgroup show` operation, check for **at least** these
            # error signals on stderr -- with exit code 1:
            ERROR: can't list qgroups: quotas not enabled
            # ... and with exit code 0:
            WARNING: qgroup data inconsistent, rescan recommended
            WARNING: rescan is running, qgroup data may be incorrect
            # Moreover, I would have the build fail on any unknown output.
        """
        # Not adding `-x` since buck-built subvolumes should not have other
        # filesystems mounted inside them.
        start_time = time.time()
        du_out = subprocess.check_output(
            [
                "sudo",
                "du",
                "--block-size=1",
                "--summarize",
                # Hack alert: `--one-file-system` works around the fact that we
                # may have host mounts inside the image, which we mustn't count.
                "--one-file-system",
                self._path,
            ]
        ).split(b"\t", 1)
        assert du_out[1] == self._path + b"\n"
        size = int(du_out[0])
        log.info(
            f"`du` estimated size of {self._path} as {size} in "
            f"{time.time() - start_time} seconds"
        )
        return size

    @contextmanager
    def receive(self, from_file) -> Iterator[None]:
        # At present, we always have an empty wrapper dir to receive into.
        # If this changes, we could make a tempdir inside `parent_fd`.
        with open_fd(
            os.path.dirname(self.path()), os.O_RDONLY | os.O_DIRECTORY
        ) as parent_fd:
            wrapper_dir_contents = os.listdir(parent_fd)
            assert wrapper_dir_contents == [], wrapper_dir_contents
            try:
                with self.popen_as_root(
                    [
                        "btrfs",
                        "receive",
                        # Future: If we get `pass_fds` support, use
                        # `/proc/self/fd'
                        Path("/proc")
                        / str(os.getpid())
                        / "fd"
                        / str(parent_fd),
                    ],
                    _subvol_exists=False,
                    stdin=from_file,
                ):
                    yield
            finally:
                received_names = os.listdir(parent_fd)
                assert len(received_names) <= 1, received_names
                if received_names:
                    os.rename(
                        received_names[0],
                        os.path.basename(self.path()),
                        src_dir_fd=parent_fd,
                        dst_dir_fd=parent_fd,
                    )
                    # This may be a **partially received** subvol.  If these
                    # semantics turn out to be broken for our purposes, we
                    # can try to clean up the subvolume on error instead,
                    # but at present it seems easier to leak it, and let the
                    # GC code delete it later.
                    self._mark_created()

    def read_path_text(self, relpath: Path) -> str:
        return self.path(relpath).read_text()

    def read_path_text_as_root(self, relpath: Path) -> str:
        # To duplicate the effects of read_path_text(), we need to first check
        # for the existence of the file and maybe return FileNotFoundError.
        # Otherwise we will end up with a CalledProcessError propagating up.
        if not self.path(relpath).exists():
            raise FileNotFoundError(relpath)

        res = self.run_as_root(
            ["cat", self.path(relpath)],
            text=True,
            stdout=subprocess.PIPE,
        )
        res.check_returncode()
        return res.stdout

    def overwrite_path_as_root(self, relpath: Path, contents: AnyStr) -> None:
        # Future: support setting user, group, and mode
        if isinstance(contents, str):
            contents = contents.encode()
        self.run_as_root(
            ["tee", self.path(relpath)],
            input=contents,
            stdout=subprocess.DEVNULL,
        ).check_returncode()


def with_temp_subvols(method):
    """
    A test that needs a TempSubvolumes instance should use this decorator.
    This is a cleaner alternative to doing this in setUp:

        self.temp_subvols.__enter__()
        self.addCleanup(self.temp_subvols.__exit__, None, None, None)

    The primary reason this is bad is explained in the TempSubvolumes
    docblock. It also fails to pass exception info to the __exit__.
    """

    @functools.wraps(method)
    def decorated(self, *args, **kwargs):
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvols:
            return method(self, temp_subvols, *args, **kwargs)

    return decorated


# NB: Memoizing this function would be pretty reasonable.
def volume_dir(path_in_repo: Optional[Path] = None) -> Path:
    return find_artifacts_dir(path_in_repo) / "volume"


def _tmp_volume_dir(path_in_repo: Optional[Path] = None) -> Path:
    return volume_dir(path_in_repo) / "tmp"


class TempSubvolumes:
    """
    Tracks the subvolumes it creates, and destroys them on context exit.

    BEST PRACTICES:

      - To nest a subvol inside one made by `TempSubvolumes`, create it
        via `Subvol` -- bypassing `TempSubvolumes`.  It is better to let it
        be cleaned up implicitly.  If you request explicit cleanup by using
        a `TempSubvolumes` method, the inner subvol would have to be deleted
        first, which would break if the parent is read-only.  See an example
        in `test_temp_subvolumes_create` (marked by "NB").

      - Avoid using `unittest.TestCase.addCleanup` to `__exit__()` this
        context.  Instead, use `@with_temp_subvols` on each test method.

        `addCleanup` is unreliable -- e.g.  clean-up is NOT triggered on
        KeyboardInterrupt.  Therefore, this **will** leak subvolumes during
        development.  For manual cleanup:

        sudo btrfs sub del buck-image-out/volume/tmp/TempSubvolumes_*/subvol &&
            rmdir buck-image-out/volume/tmp/TempSubvolumes_*

        Instead of polluting `buck-image-out/volume`, it  would be possible to
        put these on a separate `BtrfsLoopbackVolume`, to rely on `Unshare` to
        guarantee unmounting it, and to rely on `tmpwatch` to delete the stale
        loopbacks from `/tmp/`.  At present, this doesn't seem worthwhile since
        it would require using an `Unshare` object throughout `Subvol`.
    """

    def __init__(self, path_in_repo: Optional[Path] = None) -> None:
        super().__init__()
        # The 'tmp' subdirectory simplifies cleanup of leaked temp subvolumes
        volume_tmp_dir = _tmp_volume_dir(path_in_repo)
        try:
            os.mkdir(volume_tmp_dir)
        except FileExistsError:
            pass
        self._stack = ExitStack()
        self._temp_dir_ctx = temp_dir(
            dir=volume_tmp_dir.decode(), prefix=self.__class__.__name__ + "_"
        )

    def __enter__(self) -> "TempSubvolumes":
        self._stack.__enter__()
        # pyre-fixme[16]: `TempSubvolumes` has no attribute `_temp_dir`.
        self._temp_dir = self._stack.enter_context(self._temp_dir_ctx)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        # pyre-fixme[16]: `TempSubvolumes` has no attribute `_temp_dir`.
        self._temp_dir = None
        return self._stack.__exit__(exc_type, exc_val, exc_tb)

    def _new_subvol(self, subvol):
        return self._stack.enter_context(subvol.delete_on_exit())

    @property
    def temp_dir(self):
        return self._temp_dir

    def _prep_rel_path(self, rel_path: AnyStr) -> Path:
        """
        Ensures subvolumes live under our temporary directory, which
        improves safety, since its permissions ought to be u+rwx to avoid
        exposing setuid binaries inside the built subvolumes.
        """
        rel_path = (
            (self.temp_dir / rel_path)
            .realpath()
            .relpath(self.temp_dir.realpath())
        )
        if rel_path.has_leading_dot_dot():
            raise AssertionError(
                f"{rel_path} must be a subdirectory of {self.temp_dir}"
            )
        abs_path = self.temp_dir / rel_path
        os.makedirs(abs_path.dirname(), exist_ok=True)
        return abs_path

    def create(self, rel_path: AnyStr) -> Subvol:
        return self._new_subvol(Subvol(self._prep_rel_path(rel_path)).create())

    def snapshot(self, source: Subvol, dest_rel_path: AnyStr) -> Subvol:
        return self._new_subvol(
            Subvol(self._prep_rel_path(dest_rel_path)).snapshot(source)
        )

    def caller_will_create(self, rel_path: AnyStr) -> Subvol:
        return self._new_subvol(Subvol(self._prep_rel_path(rel_path)))


def get_subvolumes_dir(
    path_in_repo: Optional[Path] = None,
) -> Path:
    return volume_dir(path_in_repo) / "targets"


def find_subvolume_on_disk(
    layer_output: str,
    # pyre-fixme[9]: path_in_repo has type `Path`; used as `None`.
    path_in_repo: Path = None,
    # pyre-fixme[9]: subvolumes_dir has type `Path`; used as `None`.
    subvolumes_dir: Path = None,
) -> SubvolumeOnDisk:
    # It's OK for both to be None (uses the current file to find repo), but
    # it's not OK to set both.
    assert (path_in_repo is None) or (subvolumes_dir is None)
    with open(Path(layer_output) / "layer.json") as infile:
        return SubvolumeOnDisk.from_json_file(
            infile, subvolumes_dir or get_subvolumes_dir(path_in_repo)
        )
