#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Utilities to make Python systems programming more palatable."
import argparse
import base64
import ctypes
import errno
import importlib.resources
import json
import os
import shlex
import shutil
import signal
import stat
import subprocess
import tempfile
import time
import urllib.parse
import uuid
from contextlib import contextmanager
from typing import Any, AnyStr, Generator, IO, Iterable, Iterator, List, Union

from antlir.common import byteme, check_popen_returncode, get_logger


log = get_logger()


# We need this for lists that can contain a combination of `str` and `bytes`,
# which is very common with `subprocess`. See https://fburl.com/wiki/dqrqyd8r.
MehStr = Union[str, bytes, "Path"]


class _OpenHow(ctypes.Structure):
    _fields_ = [
        ("flags", ctypes.c_uint64),
        ("mode", ctypes.c_uint64),
        ("resolve", ctypes.c_uint64),
    ]


# `pathlib` refuses to operate on `bytes`, which is the only sane way on Linux.
class Path(bytes):
    """
    A `bytes` path that supports joining via the / operator.

    `Path` can mostly be used in place of `bytes`, with a few key differences:
      - It is an error to compare it to `str`, preventing a common bug.
      - It formats as a surrogate-escaped string, not as a quoted
        byte-string.  If you need the latter, use `repr()`.

    Most operations (including construction, and `/`) accept `str` and
    `bytes`.

    We always interconvert with `str` as if the ambient encoding is `utf-8`.
    No provisions are made for other encodings, but if a use-case arose,
    this could be improved.

    `Path` exposes many helper methods borrowed from `os` and `os.path`, and
    a some `pathlib`-style methods (`read_text`, `touch`).

    Additionally, it has integration with these commonly used tools:
      - `argparse`: `from_argparse` and `parse_args`
      - `json`: `json_load` and `json_loads`
      - `Optional[Path]`: `or_none`
      - `urllib`: `file_url`
    """

    def __new__(cls, arg, *args, **kwargs):
        return super().__new__(cls, byteme(arg), *args, **kwargs)

    @classmethod
    def __get_validators__(cls):
        # force Pydantic to deserialize a string to an actual Path, instead of
        # str/bytes
        yield cls._validate

    @classmethod
    def _validate(cls, v) -> "Path":
        return cls(v)

    def __eq__(self, obj) -> bool:
        if not isinstance(obj, (bytes, type(None))):
            # NB: The verbose error can be expensive, but this error must
            # never occur in correct code, so optimize for debuggability.
            raise TypeError(
                f"Cannot compare `Path` {repr(self)} with "
                f"`{type(obj)}` {repr(obj)}."
            )
        return super().__eq__(obj)

    def __ne__(self, obj) -> bool:
        return not self.__eq__(obj)

    def __hash__(self) -> int:
        return super().__hash__()

    @classmethod
    def or_none(cls, arg, *args, **kwargs):
        if arg is None:
            return None
        return cls(arg, *args, **kwargs)

    @classmethod
    # pyre-fixme[14]: `join` overrides method defined in `bytes` inconsistently.
    def join(cls, *paths) -> "Path":
        if not paths:
            # pyre-fixme[7]: Expected `Path` but got `None`.
            return None
        return Path(os.path.join(byteme(paths[0]), *(byteme(p) for p in paths[1:])))

    def __truediv__(self, right: AnyStr) -> "Path":
        return Path(os.path.join(self, byteme(right)))

    def __rtruediv__(self, left: AnyStr) -> "Path":
        return Path(os.path.join(byteme(left), self))

    def exists(self, raise_permission_error: bool = False) -> bool:
        if not raise_permission_error:
            return os.path.exists(self)
        try:
            os.stat(self)
            return True
        except FileNotFoundError:
            return False

    def wait_for(self, timeout_ms: int = 5000) -> int:
        start_ms = int(time.monotonic() * 1000)
        elapsed_ms = 0
        while elapsed_ms < timeout_ms:
            if self.exists(raise_permission_error=False):
                return elapsed_ms
            time.sleep(0.1)
            elapsed_ms = int(time.monotonic() * 1000) - start_ms

        raise FileNotFoundError(self)

    def file_url(self) -> str:
        return "file://" + urllib.parse.quote(self.abspath())

    def abspath(self) -> "Path":
        return Path(os.path.abspath(self))

    def basename(self) -> "Path":
        return Path(os.path.basename(self))

    def dirname(self) -> "Path":
        return Path(os.path.dirname(self))

    def islink(self) -> bool:
        return os.path.islink(self)

    # NB: A lazy `gen_dir_names()` was briefly considered, but rejected (for
    # now) because:
    #   (1) `listdir` is clearly analogous to the standard `os` module
    #   (2) `gen_dir_paths` has just 1 use-case in `test_parse_repodata.py`
    #   (3) `listdir` is shorter, and the cost of a spurious list is low
    def listdir(self) -> List["Path"]:
        """
        Prefer over `os.listdir` for conciseness, and because downstream
        code might want a `Path` (for example, to use in f-strings).
        """
        return [Path(p) for p in os.listdir(self)]

    def normpath(self) -> "Path":
        return Path(os.path.normpath(self))

    def realpath(self) -> "Path":
        return Path(os.path.realpath(self))

    def readlink(self) -> "Path":
        return Path(os.readlink(self))

    # `start` does NOT default to the current directory because code relying
    # on $PWD tends to be fragile, and we don't want to make it implicit.
    def relpath(self, start: AnyStr) -> "Path":
        return Path(os.path.relpath(self, byteme(start)))

    def _resolve_altroot_path(self, path: "Path") -> "Path":
        """
        Resolve a path relative to an alternate root. Useful when said path
        may contain symlinks that point to absolute paths.
        """

        # Normalize altroot path.
        altroot = self.realpath()

        # Constants from headers
        __NR_openat2 = 437
        __RESOLVE_IN_ROOT = 0x10

        #
        # Define openat2(2) syscall wrapper. Note, glibc does not provide
        # a wrapper for openat2() so we must use of syscall(2). The
        # function signature is:
        #
        #     long syscall(SYS_openat2, int dirfd, const char *pathname,
        #         struct open_how *how, size_t size);
        #
        _openat2 = ctypes.CDLL(None).syscall
        _openat2.restype = ctypes.c_long
        _openat2.argtypes = (
            ctypes.c_long,
            ctypes.c_uint,
            ctypes.c_char_p,
            ctypes.POINTER(_OpenHow),
            ctypes.c_size_t,
        )
        altroot_fd = os.open(altroot, os.O_RDONLY)
        open_how = _OpenHow(flags=0, mode=0, resolve=__RESOLVE_IN_ROOT)
        fd = _openat2(__NR_openat2, altroot_fd, path, open_how, ctypes.sizeof(open_how))
        errno = ctypes.get_errno()
        os.close(altroot_fd)
        if fd == -1:
            # It's possible this is a non-existent path, in which case return
            # it as is.
            log.debug(
                f"Failed to resolve path '{path}'"
                + f"within altroot '{self}' (errno: {errno})"
            )
            return self / path.lstrip(b"/")
        resolved_path = Path(os.readlink(f"/proc/self/fd/{fd}"))
        os.close(fd)
        assert resolved_path.startswith(altroot)
        return self / resolved_path[len(altroot) :].lstrip(b"/")

    def normalized_subpath(
        self,
        path: AnyStr,
        *,
        no_dereference_leaf: bool = False,
        resolve_links: bool = False,
    ) -> "Path":
        """
        Returns a normalized path to `path` interpreted as a child of the
        directory `self`, raising if the actual path points outside `self`.
        We check for two risks:
          - `path` is relative, and goes above `self` via '..'.
          - Some component of the path is a symlink, and this symlink, when
            interpreted by a non-chrooted tool, will attempt to access
            something outside of `self`.
        If `path` is absolute, the leading `/` is ignored.

        The above check fail on attempting to traverse an symlink within
        `self` that is an absolute path to another directory within the `self`
        -- i.e.  if you think of `self` as the root of another filesystem,
        absolute symlinks won't work.

        Such absolute symlinks are not supported by default because at
        present, I believe that the right idiom is to encourage image authors
        to manipulate the "real" locations of files, and not to manipulate
        paths through symlinks.

        In certain cases, we do want to resolve links relative to to 'self'
        (treated as an alternative root). This behavior can be enabled via the
        `resolve_links` option. If the link path can be resolved (within the
        context of the alternate root), the fully resolved (not normalized)
        path is returned. (This is done so that other callers can use the
        returned path without having to jump through special altroot path
        resolution hoops.) If the link path can't be resolved, it is returned
        as is.

        In the rare case that you need to manipulate a symlink itself (e.g.
        remove or rename), you will want to pass `no_dereference_leaf`.

        Future: consider using a file descriptor to refer to the base
        directory to better mitigate races due to renames in its path.
        """

        # Can't have both no_dereference_leaf and resolve_links
        assert not no_dereference_leaf or not resolve_links, (
            "Error: no_dereference_leaf and resolve_links are incompatible."
            + " The former disables link resolution, while the latter"
            + " attempts to enable it."
        )

        if resolve_links:
            return self._resolve_altroot_path(Path(path))

        # Without the lstrip, we would lose the `self` prefix if the
        # supplied path is absolute.
        result_path = (self / (Path(path).lstrip(b"/"))).normpath()

        # Paranoia: Make sure that, despite any symlinks in the path, the
        # resulting path is not outside of `self`.
        if (
            (
                (result_path.dirname().realpath() / result_path.basename())
                if no_dereference_leaf
                else result_path.realpath()
            )
            .relpath(self.realpath())
            .has_leading_dot_dot()
        ):
            raise AssertionError(f"{path} is outside of {self}")
        return Path(result_path)

    # Returns `str` because shell scripts are normally strings, not bytes.
    # Also, shlex.quote doesn't support bytes (see Python Issue #25567).
    def shell_quote(self) -> str:
        return shlex.quote(self.decode())

    # pyre-fixme[9]: errors has type `str`; used as `None`.
    def decode(self, encoding: str = "utf-8", errors: str = None) -> str:
        # Python uses `surrogateescape` for invalid UTF-8 from the filesystem.
        if errors is None:
            errors = "surrogateescape"
        # Future: if there's a legitimate reason to allow other `errors`,
        # this can be fixed -- just make `surrogatescape` a normal default.
        assert errors == "surrogateescape", errors
        return super().decode(encoding, errors)

    @classmethod
    def from_argparse(cls, s: str) -> "Path":
        # Python uses `surrogateescape` for `sys.argv`.
        return Path(s.encode(errors="surrogateescape"))

    @classmethod
    def parse_args(
        cls, parser: argparse.ArgumentParser, argv: Iterable[MehStr]
    ) -> argparse.Namespace:
        """
        Use this instead of `ArgumentParser.parse_args` because,
        inconveniently, it does not accept `bytes`, which makes it harder to
        write tests that use `Path` by default.
        """
        return parser.parse_args(
            [
                a.decode(errors="surrogateescape") if isinstance(a, bytes) else a
                for a in argv
            ]
        )

    def read_text(self) -> str:
        with self.open() as infile:
            return infile.read()

    @contextmanager
    def open(self, mode: str = "r") -> IO:
        with open(self, mode=mode) as f:
            # pyre-fixme[7]: Expected `IO[typing.Any]` but got
            #  `Generator[io.TextIOWrapper, None, None]`.
            yield f

    @classmethod
    @contextmanager
    def resource(cls, package, name: str, *, exe: bool) -> Iterator["Path"]:
        """
        An improved `importlib.resources.path`. The main differences:
          - Returns an `fs_utils.Path` instead of a `pathlib` object.
          - Unlike `importlib`, the resulting path can be executed if
            `exe=True`.  This argument should the actual mode of the
            resource, but unfortunately, `importlib` does not give us a way
            to inspect the pre-existing mode in all cases, and we don't want
            to hardcode details of FB-internal packaging formats here.

        CAUTION: The yielded path may be temporary, to be deleted upon
        exiting the context.

        This is intended to work with all supported FB-internal and
        open-source Python package formats: "zip", "fastzip", "pex", "xar".
        """
        with importlib.resources.open_binary(package, name) as rsrc_in:
            # Future: once the bug with the XAR `access` implementation
            # is fixed (https://fburl.com/42s41c0g), this can just check
            # for boolean equality.
            if (
                hasattr(rsrc_in, "name")
                and os.path.exists(rsrc_in.name)
                and (not exe or (exe and os.access(rsrc_in.name, os.X_OK)))
            ):
                yield Path(rsrc_in.name).abspath()
                return

            # The resource has no path, so we have to materialize it.
            #
            # Why does this happen? Who knows - but we can make a copy of the
            # binary that _is_ executable and antlir1 limps on another day.
            #
            # This code path is not reached by our coverage harness,
            # since resources in '@mode/dev will always have a real
            # filesystem path.  However, we get all the needed signal
            # from running `test-fs-utils-path-resource-*' in
            # `@mode/dev` and `@mode/opt'.
            #
            # Wrap in a temporary directory so we can `chmod 755` below.
            with temp_dir() as td:  # pragma: no cover
                with open(td / name, "wb") as rsrc_out:
                    # We can't use `os.sendfile` because `rsrc_in` may
                    # not be backed by a real FD.
                    while True:
                        # Read 512KiB chunks to mask the syscall cost
                        chunk = rsrc_in.read(2**19)
                        if not chunk:
                            break
                        rsrc_out.write(chunk)
                if exe:
                    # The temporary directory protects us from undesired
                    # access.  The goal is to let both the current user
                    # and `root` execute this path, but nobody else.
                    os.chmod(td / name, 0o755)
                yield td / name

    # Future: Consider if we actually want something like
    # `relative_outside_base`, which is `.normpath().has_leading_dot_dot()`.
    def has_leading_dot_dot(self) -> bool:
        return self == b".." or self.startswith(b"../")

    def strip_leading_slashes(self) -> "Path":
        return Path(self.lstrip(b"/"))

    def touch(self) -> "Path":
        with self.open(mode="a"):
            pass
        return self

    def unlink(self) -> None:
        return os.unlink(self)

    @classmethod
    def json_dumps(cls, *args, **kwargs) -> str:
        "Use instead of `json.dumps` to serializing `Path` values."
        assert "cls" not in kwargs
        return json.dumps(*args, **kwargs, cls=_PathJSONEncoder)

    @classmethod
    def json_dump(cls, *args, **kwargs) -> str:
        "Use instead of `json.dump` to allow serializing `Path` values."
        assert "cls" not in kwargs
        # pyre-fixme[7]: Expected `str` but got `None`.
        return json.dump(*args, **kwargs, cls=_PathJSONEncoder)

    def __format__(self, spec: str) -> str:
        "Allow usage of `Path` in f-strings."
        return self.decode(errors="surrogateescape").__format__(spec)

    def __str__(self) -> str:
        'Matches `__format__` since people expect `f"{p}" == str(p)`.'
        return self.decode(errors="surrogateescape")


