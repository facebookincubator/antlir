#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import contextlib
import functools
import logging
import os
import platform
import subprocess
import sys
import time
from contextlib import contextmanager
from typing import AnyStr, BinaryIO, Iterable, Iterator, NamedTuple, TypeVar

from .artifacts_dir import find_artifacts_dir
from .btrfs_loopback import LoopbackVolume, run_stdout_to_err
from .common import check_popen_returncode, get_logger, open_fd, pipe
from .fs_utils import Path, temp_dir
from .unshare import Namespace, Unshare, nsenter_as_root, nsenter_as_user


log = get_logger()
KiB = 2 ** 10
MiB = 2 ** 20


# Exposed as a helper so that test_compiler.py can mock it.
def _path_is_btrfs_subvol(path: Path) -> bool:
    "Ensure that there is a btrfs subvolume at this path. As per @kdave at "
    "https://stackoverflow.com/a/32865333"
    # You'd think I could just `os.statvfs`, but no, not until Py3.7
    # https://bugs.python.org/issue32143
    fs_type = subprocess.run(
        ["stat", "-f", "--format=%T", path], stdout=subprocess.PIPE, text=True
    ).stdout.strip()
    ino = os.stat(path).st_ino
    return fs_type == "btrfs" and ino == 256


class SubvolOpts(NamedTuple):
    # For sending subvolumes to an image, whether to leave the
    # subvolume read only
    readonly: bool = True
    # Make the resulting subvolume a seed device, where another disk can be
    # provided for writes
    seed_device: bool = False
    # Apply time-costly optimization to minimize the size of loopback image
    multi_pass_size_minimization: bool = False


_default_subvol_opts = SubvolOpts()
T = TypeVar


# This is IMPORTANT.  It does not just allow us to slightly reduce the waste
# factor, but also avoids having to handling `btrfs send` getting SIGPIPE in
# certain out-of-disk contexts.
def _drain_pipe_return_byte_count(f: BinaryIO) -> int:
    # This would be better with the `splice` syscall, but that's too much work.
    chunk_size = 2 ** 19  # limit RAM usage
    total = 0
    while True:
        num_read = len(f.read(chunk_size))
        if not num_read:
            return total
        total += num_read


