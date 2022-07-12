#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import email
import hashlib
import os
import socket
import sqlite3
import tempfile
import threading
import unittest
from contextlib import contextmanager
from typing import Mapping, Tuple

import requests
from antlir.fs_utils import temp_dir

from antlir.rpm.common import Checksum
from antlir.rpm.repo_objects import Repodata, RepoMetadata, Rpm
from antlir.rpm.repo_server import _CHUNK_SIZE, read_snapshot_dir, repo_server
from antlir.rpm.repo_snapshot import MutableRpmError, RepoSnapshot
from antlir.rpm.storage import Storage
from antlir.rpm.tests import temp_repos


# We need these fields to be real enough to satisfy `to_sqlite`.
_FAKE_RPM = Rpm(
    epoch=123,
    name="not null",
    version="not null",
    release="not null",
    arch="not null",
    build_timestamp=456,
    checksum="not null",
    canonical_checksum="not null",
    location=None,  # _replaced later
    size=789,
    source_rpm="not null",
)


def _checksum(algo: str, data: bytes) -> Checksum:
    h = hashlib.new(algo)
    h.update(data)
    return Checksum(algorithm=algo, hexdigest=h.hexdigest())


def _no_date(headers: Mapping[str, str]) -> Mapping[str, str]:
    """
    Comparing headers between two adjacent requests can break if they
    straddle a 00:00:01 boundary. So, ignore the date.
    """
    return {k.lower(): v for k, v in headers.items() if k.lower() != "date"}


class RepoServerTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        self.storage_dir_ctx = tempfile.TemporaryDirectory()  # noqa: P201
        storage_dir = self.storage_dir_ctx.__enter__()
        self.addCleanup(self.storage_dir_ctx.__exit__, None, None, None)
        # I could write some kind of in-memory storage, but this seems easier.
        self.storage = Storage.make(
            key="test", kind="filesystem", base_dir=storage_dir
        )

    @contextmanager
    def repo_server_thread(self, location_to_obj):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.bind(("127.0.0.1", 0))
        with repo_server(sock, location_to_obj, self.storage) as httpd:
            httpd.server_activate()
            thread = threading.Thread(name="RpSrv", target=httpd.serve_forever)
            thread.start()
            try:
                yield sock.getsockname()
            finally:
                httpd.shutdown()
                thread.join()

    # This validates a GET + headers, but not any of the storage retrieval.
    def test_repomd(self):
        content = b"An abacus falls from a fig tree"
        timestamp = 1234567890
        with self.repo_server_thread(
            {
                "repomd.xml": {
                    "size": len(content),
                    "build_timestamp": timestamp,
                    "content_bytes": content,
                }
            }
        ) as (host, port):
            req = requests.get(f"http://{host}:{port}/repomd.xml")
            req.raise_for_status()
            self.assertEqual(content, req.content)
            self.assertEqual(
                timestamp,
                email.utils.parsedate_to_datetime(
                    req.headers["last-modified"]
                ).timestamp(),
            )
            self.assertEqual("text/xml", req.headers["content-type"])

    def _write(self, content: bytes) -> Tuple[str, bytes]:
        with self.storage.writer() as out:
            out.write(content)
            return content, out.commit()

    def _prep_bad_blob(self, actual_size, expected_size, checksummed_size):
        content, sid = self._write(b"x" * actual_size)
        return {
            "size": expected_size,
            "build_timestamp": 0,
            "storage_id": sid,
            "checksum": str(_checksum("sha256", content[:checksummed_size])),
        }

    def _check_bad_blob(self, bad_blob):
        with self.repo_server_thread({"bad_blob": bad_blob}) as (host, port):
            # Drive-by test of HEAD requests -- note that this doesn't
            # detect the error yet, so the next GET "succeeds".
            req_head = requests.head(f"http://{host}:{port}/bad_blob")
            req = requests.get(f"http://{host}:{port}/bad_blob")
            self.assertEqual(req_head.status_code, req.status_code)
            self.assertEqual(_no_date(req_head.headers), _no_date(req.headers))
            # You'd think that `requests` would error on this, but, no...
            # https://blog.petrzemek.net/2018/04/22/
            #   on-incomplete-http-reads-and-the-requests-library-in-python/
            self.assertEqual(200, req.status_code)
            self.assertLess(
                req.raw.tell(),  # Number of bytes that were read
                int(req.headers["content-length"]),
            )
            # Ok, so we didn't get enough bytes, let's retry. This verifies
            # that the server memoizes integrity errors correctly.
            req = requests.get(f"http://{host}:{port}/bad_blob")
            self.assertEqual(500, req.status_code)
            self.assertIn(b"file_integrity", req.content)
            return req.content.decode()

    def _check_bad_size(self, actual, expected, checksummed):
        msg = self._check_bad_blob(
            self._prep_bad_blob(
                actual_size=actual,
                expected_size=expected,
                checksummed_size=checksummed,
            )
        )
        self.assertIn("'size'", msg)
        self.assertNotIn("'checksum'", msg)
        self.assertIn(str(actual), msg)
        self.assertIn(str(expected), msg)

    # Future: A slightly cleverer layout of this test would avoid spinniang
    # up and tearing down a bunch of servers, making it much faster.
    def test_bad_blobs(self):
        # Too small
        self._check_bad_size(
            actual=271828,
            expected=314159,
            checksummed=271828,  # Doesn't really matter
        )
        # Too large
        self._check_bad_size(
            actual=314159,
            expected=271828,
            checksummed=271828,  # If we just read 'expected', we wouldn't fail
        )
        # This edge case is that we have a too-large blob in storage, but
        # the server-side chunking ends exactly at the end of the declared
        # content size.  We want to make sure this still errors.
        self._check_bad_size(
            actual=_CHUNK_SIZE + 5,
            expected=_CHUNK_SIZE,
            # If we just read 'expected', we wouldn't fail
            checksummed=_CHUNK_SIZE,
        )
        # Bad checksum
        msg = self._check_bad_blob(
            self._prep_bad_blob(
                actual_size=314159,
                expected_size=314159,
                checksummed_size=271828,  # Oops!
            )
        )
        self.assertIn("'sha256'", msg)
        self.assertNotIn("'size'", msg)

    # This exercises `read_snapshot_dir` + typical access patterns with a
    # very minimal snapshot.
    def test_normal_snashot_dir_access(self):
        # We've got to populate RepoMetadata.xml with real XML because the
        # server re-parses that.  Note that the content of it isn't relevant
        # to the rest of the test, so it's fine to use a random repo.
        with temp_repos.temp_repos_steps(
            gpg_signing_key=temp_repos.get_test_signing_key(),
            repo_change_steps=[{"nil": temp_repos.Repo([])}],
        ) as repos_root, open(
            repos_root / "0/nil/repodata/repomd.xml", "rb"
        ) as infile:
            repomd = RepoMetadata.new(xml=infile.read())

        repodata_bytes, repodata_sid = self._write(b"A Repodata blob")
        repodata = Repodata(
            location="repodata/the_only",
            checksum=_checksum("sha256", repodata_bytes),
            size=len(repodata_bytes),
            build_timestamp=123,
        )

        rpm_bytes, rpm_sid = self._write(b"This is our test Rpm")
        rpm = _FAKE_RPM._replace(
            location="pkgs/good.rpm",
            checksum=_checksum("sha256", rpm_bytes),
            canonical_checksum=_checksum("sha384", rpm_bytes),
            size=len(rpm_bytes),
            build_timestamp=456,
        )

        rpm_mutable_bytes, rpm_mutable_sid = self._write(b"mutable")
        rpm_mutable = _FAKE_RPM._replace(
            location="pkgs/mutable.rpm",
            checksum=_checksum("sha256", rpm_mutable_bytes),
            canonical_checksum=_checksum("sha384", rpm_mutable_bytes),
            size=len(rpm_mutable_bytes),
            build_timestamp=789,
        )
        error_mutable_rpm = MutableRpmError(
            location=rpm_mutable.location,
            storage_id=rpm_mutable_sid,
            checksum=rpm_mutable.best_checksum(),
            other_checksums_and_universes={
                (_checksum("sha256", b"changeable"), "whateverse")
            },
        )

        with temp_dir() as td:
            sd = td / "snapshot"
            os.makedirs(sd)
            os.mkdir(sd / "yum.conf")  # yum.conf is ignored
            repo_dir = sd / "repos/mine"
            os.makedirs(repo_dir)
            with sqlite3.connect(sd / "snapshot.sql3") as db:
                RepoSnapshot._create_sqlite_tables(db)
                RepoSnapshot(
                    repomd=repomd,
                    storage_id_to_repodata={repodata_sid: repodata},
                    storage_id_to_rpm={
                        rpm_sid: rpm,
                        error_mutable_rpm: rpm_mutable,
                    },
                ).to_sqlite("mine", db)
            os.mkdir(repo_dir / "gpg_keys")
            with open(repo_dir / "gpg_keys" / "RPM-GPG-safekey", "wb") as outf:
                outf.write(b"public key")
            with self.repo_server_thread(read_snapshot_dir(td)) as (h, p):
                # A vanilla 404 doesn't affect the server's operation
                req = requests.get(f"http://{h}:{p}//DOES_NOT_EXIST")
                self.assertEqual(404, req.status_code)

                req = requests.get(f"http://{h}:{p}/mine/repodata/repomd.xml")
                req.raise_for_status()
                self.assertEqual(repomd.xml, req.content)

                req = requests.get(f"http://{h}:{p}/mine/repodata/the_only")
                req.raise_for_status()
                self.assertEqual(repodata_bytes, req.content)

                req = requests.get(f"http://{h}:{p}/mine/RPM-GPG-safekey")
                req.raise_for_status()
                self.assertEqual(b"public key", req.content)

                req = requests.get(f"http://{h}:{p}/mine/pkgs/good.rpm")
                req.raise_for_status()
                self.assertEqual(rpm_bytes, req.content)
                req = requests.get(f"http://{h}:{p}/mine/pkgs/mutable.rpm")
                self.assertEqual(500, req.status_code)
                self.assertIn(b"'mutable_rpm'", req.content)
