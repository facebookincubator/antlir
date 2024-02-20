#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Utilities to make Python systems programming more palatable."
import argparse
import errno
import importlib.resources
import os
import shlex
import shutil
import subprocess
import tempfile
from contextlib import contextmanager
from typing import AnyStr, Generator, IO, Iterable, Iterator, List, Union

from antlir.common import get_logger


log = get_logger()


# We need this for lists that can contain a combination of `str` and `bytes`,
# which is very common with `subprocess`. See https://fburl.com/wiki/dqrqyd8r.
MehStr = Union[str, bytes, "Path"]


# Bite me, Python3.
def byteme(s: AnyStr) -> bytes:
    "Byte literals are tiring, just promote strings as needed."
    # pyre-fixme[16]: `bytes` has no attribute `encode`.
    return s.encode() if isinstance(s, str) else s


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
      - `Optional[Path]`: `or_none`
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

    def touch(self) -> "Path":
        with self.open(mode="a"):
            pass
        return self

    def unlink(self) -> None:
        return os.unlink(self)

    def __format__(self, spec: str) -> str:
        "Allow usage of `Path` in f-strings."
        return self.decode(errors="surrogateescape").__format__(spec)

    def __str__(self) -> str:
        'Matches `__format__` since people expect `f"{p}" == str(p)`.'
        return self.decode(errors="surrogateescape")


@contextmanager
def temp_dir(**kwargs) -> Generator[Path, None, None]:
    with tempfile.TemporaryDirectory(**kwargs) as td:
        yield Path(td)


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
