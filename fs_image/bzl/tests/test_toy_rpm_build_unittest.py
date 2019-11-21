#!/usr/bin/env python3
import os
import subprocess
import unittest

from rpm.rpm_metadata import RpmMetadata


class ToyRpmBuildUnittestTest(unittest.TestCase):

    def test_built_files(self):
        # Files added as part of the rpmbuild_layer
        self.assertTrue(os.path.exists('/rpmbuild/SOURCES/toy_src_file'))
        self.assertTrue(os.path.exists('/rpmbuild/SPECS/specfile.spec'))

        # Built from rpmbuild
        rpm_path = b'/rpmbuild/RPMS/toy.rpm'
        self.assertTrue(os.path.exists(rpm_path))

        a = RpmMetadata.from_file(rpm_path)
        self.assertEqual(a.epoch, 0)
        self.assertEqual(a.version, '1.0')
        self.assertEqual(a.release, '1')

        # Check files contained in rpm
        files = subprocess.check_output(['rpm', '-qlp', rpm_path]).decode()
        self.assertEqual(files, '/usr/bin/toy_src_file\n')
