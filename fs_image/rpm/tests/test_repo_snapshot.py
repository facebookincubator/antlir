#!/usr/bin/env python3
import json
import os
import tempfile
import unittest
import unittest.mock

from ..common import Checksum, Path
from ..repo_objects import Repodata, RepoMetadata, Rpm
from ..repo_snapshot import (
    FileIntegrityError, HTTPError, MutableRpmError, RepoSnapshot,
)


class RepoSnapshotTestCase(unittest.TestCase):

    def setUp(self):  # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def test_serialize_and_visit(self):
        repodata = Repodata(
            location='repodata_loc',
            checksum=Checksum('a', 'b'),
            size=123,
            build_timestamp=456,
        )
        repomd = RepoMetadata(
            xml=b'foo',
            # This object is only as populated as this test requires, in
            # practice these would not be None.
            fetch_timestamp=None,
            repodatas=None,
            checksum=None,
            size=None,
            build_timestamp=None,
        )
        rpm_base = Rpm(  # Reuse this template for all RPMs
            name=None, epoch=None, version=None, release=None, arch=None,
            build_timestamp=90,
            canonical_checksum=Checksum('e', 'f'),
            checksum=Checksum('c', 'd'),
            location=None,  # `_replace`d below
            size=78,
            source_rpm=None,
        )
        rpm_normal = rpm_base._replace(location='normal.rpm')
        rpm_file_integrity = rpm_base._replace(location='file_integrity_error')
        rpm_http = rpm_base._replace(location='http_error')
        rpm_mutable = rpm_base._replace(location='mutable_rpm_error')
        error_file_integrity = FileIntegrityError(
            location=rpm_file_integrity.location,
            failed_check='size',
            expected=42,
            actual=7,
        )
        error_http = HTTPError(location=rpm_http.location, http_status=404)
        error_mutable_rpm = MutableRpmError(
            location=rpm_mutable.location,
            storage_id='rpm_mutable_sid',
            checksum=Checksum('g', 'h'),
            other_checksums={Checksum('i', 'j'), Checksum('k', 'l')},
        )
        snapshot = RepoSnapshot(
            repomd=repomd,
            storage_id_to_repodata={'repodata_sid': repodata},
            storage_id_to_rpm={
                'rpm_normal_sid': rpm_normal,
                error_file_integrity: rpm_file_integrity,
                error_http: rpm_http,
                error_mutable_rpm: rpm_mutable,
            },
        )

        # Check the serialization
        with tempfile.TemporaryDirectory() as td:
            snapshot.to_directory(Path(td))
            self.assertEqual(
                sorted(['repomd.xml', 'repodata.json', 'rpm.json']),
                sorted(os.listdir(td)),
            )
            with open(os.path.join(td, 'repomd.xml'), 'rb') as f:
                self.assertEqual(b'foo', f.read())
            with open(os.path.join(td, 'repodata.json')) as f:
                self.assertEqual({
                    'repodata_loc': {
                        'checksum': 'a:b',
                        'size': 123,
                        'build_timestamp': 456,
                        'storage_id': 'repodata_sid',
                    }
                }, json.loads(f.read()))
            # We only serialize `best_checksum()`
            ser_base = {'checksum': 'e:f', 'size': 78, 'build_timestamp': 90}
            with open(os.path.join(td, 'rpm.json')) as f:
                self.assertEqual({
                    rpm_normal.location: {
                        'storage_id': 'rpm_normal_sid',
                        **ser_base,
                    },
                    rpm_file_integrity.location: {
                        'error': {
                            'error': 'file_integrity',
                            'message':
                                error_file_integrity.to_dict()['message'],
                            'location': rpm_file_integrity.location,
                            'failed_check': 'size',
                            # These are stringified because they might have
                            # been checksums... seems OK for now.
                            'expected': '42',
                            'actual': '7',
                        },
                        **ser_base,
                    },
                    rpm_http.location: {
                        'error': {
                            'error': 'http',
                            'message': error_http.to_dict()['message'],
                            'location': rpm_http.location,
                            'http_status': 404,
                        },
                        **ser_base,
                    },
                    rpm_mutable.location: {
                        'error': {
                            'error': 'mutable_rpm',
                            'message': error_mutable_rpm.to_dict()['message'],
                            'location': rpm_mutable.location,
                            'storage_id': 'rpm_mutable_sid',
                            'checksum': 'g:h',
                            'other_checksums': ['i:j', 'k:l'],
                        },
                        **ser_base,
                    },
                }, json.loads(f.read()))

        # Check the visitor
        mock = unittest.mock.MagicMock()
        snapshot.visit(mock)
        mock.visit_repomd.assert_called_once_with(repomd)
        mock.visit_repodata.assert_called_once_with(repodata)
        rpm_calls = set()
        for name, args, kwargs in mock.visit_rpm.mock_calls:
            self.assertEqual('', name)
            self.assertEqual({}, kwargs)
            self.assertEqual(1, len(args))
            rpm_calls.add(args[0])
        self.assertEqual(
            rpm_calls,
            {rpm_normal, rpm_file_integrity, rpm_http, rpm_mutable},
        )
