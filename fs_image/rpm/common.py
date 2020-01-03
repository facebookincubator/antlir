#!/usr/bin/env python3
import errno
import hashlib
import os
import shutil
import stat
import struct
import time
import tempfile
import urllib.parse

from contextlib import contextmanager
from io import BytesIO
from typing import AnyStr, Callable, Iterable, List, NamedTuple, TypeVar

# Hide the fact that some of our dependencies aren't in `rpm` any more, the
# `rpm` library still imports them from `rpm.common`.
from fs_image.common import (  # noqa: F401
    byteme, check_popen_returncode, get_file_logger, init_logging,
)
# Future: update dependencies to import this directly.
from fs_image.fs_utils import (  # noqa: F401
    create_ro, Path, populate_temp_dir_and_rename, temp_dir,
)

log = get_file_logger(__file__)
_UINT64_STRUCT = struct.Struct('=Q')
T = TypeVar('T')


def retry_fn(
    fn: Callable[[], T], *, delays: List[float] = None, what: str,
) -> T:
    'Delays are in seconds.'
    for i, delay in enumerate(delays):
        try:
            return fn()
        except Exception:
            log.exception(
                f'\n\n[Retry {i + 1} of {len(delays)}] {what} -- waiting '
                f'{delay} seconds.\n\n'
            )
            time.sleep(delay)
    return fn()  # With 0 retries, we should still run the function.


class RpmShard(NamedTuple):
    '''
    Used for testing, or for splitting a snapshot into parallel processes.
    In the latter case, each snapshot will redundantly fetch & store the
    metadata, so don't go overboard with the number of shards.
    '''
    shard: int
    modulo: int

    @classmethod
    def from_string(cls, shard_name: str) -> 'RpmShard':
        shard, mod = (int(v) for v in shard_name.split(':'))
        assert 0 <= shard < mod, f'Bad RPM shard: {shard_name}'
        return RpmShard(shard=shard, modulo=mod)

    def in_shard(self, rpm):
        # Our contract is that the RPM NEVRA is the global primary key,
        #
        # We use the last 8 bytes of SHA1, since we need a deterministic
        # hash for parallel downloads, and Python standard library lacks
        # fast non-cryptographic hashes like CityHash or SpookyHashV2.
        # adler32 is faster, but way too collision-prone to bother.
        h, = _UINT64_STRUCT.unpack_from(
            hashlib.sha1(byteme(rpm.nevra())).digest(), 12
        )
        return h % self.modulo == self.shard


class Checksum(NamedTuple):
    algorithm: str
    hexdigest: str

    @classmethod
    def from_string(cls, s: str) -> 'Checksum':
        algorithm, hexdigest = s.split(':')
        return cls(algorithm=algorithm, hexdigest=hexdigest)

    def __str__(self):
        return f'{self.algorithm}:{self.hexdigest}'

    def hasher(self):
        # Certain repos use "sha" to refer to "SHA-1", whereas in `hashlib`,
        # "sha" goes through OpenSSL and refers to a different digest.
        if self.algorithm == 'sha':
            return hashlib.sha1()
        return hashlib.new(self.algorithm)


def read_chunks(input: BytesIO, chunk_size: int) -> Iterable[bytes]:
    while True:
        chunk = input.read(chunk_size)
        if not chunk:
            break
        yield chunk
