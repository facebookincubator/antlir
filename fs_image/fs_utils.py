#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'Utilities to make Python systems programming more palatable.'
import argparse
import errno
import importlib.resources
import json
import os
import shlex
import shutil
import stat
import subprocess
import urllib.parse
import tempfile

from contextlib import contextmanager
from typing import AnyStr, Generator, Iterable, Iterator, List, Union

from .common import byteme, check_popen_returncode, get_file_logger

log = get_file_logger(__file__)

# We need this for lists that can contain a combination of `str` and `bytes`,
# which is very common with `subprocess`. See https://fburl.com/wiki/dqrqyd8r.
MehStr = Union[str, bytes]


# `pathlib` refuses to operate on `bytes`, which is the only sane way on Linux.
class Path(bytes):
    '''
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
    '''

    def __new__(cls, arg, *args, **kwargs):
        return super().__new__(cls, byteme(arg), *args, **kwargs)

    def __eq__(self, obj) -> bool:
        if not isinstance(obj, bytes):
            # NB: The verbose error can be expensive, but this error must
            # never occur in correct code, so optimize for debuggability.
            raise TypeError(
                f'Cannot compare `Path` {repr(self)} with '
                f'`{type(obj)}` {repr(obj)}.'
            )
        return super().__eq__(obj)

    def __ne__(self, obj) -> bool:
        return not self.__eq__(obj)

    __hash__ = bytes.__hash__

    @classmethod
    def or_none(cls, arg, *args, **kwargs):
        if arg is None:
            return None
        return cls(arg, *args, **kwargs)

    def __truediv__(self, right: AnyStr) -> 'Path':
        return Path(os.path.join(self, byteme(right)))

    def __rtruediv__(self, left: AnyStr) -> 'Path':
        return Path(os.path.join(byteme(left), self))

    def file_url(self) -> str:
        return 'file://' + urllib.parse.quote(os.path.abspath(self))

    def basename(self) -> 'Path':
        return Path(os.path.basename(self))

    def dirname(self) -> 'Path':
        return Path(os.path.dirname(self))

    # NB: A lazy `gen_dir_names()` was briefly considered, but rejected (for
    # now) because:
    #   (1) `listdir` is clearly analogous to the standard `os` module
    #   (2) `gen_dir_paths` has just 1 use-case in `test_parse_repodata.py`
    #   (3) `listdir` is shorter, and the cost of a spurious list is low
    def listdir(self) -> List['Path']:
        '''
        Prefer over `os.listdir` for conciseness, and because downstream
        code might want a `Path` (for example, to use in f-strings).
        '''
        return [Path(p) for p in os.listdir(self)]

    def normpath(self) -> 'Path':
        return Path(os.path.normpath(self))

    # Returns `str` because shell scripts are normally strings, not bytes.
    def shell_quote(self) -> str:
        return shlex.quote(self.decode())

    def decode(self, encoding='utf-8', errors=None) -> str:
        # Python uses `surrogateescape` for invalid UTF-8 from the filesystem.
        if errors is None:
            errors = 'surrogateescape'
        # Future: if there's a legitimate reason to allow other `errors`,
        # this can be fixed -- just make `surrogatescape` a normal default.
        assert errors == 'surrogateescape', errors
        return super().decode(encoding, errors)

    @classmethod
    def from_argparse(cls, s: str) -> 'Path':
        # Python uses `surrogateescape` for `sys.argv`.
        return Path(s.encode(errors='surrogateescape'))

    @classmethod
    def parse_args(
        cls, parser: argparse.ArgumentParser, argv: Iterable[Union[str, 'Path']]
    ) -> argparse.Namespace:
        '''
        Use this instead of `ArgumentParser.parse_args` because,
        inconveniently, it does not accept `bytes`, which makes it harder to
        write tests that use `Path` by default.
        '''
        return parser.parse_args([
            a.decode(errors='surrogateescape') if isinstance(a, bytes) else a
                for a in argv
        ])

    def read_text(self) -> str:
        with open(self) as infile:
            return infile.read()

    @classmethod
    @contextmanager
    def resource(cls, package, name: str, *, exe: bool) -> Iterator['Path']:
        '''
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
        '''
        with importlib.resources.open_binary(package, name) as rsrc_in:
            # This check is clowny, but `importlib` doesn't provide a clean
            # way to ask if the resource already exists on disk.
            if hasattr(rsrc_in, 'name'):
                # Future: once the bug with the XAR `access` implementation
                # is fixed (https://fburl.com/42s41c0g), this can just check
                # for boolean equality.
                if exe and not os.access(rsrc_in.name, os.X_OK):
                    raise RuntimeError(  # pragma: no cover
                        f'{package}.{name} is not executable'
                    )
                yield Path(os.path.abspath(rsrc_in.name))
            else:  # pragma: no cover
                # The resource has no path, so we have to materialize it.
                #
                # This code path is not reached by our coverage harness,
                # since resources in '@mode/dev will always have a real
                # filesystem path.  However, we get all the needed signal
                # from running `test-fs-utils-path-resource-*' in
                # `@mode/dev` and `@mode/opt'.
                #
                # Wrap in a temporary directory so we can `chmod 755` below.
                with temp_dir() as td:
                    with open(td / name, 'wb') as rsrc_out:
                        # We can't use `os.sendfile` because `rsrc_in` may
                        # not be backed by a real FD.
                        while True:
                            # Read 512KiB chunks to mask the syscall cost
                            chunk = rsrc_in.read(2 ** 19)
                            if not chunk:
                                break
                            rsrc_out.write(chunk)
                    if exe:
                        # The temporary directory protects us from undesired
                        # access.  The goal is to let both the current user
                        # and `root` execute this path, but nobody else.
                        os.chmod(td / name, 0o755)
                    yield td / name

    def touch(self) -> 'Path':
        with open(self, 'a'):
            pass
        return self

    @classmethod
    def json_dumps(cls, *args, **kwargs) -> str:
        'Use instead of `json.dumps` to serializing `Path` values.'
        assert 'cls' not in kwargs
        return json.dumps(*args, **kwargs, cls=_PathJSONEncoder)

    @classmethod
    def json_dump(cls, *args, **kwargs) -> str:
        'Use instead of `json.dump` to allow serializing `Path` values.'
        assert 'cls' not in kwargs
        return json.dump(*args, **kwargs, cls=_PathJSONEncoder)

    def __format__(self, spec) -> str:
        'Allow usage of `Path` in f-strings.'
        return self.decode(errors='surrogateescape').__format__(spec)

    def __str__(self) -> str:
        'Matches `__format__` since people expect `f"{p}" == str(p)`.'
        return self.decode(errors='surrogateescape')


