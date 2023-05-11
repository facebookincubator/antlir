#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import errno
import functools
import os
import platform
import subprocess
import sys
import time
from contextlib import contextmanager, ExitStack
from typing import AnyStr, BinaryIO, Iterator, Optional, TypeVar, Union

import antlir.btrfsutil as btrfsutil
from antlir.artifacts_dir import find_artifacts_dir
from antlir.btrfs_diff.freeze import DoNotFreeze
from antlir.common import check_popen_returncode, get_logger, open_fd
from antlir.compiler.subvolume_on_disk import SubvolumeOnDisk

from antlir.errors import InfraError
from antlir.fs_utils import Path, temp_dir


log = get_logger()
KiB = 2**10
MiB = 2**20


# Exposed as a helper so that test_compiler.py can mock it.
def _path_is_btrfs_subvol(path: Path) -> bool:
    try:
        return btrfsutil.is_subvolume(path)
    except btrfsutil.BtrfsUtilError as be:
        # if the path simply doesn't exist or is not a directory, then it's
        # obviously not a subvolume
        if be.errno in (errno.ENOENT, errno.ENOTDIR):
            return False
        # any other error is bad and should be raised instead of ignored
        raise  # pragma: no cover


T = TypeVar


# Subvol is marked as `DoNotFreeze` as its hash is just of
# byte string that contains the path to the subvol. Its member
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
        """
        self._path = Path(path).abspath()
        if already_exists:
            if not self._exists:
                raise AssertionError(f"No btrfs subvol at {self._path}")
        elif not _test_only_allow_existing:
            assert not self._exists, self._path

    @property
    def _exists(self):
        return _path_is_btrfs_subvol(self._path)

    def __eq__(self, other: "Subvol") -> bool:
        assert self._exists == other._exists, self.path()
        return self._path == other._path

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
        btrfsutil.create_subvolume(self.path())
        return self

    @contextmanager
    def maybe_create_externally(self) -> Iterator[None]:
        assert not self._exists, self._path
        yield

    def snapshot(self, source: "Subvol") -> "Subvol":
        btrfsutil.create_snapshot(source.path(), self.path())
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

        If this `Subvol` does not exist on disk at the time, this is a no-op.
        This way we can use btrfsutil to iterate over children subvolumes
        and potentially attempt "concurrent" deletions, instead of having to
        track them ourselves.

        For "cleanup" logic, check out `delete_on_exit`.
        """
        assert self._exists, self._path

        # Any child subvolumes must be marked as read-write before the parent
        # can be deleted
        for (child_path, _) in btrfsutil.SubvolumeIterator(self._path, post_order=True):
            child_path = self._path / child_path
            btrfsutil.set_subvolume_read_only(child_path, False)

        try:
            # SubvolumeIterator does not yield the subvol it was given, only
            # children, so the the parent subvol must be marked as read-write as
            # well before starting the recursive delete
            btrfsutil.set_subvolume_read_only(self._path, False)
            btrfsutil.delete_subvolume(self._path, recursive=True)
        except btrfsutil.BtrfsUtilError as e:
            # this subvol may have been deleted already, in which case our job
            # is done
            if e.errno != errno.ENOENT:
                raise e

    def set_readonly(self, readonly: bool) -> None:
        btrfsutil.set_subvolume_read_only(self.path(), readonly)

    def sync(self) -> None:
        btrfsutil.sync(self.path())

    @contextmanager
    def _mark_readonly_and_send(
        self,
        *,
        stdout,
        # The protocol version for btrfs send:
        # Setting this to 0 will not explicitly set the proto
        # Setting this to 2 will generate a sendstream with encoded writes
        proto: int = 0,
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

        send_args = [
            "btrfs",
            "send",
            *(["--no-data"] if no_data else []),
            *(["-p", parent.path()] if parent else []),
            *(["--proto", str(proto)] if proto != 0 else []),
            *(["--compressed-data"] if proto == 2 else []),
            self.path(),
        ]

        log.debug(f"arguments to `btrfs send` are {send_args}")

        # `btrfs send` can fail if there is a rebalancing operation in progress.
        # Retry up to 3 times 30s apart to hopefully get a working send after
        # the rebalancing operation is complete. This is fairly arbitrary but
        # was recommended by Kernel folks.
        with self.popen_as_root(
            send_args,
            stdout=stdout,
        ) as proc:
            yield proc

    def mark_readonly_and_get_sendstream(self, **kwargs) -> bytes:
        with self._mark_readonly_and_send(stdout=subprocess.PIPE, **kwargs) as proc:
            # pyre-fixme[16]: Optional type has no attribute `read`.
            return proc.stdout.read()

    def mark_readonly_and_write_sendstream_to_file(
        self,
        outfile,
        _retries=3,
        **kwargs,
    ) -> None:
        def _maybe_retry():  # pragma: no cover
            if _retries <= 0:
                raise InfraError("'btrfs send' failed too many times")
            log.warning(f"'btrfs send' failed, retries remaining = {_retries}")
            time.sleep(30)
            self.mark_readonly_and_write_sendstream_to_file(
                outfile=outfile, _retries=_retries - 1, **kwargs
            )

        try:
            with self._mark_readonly_and_send(stdout=outfile, **kwargs) as proc:
                code = proc.wait()
            if code != 0:  # pragma: no cover
                _maybe_retry()
            return
        except subprocess.CalledProcessError:  # pragma: no cover
            _maybe_retry()

    @contextmanager
    def write_tarball_to_file(self, outfile: BinaryIO, **kwargs) -> Iterator[None]:
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
                        Path("/proc") / str(os.getpid()) / "fd" / str(parent_fd),
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

    def read_path_text(self, relpath: Path) -> str:
        return self.path(relpath).read_text()

    def overwrite_path_as_root(self, relpath: Path, contents: AnyStr) -> None:
        # Future: support setting user, group, and mode
        if isinstance(contents, str):
            contents = contents.encode()
        assert isinstance(contents, bytes)
        try:
            with open(self.path(relpath), "wb") as f:
                f.write(contents)
        # TODO: does this branch ever happen? Can we just delete it?
        except PermissionError:  # pragma: no cover
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
    """

    def __init__(self, path_in_repo: Optional[Path] = None) -> None:
        super().__init__()
        # The 'tmp' subdirectory simplifies cleanup of leaked temp subvolumes
        volume_tmp_dir = _tmp_volume_dir(path_in_repo)
        os.makedirs(volume_tmp_dir, exist_ok=True)
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
            (self.temp_dir / rel_path).realpath().relpath(self.temp_dir.realpath())
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
    layer_output: Union[str, Path],
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
