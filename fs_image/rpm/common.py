#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import hashlib
import inspect
import logging
import os
import shutil
import struct
import time
from functools import wraps

from io import BytesIO
from typing import Callable, Iterable, NamedTuple, Optional, TypeVar

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
    retryable_fn: Callable[[], T],
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    *,
    delays: Optional[Iterable[float]] = None,
    what: str,
    log_exception: bool = True,
) -> T:
    '''Allows functions to be retried `len(delays)` times, with each iteration
    sleeping for its respective index into `delays`. `is_exception_retryable`
    is an optional function that takes the raised exception and potentially
    evaluate to False, at which case no retry will occur and the exception will
    be re-raised. If the exception is not re-raised, the retry message will be
    logged to either DEBUG or ERROR depending whether `log_exception` is True.

    Delays are in seconds.
    '''
    if delays is None:
        delays = []
    for i, delay in enumerate(delays):
        try:
            return retryable_fn()
        except Exception as e:
            if is_exception_retryable and not is_exception_retryable(e):
                raise
            log.log(
                logging.ERROR if log_exception else logging.DEBUG,
                f'\n\n[Retry {i + 1} of {len(delays)}] {what} -- waiting '
                f'{delay} seconds.\n\n',
                exc_info=log_exception,
            )
            time.sleep(delay)
    return retryable_fn()  # With 0 retries, we should still run the function.


def retryable(
    format_msg: str,
    delays: Iterable[float],
    *,
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    log_exception: bool = True,
):
    '''Decorator used to retry a function if exceptions are thrown. `format_msg`
    should be a format string that can access any args provided to the
    decorated function. `delays` are the delays between retries, in seconds.
    `is_exception_retryable` and `log_exception` are forwarded to `retry_fn`,
    see its docblock.
    '''
    def wrapper(fn):
        @wraps(fn)
        def decorated(*args, **kwargs):
            fn_args = inspect.getcallargs(fn, *args, **kwargs)
            return retry_fn(
                lambda: fn(*args, **kwargs),
                is_exception_retryable,
                delays=delays,
                what=format_msg.format(**fn_args),
                log_exception=log_exception
            )
        return decorated
    return wrapper


async def async_retry_fn(
    retryable_fn: Callable[[], T],
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    *,
    delays: Optional[Iterable[float]] = None,
    what: str,
    log_exception: bool = True,
) -> T:
    ''' Similar to retry_fn except the function is executed asynchronously.
    See retry_fn docblock for details.
    '''
    if delays is None:
        delays = []
    for i, delay in enumerate(delays):
        try:
            return await retryable_fn()
        except Exception as e:
            if is_exception_retryable and not is_exception_retryable(e):
                raise
            log.log(
                logging.ERROR if log_exception else logging.DEBUG,
                f'\n\n[Retry {i + 1} of {len(delays)}] {what} -- waiting '
                f'{delay} seconds.\n\n',
                exc_info=log_exception,
            )
            time.sleep(delay)
    return await retryable_fn()  # With 0 retries, we should still run the function.


def async_retryable(
    format_msg: str,
    delays: Iterable[float],
    *,
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    log_exception: bool = True,
):
    '''Decorator used to retry an asynchronous function if exceptions are
    thrown. `format_msg` should be a format string that can access any args
    provided to the decorated function. `delays` are the delays between
    retries, in seconds. `is_exception_retryable` and `log_exception` are
    forwarded to `async_retry_fn`, see its docblock.
    '''
    def wrapper(fn):
        @wraps(fn)
        async def decorated(*args, **kwargs):
            fn_args = inspect.getcallargs(fn, *args, **kwargs)
            return await async_retry_fn(
                lambda: fn(*args, **kwargs),
                is_exception_retryable,
                delays=delays,
                what=format_msg.format(**fn_args),
                log_exception=log_exception
            )
        return decorated
    return wrapper


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


def yum_is_dnf():
    """ Determine if yum is really just dnf by looking at `which yum`"""
    yum_path = shutil.which('yum')

    # If yum does not exist or it's not a symlink then it's not dnf
    if not yum_path or not os.path.islink(yum_path):
        return False

    maybe_dnf = os.path.basename(os.readlink(yum_path))
    dnf_exists = os.path.exists(shutil.which(maybe_dnf))
    assert dnf_exists, f'Yum points to invalid path: {maybe_dnf}'

    # Inspect the name of the binary yum points to and assume that if
    # it starts with `dnf` its probably dnf
    return maybe_dnf.startswith('dnf')
