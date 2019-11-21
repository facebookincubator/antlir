#!/usr/bin/env python3
import bz2
import re
import sqlite3
import tempfile
import zlib

from collections import defaultdict
from contextlib import AbstractContextManager
from typing import Iterator, Union
from xml.etree import ElementTree

from .repo_objects import Checksum, Repodata, Rpm


class SQLiteRpmParser(AbstractContextManager):
    '''
    Extracts RPM location, checksum, and size from -primary.sqlite.{gz,bz2}.

    We always prefer SQLite over XMLRpmParser, but some weird repos (ahem,
    EPEL, ahem) do not ship SQLite metadata.  Unfortunately, it's far faster
    to write code than to convince people to fix their stuff, so both exist.

    ## Why do we avoid XML?

    Querying SQLite is FAR faster than XMLRpmParser, especially for .gz
    archives.  I ran experiments on a ~10,000 RPM CentOS repo, with an
    uncompressed 26MB XML and a 29MB SQLite:
     - .bz2 extraction takes 880ms
     - .gz extraction takes 230ms
     - .zst extraction takes 80ms and is 2% smaller than bz2
     - Once extracted, the SQLite query takes 20ms

    In contrast, .gz extraction + an XML parse take ~2 seconds, and this is
    in spite of the fact that XMLRpmParser is the fastest Python
    implementation out of ~5 iterations.  That said, even vanilla `libxml`
    needs about ~1 second to parse this file (piped through `xmllint`), so
    XML is simply not competitive.
    '''

    def __init__(self, path: str):
        self._path = path
        # Sadly, we must support both formats. Luckily, the APIs are similar.
        if path.endswith('.gz'):
            self._unpacker = zlib.decompressobj(wbits=zlib.MAX_WBITS + 16)
            self._unpacker_needs_input_and_next_chunk = lambda: (
                not self._unpacker.unconsumed_tail,
                self._unpacker.unconsumed_tail,
            )
        elif path.endswith('.bz2'):
            self._unpacker = bz2.BZ2Decompressor()
            self._unpacker_needs_input_and_next_chunk = lambda: (
                self._unpacker.needs_input, b'',
            )
        else:  # pragma: no cover -- testing this is not useful
            raise NotImplementedError(path)

    def __enter__(self):
        self._tmp_db_ctx = tempfile.NamedTemporaryFile()
        self._tmp_db = self._tmp_db_ctx.__enter__()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        # Clean up before maybe raising our own exception
        retval = self._tmp_db_ctx.__exit__(exc_type, exc_val, exc_tb)
        if exc_type is None and not self._unpacker.eof:
            raise RuntimeError(
                'Either the caller failed to consume feed(), or this archive '
                f'is incomplete: {self._path}'
            )
        if self._unpacker.unused_data:
            raise RuntimeError(f'Unused data after end of {self._path}')
        return retval

    def feed(self, chunk: bytes) -> Iterator[Rpm]:
        while True:
            self._tmp_db.write(
                # Don't use arbitrary amounts of RAM for decompression.
                # Bigger is better, within reason.  See the note on `zlib`
                # incremental complexity in `XMLRpmParser.feed`.
                self._unpacker.decompress(chunk, max_length=2 ** 23),
            )
            needs_input, chunk = self._unpacker_needs_input_and_next_chunk()
            if needs_input or self._unpacker.eof:
                break
        if self._unpacker.eof:  # We yield **everything** once the DB is ready
            self._tmp_db.flush()
            for loc, chk_type, chk_val, size, build_time in sqlite3.connect(
                self._tmp_db.name,
            ).execute(
                'SELECT "location_href", "checksum_type", "pkgId", '
                '"size_package", "time_build" FROM "packages";'
            ).fetchall():
                yield Rpm(
                    location=loc,
                    # The canonical checksum is set after we download the RPM
                    canonical_checksum=None,
                    checksum=Checksum(algorithm=chk_type, hexdigest=chk_val),
                    size=size,
                    build_timestamp=build_time,
                )