# This path is off-limits to regular image operations, it exists only to
# record image metadata and configuration.  This is at the root, instead of
# in `etc` because that means that `FilesystemRoot` does not have to provide
# `etc` and determine its permissions.  In other words, a top-level ".meta"
# directory makes the compiler less opinionated about other image content.
#
# NB: The trailing slash is significant, making this a protected directory,
# not a protected file.
META_DIR = Path(".meta/")

# Keep in sync with `snapshot_install_dir.bzl`
RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR = Path(
    "/__antlir__/rpm/default-snapshot-for-installer/"
)

# Contains data needed to run the package manager in images.
ANTLIR_DIR = Path("/__antlir__")

# The name of the flavor file in the meta directory
META_FLAVOR_FILE: Path = META_DIR / "flavor"

# The name of the build directory in the meta directory
META_BUILD_DIR: Path = META_DIR / "build"


# Future: If it becomes necessary to serialize dict keys that are `Path`,
# the `json` module currently does not support custom key serialization.  In
# that case, we would delete this and replace it with an explicit recursive
# traversal on the input that returns a new structure.  Yay, `json`.
class _PathJSONEncoder(json.JSONEncoder):
    "Implementation detail for `Path.json_dump` & `Path.json_dumps`."

    # pyre-fixme[14]: `default` overrides method defined in `JSONEncoder`
    #  inconsistently.
    def default(self, obj: Path) -> str:
        if isinstance(obj, Path):
            return obj.decode(errors="surrogateescape")
        return super().default(self, obj)


