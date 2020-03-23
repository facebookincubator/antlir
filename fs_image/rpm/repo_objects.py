#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
The classes in this file are models of repo objects (metadata, data, blobs)
They have a shared interface, which lets us handle them somewhat uniformly.
In particular, the commonality of interface helps with:
 - writing them to a database,
 - estimating repo size,
 - checking integrity, etc.

All objects are expected to have:
    .checksum
    .best_checksum()
    .size
    .build_timestamp
'''
import hashlib
import time

from typing import Iterable, Iterator, NamedTuple
from xml.dom import minidom

from .common import Checksum

# Used as the common hash for all RPMs (and also repomd.xml). THIS CANNOT BE
# CHANGED WITHOUT MIGRATING THE DATABASE.  `sha384` is a good balance of
# x86_64 speed, security (including okish resistance to Grover's algorithm,
# and to length extension attacks), and availability (`sha384sum` or `shasum
# -a 384` is available on all modern Unices).
CANONICAL_HASH = 'sha384'


class Rpm(NamedTuple):

    # ENVRA

    epoch: int
    name: str
    version: str
    release: str
    arch: str

    # The remaining args are in lexicographic order

    build_timestamp: int  # build time from the primary metadata
    # pkgId from the current repo's primary repodata -- never None
    checksum: Checksum
    # Computed by us, see CANONICAL_HASH -- None until after the download
    canonical_checksum: Checksum
    # NB: The basename should be `n-v-r.a.rpm`, but we don't enforce this.
    location: str  # location href from the primary repodata
    size: int  # package size from the primary repodata
    source_rpm: str

    def nevra(self) -> str:
        r = self
        return f'{r.name}-{r.epoch}:{r.version}-{r.release}.{r.arch}'

    def best_checksum(self) -> Checksum:
        return self.canonical_checksum or self.checksum  # never None


class Repodata(NamedTuple):
    location: str  # <location href=...> from repomd.xml
    checksum: Checksum  # <checksum> from repomd.xml
    size: int  # <size> from repomd.xml
    # IMPORTANT: In the DB, we deduplicate Repodata blobs by checksum, and
    # only store the EARLIEST build_timestamp that we have seen.  It is
    # entirely possible for a `repomd.xml` is rebuilt, and to produce some
    # bit-for-bit identical Repodata blobs.  So, the checksum does not
    # correspond to a unique build timestamp.
    build_timestamp: int  # <timestamp> from repomd.xml

    def is_primary_sqlite(self) -> bool:
        return self.location.endswith('-primary.sqlite.bz2') or \
            self.location.endswith('-primary.sqlite.gz') or \
                self.location.endswith('-primary.sqlite.xz')

    def is_primary_xml(self) -> bool:
        return self.location.endswith('-primary.xml.gz')

    def best_checksum(self) -> Checksum:
        return self.checksum


def _parse_repomd(xml: bytes) -> Iterator[Repodata]:
    with minidom.parseString(xml) as repomd:
        for data in repomd.getElementsByTagName('data'):
            location_node, = data.getElementsByTagName('location')
            (attr_name, location_href), = location_node.attributes.items()
            assert attr_name == 'href'

            checksum_node, = data.getElementsByTagName('checksum')
            checksum_text_node, = checksum_node.childNodes
            (attr_name, checksum_type), = checksum_node.attributes.items()
            assert attr_name == 'type'

            size_node, = data.getElementsByTagName('size')
            size_text_node, = size_node.childNodes
            assert len(size_node.attributes) == 0

            timestamp_node, = data.getElementsByTagName('timestamp')
            timestamp_text_node, = timestamp_node.childNodes
            assert len(timestamp_node.attributes) == 0

            yield Repodata(
                checksum=Checksum(
                    algorithm=checksum_node.getAttribute('type'),
                    hexdigest=checksum_text_node.wholeText,
                ),
                location=location_href,
                size=int(size_text_node.wholeText),
                # Some repos have fractional seconds, but since they are not
                # critically useful, I find it easier to truncate here.
                build_timestamp=int(float(timestamp_text_node.wholeText)),
            )


class RepoMetadata(NamedTuple):
    xml: bytes
    fetch_timestamp: int
    repodatas: Iterable[Repodata]
    checksum: Checksum
    size: int  # Extracted from `xml` for the sake of RepoSizer
    # For ease of SQL queries, this is max(repodata.build_timestamp). This
    # is NOT the timestamp that's often found in the <revision> tag, since
    # there are no guarantees that this will be a timestamp always.
    build_timestamp: int

    # NamedTuple.__new__ cannot be overridden
    @classmethod
    def new(cls, *, xml: bytes) -> 'RepoMetadata':
        repodatas = frozenset(_parse_repomd(xml))
        return cls.__new__(
            cls,
            xml=xml,
            fetch_timestamp=int(time.time()),
            build_timestamp=max(r.build_timestamp for r in repodatas),
            repodatas=repodatas,
            checksum=Checksum(
                algorithm=CANONICAL_HASH,
                hexdigest=hashlib.new(CANONICAL_HASH, xml).hexdigest(),
            ),
            size=len(xml),
        )

    def best_checksum(self) -> Checksum:
        return self.checksum

    def __repr__(self):
        return f'RepoMetadata(fetch_timestamp: {self.fetch_timestamp}, checksum: {self.checksum}, size: {self.size}, build_timestamp: {self.build_timestamp}'