class Subvol:
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

    def __init__(self, path: AnyStr, already_exists=False):
        """
        `Subvol` can represent not-yet-created subvolumes.  Unless
        already_exists=True, you must call create() or snapshot() to
        actually make the subvolume.
        """
        self._path = Path(path).abspath()
        self._exists = already_exists
        if self._exists and not _path_is_btrfs_subvol(self._path):
            raise AssertionError(f"No btrfs subvol at {self._path}")

    def __eq__(self, other: "Subvol") -> bool:
        assert self._exists == other._exists, self.path()
        return self._path == other._path

    def __hash__(self) -> int:
        return hash(self._path)

    def path(
        self, path_in_subvol: AnyStr = b".", *, no_dereference_leaf=False
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
            path_in_subvol, no_dereference_leaf=no_dereference_leaf
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
        self, args, *, _subvol_exists=True, stdout=None, check=True, **kwargs
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
        _subvol_exists=True,
        check=True,
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

    def create(self):
        self.run_as_root(
            ["btrfs", "subvolume", "create", self.path()], _subvol_exists=False
        )
        self._exists = True

    def snapshot(self, source: "Subvol"):
        # Since `snapshot` has awkward semantics around the `dest`,
        # `_subvol_exists` won't be enough and we ought to ensure that the
        # path physically does not exist.  This needs to run as root, since
        # `os.path.exists` may not have the right permissions.
        self.run_as_root(["test", "!", "-e", self.path()], _subvol_exists=False)
        self.run_as_root(
            ["btrfs", "subvolume", "snapshot", source.path(), self.path()],
            _subvol_exists=False,
        )
        self._exists = True

    def delete(self):
        self.run_as_root(["btrfs", "subvolume", "delete", self.path()])
        self._exists = False

    def _delete_inner_subvols(self):
        """
        This is currently a private call, separate from `delete` because
        it's only used by `TempSubvolumes` for cleanup of container-created
        subvolumes, which are owned by neither `TempSubvolumes` nor `Subvol`.
        Adding a "production-ready" variant of this would require more
        attention to the API and usage. Likely TODOs:
          - Future / fix me: We currently will fail to delete read-only
            inner subvolumes.  Fixing this requires first walking the
            subvols in the **opposite** order (parent to child), and marking
            them read-write (and adding tests).  This is not done because we
            currently have no use-case.
          - If used outside of its current application, it could delete an
            inner subvolume that is already managed by another `Subvol`
            object wihout updating its `_exists`.
        """
        # Delete from the innermost to the outermost
        for inner_path in sorted(self._gen_inner_subvol_paths(), reverse=True):
            assert _path_is_btrfs_subvol(inner_path), inner_path
            self.run_as_root(["btrfs", "subvolume", "delete", inner_path])

    def _gen_inner_subvol_paths(self) -> Iterable[Path]:
        """
        Implementation detail for `_delete_inner_subvols`, do not use
        otherwise without strengthening the API contract & tests.

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
        my_rel, _ = self.run_as_root(
            ["btrfs", "subvolume", "show", self.path()], stdout=subprocess.PIPE
        ).stdout.split(b"\n", 1)

        my_path = self.path()
        # NB: The below will fire if `Subvol` is the root subvol, since its
        # relative path is `/`.  However, that's not a case we need to
        # support in any foreseeable future, and it would also require
        # special-casing in the prefix search logic.
        assert not my_rel.startswith(b"/"), my_rel
        assert my_path.endswith(b"/" + my_rel), (my_path, my_rel)
        vol_dir = my_path[: -len(my_rel)]

        # We need a trailing slash to chop off this path prefix below.
        my_prefix = my_rel + (b"" if my_rel.endswith(b"/") else b"/")

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

    def set_readonly(self, readonly: bool):
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

    def set_seed_device(self, output_path: str):
        # Clearing the seeding flag on a device may be dangerous. If a
        # previously-seeding device is changed, all filesystems that used that
        # device will become unmountable. Setting the seeding flag back will
        # not fix that.
        # Due to this danger and the limited usefulness we don't support
        # clearing the seed flag.
        self.run_as_root(["btrfstune", "-S", "1", output_path])

    def sync(self):
        self.run_as_root(["btrfs", "filesystem", "sync", self.path()])

    @contextmanager
    def _mark_readonly_and_send(
        self, *, stdout, no_data: bool = False, parent: "Subvol" = None
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
            return proc.stdout.read()

    @contextmanager
    def mark_readonly_and_write_sendstream_to_file(
        self, outfile: BinaryIO, **kwargs
    ) -> Iterator[None]:
        with self._mark_readonly_and_send(stdout=outfile, **kwargs):
            yield

    def mark_readonly_and_send_to_new_loopback(
        self,
        output_path,
        subvol_opts: SubvolOpts = _default_subvol_opts,
        waste_factor=1.15,
    ) -> int:
        """
        Overwrites `ouput_path` with a new btrfs image, and send this
        subvolume to this new volume.  The image is populated as a loopback
        mount, which will be unmounted before this function returns.

        Since btrfs sizing facilities are unreliable, we size the new
        filesystem by guesstimating the content size of the filesystem, and
        multiplying it by `waste_factor` to ensure that `receive` does not
        run out of space.  If out-of-space does occur, this function repeats
        multiply-send-receive until we succeed, so a low `waste_factor` can
        make image builds much slower.

        ## Notes on setting `waste_factor`

          - This is exposed for unit tests, you should probably not surface
            it to users.  We should improve the auto-sizing instead.

          - Even though sparse files make it fairly cheap to allocate a
            much larger loopback than what is required to contain the
            subvolume, we want to try to keep the loopback filesystem as
            full as possible. The primary rationale is that parts of
            our image distribution filesystem do not support sparse files
            (to be fixed). Secondarily, btrfs seems to increase the
            amount of overhead it permits itself as the base filesystem
            becomes larger. I have not carefully measured the loopback
            size after accounting for sparseness, but this needs to
            be tested before considering much larger waste factors.

          - While we resize down to `min-dev-size` after populating the
            volume, setting a higher waste factor is still not free.  The
            reason is that btrfs auto-allocates more metadata blocks for
            larger filesystems, but `resize` does not release them.  So if
            you start with a larger waste factor, your post-shrink
            filesystem will be larger, too.  This is one of the reasons why
            we should just `findmnt -o SIZE` to determine a safe size of the
            loopback (the other reason is below).

          - The default of 15% is very conservative, with the goal of
            never triggering an expensive send+receive combo. This seeks to
            optimize developer happiness.  In my tests, I have not seen a
            filesystem that needed more than 5%.  Later, we can add
            monitoring and gradually dial this down.

          - If your subvolume's `_estimate_content_bytes` is X, and it
            fits in a loopback of size Y, it is not guaranteed that you
            could have used `waste_factor = Y / X`, because lazy writes make
            it possible to resize a populated filesystem to have a size
            **below** what you would have needed to populate its content.

          - There is an alternative strategy to "multiply by waste_factor &
            re-send", which is to implement a `pv`-style function that
            sits on a pipe between `send` and `receive`, and does the
            following to make sure `receive` never runs out of space:
              - `btrfs filesystem sync`, `usage`, and if "min" free space
                drops too low, `resize`
              - `splice` (via `ctypes`, or write this interposition program
                in C) a chunk from `send` to `receive`. Using `splice`
                instead of copying through userspace is not **necessarily**
                essential, but in order to minimize latency, it's important
                that we starve the `receive` end as rarely as possible,
                which may require some degree of concurrency between reading
                from `send` and writing to `receive`.  To clarify: a naive
                Python prototype that read & wrote 2MB at a time -- a buffer
                that's large enough that we'd frequently starve `receive` or
                stall `send` -- experienced a 30% increase in wall time
                compared to `send | receive`.
              - Monitor usage much more frequently than the free space to
                chunk size ratio would indicate, since something may buffer.
                Don't use a growth increment that is TOO small.
              - Since there are no absolute guarantees that btrfs won't
                run out of space on `receive`, there should still be an
                outer retry layer, but it ought to never fire.
              - Be aware that the minimum `mkfs.brfs` size is 108MiB, the
                minimum size to which we can grow via `resize` is 175MiB,
                while the minimum size to which we can shrink via `resize`
                is 256MiB, so the early growth tactics should reflect this.

            The advantage of this strategy of interposing on a pipe, if
            implemented well, is that we should be able to achieve a smaller
            waste factor without paying occasionally doubling our wall clock
            and IOP costs due to retries.  The disadvantage is that if we do
            a lot of grow cycles prior to our shrink, the resulting
            filesystem may end up being more out-of-tune than if we had
            started with a large enough size from the beginning.
        """
        # In my experiments, btrfs needs at least 81 MB of overhead in all
        # circumstances, and this initial overhead is not multiplicative.
        # To be specific, I tried single-file subvolumes with files of size
        # 27, 69, 94, 129, 175, 220MiB.
        fs_bytes = self._estimate_content_bytes() + 81 * MiB
        attempts = 0
        while True:
            attempts += 1
            fs_bytes *= waste_factor
            leftover_bytes, image_size = self._send_to_loopback_if_fits(
                output_path, int(fs_bytes), subvol_opts
            )
            if leftover_bytes == 0:
                if not subvol_opts.multi_pass_size_minimization:
                    break
                # The following simple trick saves about 30% of image size. The
                # reason is that btrfs auto-allocates more metadata blocks for
                # larger filesystems, but `resize` does not release them. For
                # many practical use-cases the compression ratio is close to 2,
                # hence initial `fs_bytes` estimate is too high.
                leftover_bytes, new_size = self._send_to_loopback_second_pass(
                    output_path, image_size, subvol_opts
                )
                assert leftover_bytes == 0, (
                    f"Cannot fit {self._path} in {image_size} bytes, "
                    f"{leftover_bytes} sendstream bytes were left over"
                )
                assert new_size <= image_size, (
                    "The second pass of btrfs send-receive produced worse"
                    f"results that the first: {new_size} vs. {image_size}"
                )
                break  # pragma: no cover
            fs_bytes += leftover_bytes
            log.warning(
                f"{self._path} did not fit in {fs_bytes} bytes, "
                f"{leftover_bytes} sendstream bytes were left over"
            )
        if subvol_opts.seed_device:
            self.set_seed_device(output_path)
        # Future: It would not be unreasonable to run some sanity checks on
        # the resulting filesystem here. Ideas:
        #  - See if we have an unexpectedly large amount of unused metadata
        #    space, or other "waste" via `btrfs filesystem usage -b` --
        #    could ask @clm if this is a frequent issue.
        #  - Can we check for fragmentation / balance issues?
        #  - We could (very occasionally & maybe in the background, since
        #    this is expensive) check that the received subvolume is
        #    identical to the source subvolume.
        return attempts

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
                "--xattrs "
                "--to-stdout "
                "-C "
                f"{self.path()} "
                ".",
            ],
            stdout=outfile,
        ):
            yield

    def _estimate_content_bytes(self):
        """
        Returns a (usually) tight lower-bound guess of the filesystem size
        necessary to contain this subvolume.  The caller is responsible for
        appropriately padding this size when creating the destination FS.

        ## Future: Query the subvolume qgroup to estimate its size

          - If quotas are enabled, this should be an `O(1)` operation
            instead of the more costly filesystem tree traversal.  NB:
            qgroup size estimates tend to run a bit (~1%) lower than `du`,
            so growth factors may need a tweak.  `_estimate_content_bytes()`
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

    # Mocking this allows tests to exercise the fallback "out of space" path.
    _OUT_OF_SPACE_SUFFIX = b": No space left on device\n"

    def _send_to_loopback_if_fits(
        self, output_path, fs_size_bytes, subvol_opts: SubvolOpts
    ) -> (int, int):
        """
        Creates a loopback of the specified size, and sends the current
        subvolume to it. Returns a tuple of two values. The first is the number
        of bytes which didn't fit in that space. It is zero if the subvolume
        fits. The second value is the image size in the end of operation.
        """
        open(output_path, "wb").close()
        with pipe() as (r_send, w_send), Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as ns, LoopbackVolume(
            ns, output_path, fs_size_bytes
        ) as loop_vol, self.mark_readonly_and_write_sendstream_to_file(
            w_send
        ):
            w_send.close()  # This end is now fully owned by `btrfs send`.
            with r_send:
                recv_ret = run_stdout_to_err(
                    nsenter_as_root(ns, "btrfs", "receive", loop_vol.dir()),
                    stdin=r_send,
                    stderr=subprocess.PIPE,
                )
                if recv_ret.returncode != 0:
                    if recv_ret.stderr.endswith(self._OUT_OF_SPACE_SUFFIX):
                        log.info("Will retry receive, did not fit")
                        return (
                            _drain_pipe_return_byte_count(r_send),
                            loop_vol.get_size(),
                        )
                    log.info("Receive failed: {}")
                    # It's pretty lame to rely on `btrfs receive` continuing
                    # to be unlocalized, and emitting that particular error
                    # message, so we fall back to checking available bytes.
                    size_ret = subprocess.run(
                        nsenter_as_user(
                            ns,
                            "findmnt",
                            "--noheadings",
                            "--bytes",
                            "--output",
                            "AVAIL",
                            loop_vol.dir(),
                        ),
                        stdout=subprocess.PIPE,
                    )
                    # If the `findmnt` fails, don't mask the original error.
                    # If `btrfs receive` fails with "No space left on device",
                    # a few KBs (up to 32 in my experiments) of free space may
                    # remain on destination file-system.
                    if (
                        size_ret.returncode == 0
                        and int(size_ret.stdout) <= 32 * KiB
                    ):
                        log.info(
                            "Will retry receive, volume "
                            f"AVAIL={int(size_ret.stdout)}"
                        )
                        return (
                            _drain_pipe_return_byte_count(r_send),
                            loop_vol.get_size(),
                        )
                    # Covering this is hard, so the test plan is "inspection".
                    log.error(  # pragma: no cover
                        "Unhandled receive stderr:\n\n"
                        + recv_ret.stderr.decode(errors="surrogateescape")
                    )
                recv_ret.check_returncode()
                if not subvol_opts.readonly:
                    sub_name = os.path.basename(self.path())
                    subvol_path = os.path.join(loop_vol.dir(), sub_name)
                    run_stdout_to_err(
                        nsenter_as_root(
                            ns,
                            "btrfs",
                            "property",
                            "set",
                            "-ts",
                            subvol_path,
                            "ro",
                            "false",
                        )
                    ).check_returncode()
            return (0, loop_vol.minimize_size())

    def _send_to_loopback_second_pass(
        self, output_path, initial_size_bytes, subvol_opts: SubvolOpts
    ) -> (int, int):
        size_bytes_to_try = 512 * os.stat(output_path).st_blocks
        attempts = 0
        last_effort = False
        while True:
            attempts += 1
            size_bytes_to_try *= 1.1
            if size_bytes_to_try >= initial_size_bytes:
                # If we got here we could just use the output of the first pass.
                # This is a possible future disk vs time optimization.
                size_bytes_to_try = initial_size_bytes
                last_effort = True
            leftover_bytes, new_size = self._send_to_loopback_if_fits(
                output_path, int(size_bytes_to_try), subvol_opts
            )
            if leftover_bytes != 0:
                log.warning(
                    f"{self._path} did not fit in {size_bytes_to_try} bytes, "
                    f"{leftover_bytes} sendstream bytes were left over, "
                    f"attempts {attempts}"
                )
            if leftover_bytes == 0 or last_effort:
                return (leftover_bytes, new_size)
            assert (
                attempts <= 10
            ), f"{attempts} attempts were not enough for {self._path}"

    @contextmanager
    def receive(self, from_file):
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
                    self._exists = True


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
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            return method(self, temp_subvols, *args, **kwargs)

    return decorated


# NB: Memoizing this function would be pretty reasonable.
def volume_dir(path_in_repo=None) -> Path:
    return find_artifacts_dir(path_in_repo) / "volume"


class TempSubvolumes(contextlib.AbstractContextManager):
    """
    Tracks the subvolumes it creates, and destroys them on context exit.

    Note that relying on unittest.TestCase.addCleanup to __exit__ this
    context is unreliable -- e.g. clean-up is NOT triggered on
    KeyboardInterrupt. Therefore, this **will** leak subvolumes
    during development. You can clean them up thus:

        sudo btrfs sub del buck-image-out/volume/tmp/TempSubvolumes_*/subvol &&
            rmdir buck-image-out/volume/tmp/TempSubvolumes_*

    Instead of polluting `buck-image-out/volume`, it  would be possible to
    put these on a separate `LoopbackVolume`, to rely on `Unshare` to
    guarantee unmounting it, and to rely on `tmpwatch` to delete the stale
    loopbacks from `/tmp/`.  At present, this doesn't seem worthwhile since
    it would require using an `Unshare` object throughout `Subvol`.

    The easier approach is to write `with TempSubvolumes() ...` in each test.
    """

    def __init__(self, path_in_repo=None):
        self.subvols = []
        # The 'tmp' subdirectory simplifies cleanup of leaked temp subvolumes
        volume_tmp_dir = volume_dir(path_in_repo) / "tmp"
        try:
            os.mkdir(volume_tmp_dir)
        except FileExistsError:
            pass
        # Our exit is written with exception-safety in mind, so this
        # `_temp_dir_ctx` **should** get `__exit__`ed when this class does.
        self._temp_dir_ctx = temp_dir(  # noqa: P201
            dir=volume_tmp_dir.decode(), prefix=self.__class__.__name__ + "_"
        )

    def __enter__(self):
        self._temp_dir = self._temp_dir_ctx.__enter__()
        return self

    def _prep_rel_path(self, rel_path: AnyStr) -> Path:
        """
        Ensures subvolumes live under our temporary directory, which
        improves safety, since its permissions ought to be u+rwx to avoid
        exposing setuid binaries inside the built subvolumes.
        """
        rel_path = (
            (self._temp_dir / rel_path)
            .realpath()
            .relpath(self._temp_dir.realpath())
        )
        if rel_path.has_leading_dot_dot():
            raise AssertionError(
                f"{rel_path} must be a subdirectory of {self._temp_dir}"
            )
        abs_path = self._temp_dir / rel_path
        try:
            os.makedirs(abs_path.dirname())
        except FileExistsError:
            pass
        return abs_path

    def create(self, rel_path: AnyStr) -> Subvol:
        subvol = Subvol(self._prep_rel_path(rel_path))
        subvol.create()
        self.subvols.append(subvol)
        return subvol

    def snapshot(self, source: Subvol, dest_rel_path: AnyStr) -> Subvol:
        dest = Subvol(self._prep_rel_path(dest_rel_path))
        dest.snapshot(source)
        self.subvols.append(dest)
        return dest

    def caller_will_create(self, rel_path: AnyStr) -> Subvol:
        subvol = Subvol(self._prep_rel_path(rel_path))
        # If the caller fails to create it, our __exit__ is robust enough
        # to ignore this subvolume.
        self.subvols.append(subvol)
        return subvol

    def external_command_will_create(self, rel_path: AnyStr) -> Subvol:
        subvol = self.caller_will_create(rel_path)
        subvol._exists = True
        return subvol

    def __exit__(self, exc_type, exc_val, exc_tb):
        # If any of subvolumes are nested, and the parents were made
        # read-only, we won't be able to delete them.
        for subvol in self.subvols:
            try:
                subvol.set_readonly(False)
            except BaseException:  # Ctrl-C does not interrupt cleanup
                pass
        for subvol in reversed(self.subvols):
            try:
                subvol._delete_inner_subvols()
                subvol.delete()
            except BaseException:  # Ctrl-C does not interrupt cleanup
                logging.exception(f"Deleting volume {subvol.path()} failed.")
        return self._temp_dir_ctx.__exit__(exc_type, exc_val, exc_tb)