@contextmanager
def temp_dir(**kwargs) -> Generator[Path, None, None]:
    with tempfile.TemporaryDirectory(**kwargs) as td:
        yield Path(td)


def generate_work_dir() -> Path:
    return Path(
        b"/work"
        + base64.urlsafe_b64encode(
            uuid.uuid4().bytes  # base64 instead of hex saves 10 bytes
        ).strip(b"=")
    )


@contextmanager
def open_for_read_decompress(
    path: Path, zstd_threads: int = 0
) -> Generator[Any, Any, Any]:
    'Wraps `open(path, "rb")` to add transparent `.zst` or `.gz` decompression.'
    path = Path(path)
    if path.endswith(b".zst"):
        decompress_cmd = ["zstd", f"--threads={zstd_threads}"]
    elif path.endswith(b".gz") or path.endswith(b".tgz"):
        decompress_cmd = ["gzip"]
    else:
        with path.open(mode="rb") as f:
            yield f
        return
    with subprocess.Popen(
        decompress_cmd + ["--decompress", "--stdout", path],
        stdout=subprocess.PIPE,
    ) as proc:
        yield proc.stdout
    # If the caller does not consume all of `proc.stdout`, the decompressor
    # program can get a SIGPIPE as it tries to write to a closed pipe.
    #
    # Rationale for just ignoring the signal -- I considered adding a
    # mandatory `must_read_all_data` boolean arg , but decided it against it:
    #   - Adding this in the no-compression case feels artificial.
    #   - This is not typical behavior for Python file context managers -- it
    #     should likely be provided as a wrapper, not as part of the API --
    #     if it's even desirable.
    #   - The extra API complexity is of dubious utility.
    if proc.returncode == -signal.SIGPIPE:
        log.error(f"Ignoring SIGPIPE exit of child `{decompress_cmd[0]}` for `{path}`")
    else:
        check_popen_returncode(proc)


