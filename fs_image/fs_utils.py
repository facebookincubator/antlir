#!/usr/bin/env python3
'Utilities to make Python systems programming more palatable.'
import errno
import os
import shutil
import stat
import subprocess
import urllib.parse
import tempfile

from contextlib import contextmanager
from typing import AnyStr, Iterable

from .common import byteme, check_popen_returncode, get_file_logger

log = get_file_logger(__file__)


# `pathlib` refuses to operate on `bytes`, which is the only sane way on Linux.
class Path(bytes):
    'A byte path that supports joining via the / operator.'

    def __new__(cls, arg, *args, **kwargs):
        return super().__new__(cls, byteme(arg), *args, **kwargs)

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

    def normpath(self) -> 'Path':
        return Path(os.path.normpath(self))

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

    def read_text(self) -> str:
        with open(self) as infile:
            return infile.read()


@contextmanager
def temp_dir(**kwargs) -> Iterable[Path]:
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
def populate_temp_dir_and_rename(dest_path, overwrite=False) -> Path:
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