# Future: If it becomes necessary to serialize dict keys that are `Path`,
# the `json` module currently does not support custom key serialization.  In
# that case, we would delete this and replace it with an explicit recursive
# traversal on the input that returns a new structure.  Yay, `json`.
class _PathJSONEncoder(json.JSONEncoder):
    'Implementation detail for `Path.json_dump` & `Path.json_dumps`.'
    def default(self, obj):
        if isinstance(obj, Path):
            return obj.decode(errors='surrogateescape')
        return super().default(self, obj)


@contextmanager
def temp_dir(**kwargs) -> Generator[Path, None, None]:
    with tempfile.TemporaryDirectory(**kwargs) as td:
        yield Path(td)


@contextmanager
def open_for_read_decompress(path):
    'Wraps `open(path, "rb")` to add transparent `.zst` or `.gz` decompression.'
    path = Path(path)
    if path.endswith(b'.zst'):
        decompress = 'zstd'
    elif path.endswith(b'.gz') or path.endswith(b'.tgz'):
        decompress = 'gzip'
    else:
        with open(path, 'rb') as f:
            yield f
        return
    with subprocess.Popen([
        decompress, '--decompress', '--stdout', path,
    ], stdout=subprocess.PIPE) as proc:
        yield proc.stdout
    check_popen_returncode(proc)


def create_ro(path, mode):
    '`open` that creates (and never overwrites) a file with mode `a+r`.'
    def ro_opener(path, flags):
        return os.open(
            path,
            (flags & ~os.O_TRUNC) | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC,
            mode=stat.S_IRUSR | stat.S_IRGRP | stat.S_IROTH,
        )
    return open(path, mode, opener=ro_opener)


@contextmanager
def populate_temp_dir_and_rename(dest_path, *, overwrite=False) -> Path:
    '''
    Returns a Path to a temporary directory. The context block may populate
    this directory, which will then be renamed to `dest_path`, optionally
    deleting any preexisting directory (if `overwrite=True`).

    If the context block throws, the partially populated temporary directory
    is removed, while `dest_path` is left alone.

    By writing to a brand-new temporary directory before renaming, we avoid
    the problems of partially writing files, or overwriting some files but
    not others.  Moreover, populate-temporary-and-rename is robust to
    concurrent writers, and tends to work on broken NFSes unlike `flock`.
    '''
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
                if not (overwrite and ex.errno in [
                    # Different kernels have different error codes when the
                    # target already exists and is a nonempty directory.
                    errno.ENOTEMPTY, errno.EEXIST,
                ]):
                    raise
                log.exception(  # pragma: no cover
                    f'Retrying deleting {dest_path}, another writer raced us'
                )
            break  # We won the race
    except BaseException:
        shutil.rmtree(td)
        raise


@contextmanager
def populate_temp_file_and_rename(dest_path: Path, *, overwrite=False,
                                    mode='w'):
    '''
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
    '''
    with tempfile.NamedTemporaryFile(
        mode=mode, dir=dest_path.dirname(), delete=False,
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
