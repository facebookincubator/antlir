#!/usr/bin/env python3
import os
import unittest
import shutil
import subprocess
import tempfile
import volume_for_repo as vfr


class VolumeForRepoTestCase(unittest.TestCase):

    def test_volume_repo(self):
        artifacts_dir = tempfile.mkdtemp(prefix='test_volume_repo')
        volume_dir = os.path.join(artifacts_dir, vfr.VOLUME_DIR)
        image_file = os.path.join(artifacts_dir, vfr.IMAGE_FILE)
        min_free_bytes = 250e6

        try:
            self.assertEqual(
                vfr.get_volume_for_current_repo(
                    min_free_bytes=min_free_bytes,
                    artifacts_dir=artifacts_dir,
                ),
                volume_dir,
            )
            self.assertGreaterEqual(
                os.stat(image_file).st_size, min_free_bytes,
            )

            fstype_and_avail = subprocess.check_output([
                'findmnt', '--noheadings', '--output', 'FSTYPE,AVAIL',
                '--bytes', volume_dir
            ])
            fstype, avail = fstype_and_avail.strip().split()
            self.assertEqual(b'btrfs', fstype)
            self.assertGreaterEqual(int(avail), min_free_bytes)
        finally:
            try:
                subprocess.call(['sudo', 'umount', volume_dir])
            except Exception:
                pass  # Might not have been mounted in case of an earlier error
            shutil.rmtree(artifacts_dir)


if __name__ == '__main__':
    unittest.main()