def create_ro(path, mode):
    "`open` that creates (and never overwrites) a file with mode `a+r`."

    def ro_opener(path, flags):
        return os.open(
            path,
            (flags & ~os.O_TRUNC) | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC,
            mode=stat.S_IRUSR | stat.S_IRGRP | stat.S_IROTH,
        )

    return open(path, mode, opener=ro_opener)


@contextmanager
def populate_temp_dir_and_rename(
    dest_path, *, overwrite: bool = False
) -> Iterator[Path]:
    """
    Returns a Path to a temporary directory. The context block may populate
    this directory, which will then be renamed to `dest_path`, optionally
    deleting any preexisting directory (if `overwrite=True`).

    If the context block throws, the partially populated temporary directory
    is removed, while `dest_path` is left alone.

    By writing to a brand-new temporary directory before renaming, we avoid
    the problems of partially writing files, or overwriting some files but
    not others.  Moreover, populate-temporary-and-rename is robust to
    concurrent writers, and tends to work on broken NFSes unlike `flock`.
    """
    dest_path = os.path.normpath(dest_path)  # Trailing / breaks `os.rename()`
    # Putting the temporary directory as a sibling minimizes permissions
    # issues, and maximizes the chance that we're on the same filesystem
    base_dir = os.path.dirname(dest_path)
    td = tempfile.mkdtemp(dir=base_dir)
    try:
        yield Path(td)

        # Delete+rename is racy, but EdenFS lacks RENAME_EXCHANGE (t34057927)
        # Retry if we raced with another writer -- i.e., last-to-run wins.
        while True:
            if overwrite and os.path.isdir(dest_path):
                with tempfile.TemporaryDirectory(dir=base_dir) as del_dir:
                    try:
                        os.rename(dest_path, del_dir)
                    except FileNotFoundError:  # pragma: no cover
                        continue  # retry, another writer deleted first?
            try:
                os.rename(td, dest_path)
            except OSError as ex:
                if not (
                    overwrite
                    and ex.errno
                    in [
                        # Different kernels have different error codes when the
                        # target already exists and is a nonempty directory.
                        errno.ENOTEMPTY,
                        errno.EEXIST,
                    ]
                ):
                    raise
                log.exception(  # pragma: no cover
                    f"Retrying deleting {dest_path}, another writer raced us"
                )
            # We won the race
            break  # pragma: no cover
    except BaseException:
        shutil.rmtree(td)
        raise