class XMLRpmParser(AbstractContextManager):
    '''
    Extracts RPM location, checksum, and size from -primary.xml.gz.  See the
    docblock of `SQLiteRpmParser` to learn why this parser is dispreferred,
    and why it exists anyway. Learnings from past iterations:
     - Avoid `minidom`: it is horrendously slow.
     - `Element.clear()` after every package helps a lot.
     - Feeding small (e.g. 16KB) chunks to XMLPullParser is a lot more
       performatn than feeding multi-megabyte chunks.
    '''

    def __init__(self):
        self.decompressor = zlib.decompressobj(wbits=zlib.MAX_WBITS + 16)
        self.xml_parser = ElementTree.XMLPullParser(['end'])
        # ElementTree mangles the tags thus: '{xml_namespace}tag_name'
        self.tag_re = re.compile(
            '({[^}]+}|)(location|size|checksum|package|time)$'
        )
        # Package state must persist across `feed()` calls, since a
        # package element may straddle a chunk boundary.
        self._package = {}

    # This context manager does not suppress exceptions.
    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        # Closing the parser detects incomplete XML files. It also breaks
        # some circular refs to speed up GC.
        self.xml_parser.close()

    def feed(self, chunk: bytes) -> Iterator[Rpm]:
        while chunk:
            # Consume the decompressed data in small chunks. This prevents
            # us from using unbounded amounts of RAM for decompression.
            # More crucially, apparently XMLPullParser gets up to 50% slower
            # on package data if we feed it larger chuks.  This buffer size
            # was picked experimentally :)
            #
            # NB: zlib appears to copy bytes into `unconsumed_tail` instead
            # of using something like `memoryview`, so this has poor
            # theoretical complexity due to all the extra copying.  I could
            # add an extra layer of input chunking to mitigate this, but in
            # practice it seems ok to just limit the incoming chunk size.
            self.xml_parser.feed(self.decompressor.decompress(
                chunk, max_length=2 ** 14,
            ))
            chunk = self.decompressor.unconsumed_tail
            for _, elt in self.xml_parser.read_events():
                m = self.tag_re.match(elt.tag)
                if m:
                    if m.group(2) == 'location':
                        self._package['location'] = elt.attrib['href']
                    elif m.group(2) == 'size':
                        self._package['size'] = elt.attrib['package']
                    elif m.group(2) == 'checksum':
                        assert elt.attrib['pkgid'] == 'YES'
                        self._package['checksum'] = Checksum(
                            algorithm=elt.attrib['type'], hexdigest=elt.text,
                        )
                    elif m.group(2) == 'time':
                        self._package['build_time'] = elt.attrib['build']
                    elif m.group(2) == 'package':
                        yield Rpm(
                            location=self._package['location'],
                            # This is set after we download the RPM
                            canonical_checksum=None,
                            checksum=self._package['checksum'],
                            size=int(self._package['size']),
                            build_timestamp=int(self._package['build_time']),
                        )
                        self._package = {}  # Detect missing fields
                        elt.clear()  # Uses less RAM, speeds up the run 50%


def pick_primary_repodata(repodatas: Repodata) -> Repodata:
    primaries = defaultdict(list)
    for rd in repodatas:
        if rd.is_primary_sqlite():
            primaries['sqlite'].append(rd)
        elif rd.is_primary_xml():
            primaries['xml'].append(rd)

    primaries = primaries.get('sqlite', primaries.get('xml'))  # Prefer SQLite

    if not primaries:
        raise RuntimeError(f'{repodatas} has no known primary file.')

    if len(primaries) > 1:
        raise RuntimeError(f'More than one primary of one type: {primaries}')

    return primaries[0]


def get_rpm_parser(repodata: Repodata) -> Union[SQLiteRpmParser, XMLRpmParser]:
    if repodata.is_primary_sqlite():
        return SQLiteRpmParser(repodata.location)
    elif repodata.is_primary_xml():
        return XMLRpmParser()
    assert False, f'Not reached: {repodata}'
