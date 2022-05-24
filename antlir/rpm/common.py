#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import hashlib
import os
import shutil
import sqlite3
import struct
from contextlib import AbstractContextManager
from io import BytesIO
from typing import (
    ContextManager,
    Generic,
    Iterable,
    Iterator,
    NamedTuple,
    TypeVar,
)

from antlir.common import byteme, get_logger
from antlir.fs_utils import Path


# pyre-fixme[5]: Global expression must be annotated.
log = get_logger()
_UINT64_STRUCT = struct.Struct("=Q")
T = TypeVar("T")


def snapshot_subdir(snapshot_dir: Path) -> Path:
    return snapshot_dir / "snapshot"


def readonly_snapshot_db(snapshot_dir: Path) -> sqlite3.Connection:
    "Returns a read-only snapshot DB connection"
    db_path = snapshot_subdir(snapshot_dir) / "snapshot.sql3"
    if not db_path.exists():  # The SQLite error lacks the path.
        raise FileNotFoundError(db_path, "RPM snapshot lacks SQL3 DB")
    return sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)


class DecorateContextEntry(Generic[T], AbstractContextManager):
    """Lightweight helper class to decorate context manager __enter__"""

    # pyre-fixme[2]: Parameter must be annotated.
    def __init__(self, ctx_mgr: ContextManager[T], decorator) -> None:
        self._ctx_mgr = ctx_mgr
        # pyre-fixme[4]: Attribute must be annotated.
        self._decorator = decorator

    # pyre-fixme[3]: Return type must be annotated.
    def __enter__(self):
        return self._decorator(self._ctx_mgr.__enter__)()

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def __exit__(self, *args, **kwargs):
        return self._ctx_mgr.__exit__(*args, **kwargs)


class RpmShard(NamedTuple):
    """
    Used for testing, or for splitting a snapshot into parallel processes.
    In the latter case, each snapshot will redundantly fetch & store the
    metadata, so don't go overboard with the number of shards.
    """

    shard: int
    modulo: int

    @classmethod
    def from_string(cls, shard_name: str) -> "RpmShard":
        shard, mod = (int(v) for v in shard_name.split(":"))
        assert 0 <= shard < mod, f"Bad RPM shard: {shard_name}"
        return RpmShard(shard=shard, modulo=mod)

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def in_shard(self, rpm):
        (  # Our contract is that the RPM NEVRA is the global primary key,
            #
            # We use the last 8 bytes of SHA1, since we need a deterministic
            # hash for parallel downloads, and Python standard library lacks
            # fast non-cryptographic hashes like CityHash or SpookyHashV2.
            # adler32 is faster, but way too collision-prone to bother.
            h,
        ) = _UINT64_STRUCT.unpack_from(
            hashlib.sha1(byteme(rpm.nevra())).digest(), 12
        )
        return h % self.modulo == self.shard


class Checksum(NamedTuple):
    algorithm: str
    hexdigest: str

    @classmethod
    def from_string(cls, s: str) -> "Checksum":
        algorithm, hexdigest = s.split(":")
        return cls(algorithm=algorithm, hexdigest=hexdigest)

    def __str__(self) -> str:
        return f"{self.algorithm}:{self.hexdigest}"

    # pyre-fixme[3]: Return type must be annotated.
    def hasher(self):
        # Certain repos use "sha" to refer to "SHA-1", whereas in `hashlib`,
        # "sha" goes through OpenSSL and refers to a different digest.
        if self.algorithm == "sha":
            return hashlib.sha1()
        return hashlib.new(self.algorithm)


def read_chunks(input: BytesIO, chunk_size: int) -> Iterable[bytes]:
    while True:
        chunk = input.read(chunk_size)
        if not chunk:
            break
        yield chunk


def has_yum() -> bool:
    """Determine if our system might have yum with `which yum`"""
    if not shutil.which("yum"):
        return False
    return True


# pyre-fixme[3]: Return type must be annotated.
def yum_is_dnf():
    """Determine if yum is really just dnf by looking at `which yum`"""
    yum_path = shutil.which("yum")

    if not yum_path:
        return False

    # If yum is not a symlink then it's not dnf
    if not os.path.islink(yum_path):
        return False

    maybe_dnf = os.path.basename(os.readlink(yum_path))
    # pyre-fixme[6]: For 1st param expected `Union[PathLike[bytes], PathLike[str],
    #  bytes, int, str]` but got `Optional[str]`.
    dnf_exists = os.path.exists(shutil.which(maybe_dnf))
    assert dnf_exists, f"Yum points to invalid path: {maybe_dnf}"

    # Inspect the name of the binary yum points to and assume that if
    # it starts with `dnf` its probably dnf
    return maybe_dnf.startswith("dnf")