@contextmanager
def populate_temp_file_and_rename(
    dest_path: Path, *, overwrite: bool = False, mode: str = "w"
):
    """
    Opens a temporary file for writing in the same directory as the desired
    file `dest_path`. Yields a `file`-like object to be populated inside
    the context.

    On successfully exiting the with-block, one of two things will happen:

    1. Default: If `overwrite` is not set, then the temporary file will
       attempt to be renamed to the `dest_path`. If `dest_path` already
       exists (determined through a race-prone `os.path.exists` check),
       the temporary file will be removed and an `FileExistsError` will
       be raised. If `dest_path` does not exist, the renaming will be an
       atomic operation (this is a POSIX requirement).
    2. If `overwrite` is set, then the temporary file will replace any
       file existing at `dest_path` and all of its content.

    If the with-block is exited unsuccessfully, the temporary file
    will be deleted. If the dest_path specifies a directory, it will
    fail to replace the directory. This behaviour is regardless of
    the `overwrite` value provided and is subject to change (should not
    be relied on).
    """
    with tempfile.NamedTemporaryFile(
        mode=mode, dir=dest_path.dirname(), delete=False
    ) as tf:
        try:
            yield tf
            if not overwrite and os.path.exists(dest_path):
                # Race prone to check to determine if file exists.
                # If eliminating the possibility of a race is important
                # look into using `renameat2` via `ctypes`
                raise FileExistsError
            os.rename(tf.name, dest_path)
        except BaseException:  # Clean up even on Ctrl-C
            os.unlink(tf.name)
            raise


# This list contains the arguments needed to make a btrfs reflink from a
# different file.
#
# Option rationales:
#   - The compiler should have detected any collisons on the destination, so
#     `--no-clobber` is just a failsafe.
#   - `--no-dereference` is needed since our contract is to copy each
#     symlink's destination text verbatim.  Not doing this would also risk
#     following absolute symlinks, reaching OUTSIDE of the source subvolume!
#   - `--reflink=always` aids efficiency and, more importantly, preserves
#     "cloned extent" relationships that existed within the source subtree.
#   - `--sparse=auto` is implied by `--reflink=always`.  The two together
#     ought to preserve the original sparseness layout,
#   - `--preserve=all` keeps as much original metadata as possible,
#     including hardlinks.
CP_CLONE_CMD = [
    "cp",
    "--recursive",
    "--no-clobber",
    "--no-dereference",
    "--reflink=always",
    "--sparse=auto",
    "--preserve=all",
]
